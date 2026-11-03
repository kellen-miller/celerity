use std::io::Write;

use celerity_proto::celerity::v1::{
    CompleteRunRequest, CompletedRun, Completion, DownloadModelRequest, DownloadModelResponse,
    GetJobRequest, HomeApiError, JobRequest, JobStarted, JobStatus, ReconcileRequest,
    ReconcileResponse, RunChunk, RunManifest, UploadChunkRequest, UploadChunkResponse,
    home_server::Home,
};
use prost::Message;
use rusqlite::TransactionBehavior;
use subtle::ConstantTimeEq;
use tonic::{Code, Request, Response, Status, service::Interceptor};

use celerity_sql::home as sql;

use crate::{HomeService, digest, is_digest, jobs, reconcile, run, storage};

pub(crate) type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub(crate) struct ApiError {
    code: Code,
    body: HomeApiError,
}

impl ApiError {
    pub(crate) fn new(code: Code, message: impl Into<String>) -> Self {
        Self {
            code,
            body: HomeApiError {
                schema_version: 1,
                message: message.into(),
                incomplete_run_digests: Vec::new(),
            },
        }
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(Code::InvalidArgument, message)
    }

    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self::new(Code::FailedPrecondition, message)
    }

    pub(crate) fn incomplete_runs(digests: Vec<String>) -> Self {
        Self {
            code: Code::FailedPrecondition,
            body: HomeApiError {
                schema_version: 1,
                message: "Runs are incomplete".into(),
                incomplete_run_digests: digests,
            },
        }
    }

    pub(crate) fn internal(error: impl std::fmt::Display) -> Self {
        eprintln!("home request failed: {error}");
        Self::new(Code::Internal, "internal server error")
    }
}

impl From<ApiError> for Status {
    fn from(error: ApiError) -> Self {
        Self::with_details(
            error.code,
            &error.body.message,
            error.body.encode_to_vec().into(),
        )
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        Self::internal(error)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::internal(error)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(error: serde_json::Error) -> Self {
        Self::internal(error)
    }
}

/// Required bearer authentication on every Home RPC; health remains public.
#[derive(Clone)]
pub struct HomeAuthentication {
    expected: Vec<u8>,
}

impl HomeAuthentication {
    pub(crate) fn new(token: &str) -> Self {
        Self {
            expected: format!("Bearer {token}").into_bytes(),
        }
    }
}

impl Interceptor for HomeAuthentication {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        let provided = request
            .metadata()
            .get("authorization")
            .map_or(&[][..], |value| value.as_encoded_bytes());
        if !bool::from(provided.ct_eq(&self.expected)) {
            return Err(ApiError::new(Code::Unauthenticated, "invalid vehicle token").into());
        }

        Ok(request)
    }
}

#[tonic::async_trait]
impl Home for HomeService {
    async fn reconcile(
        &self,
        request: Request<ReconcileRequest>,
    ) -> Result<Response<ReconcileResponse>, Status> {
        reconcile::reconcile(self, request.into_inner())
            .map(Response::new)
            .map_err(Into::into)
    }

    async fn upload_chunk(
        &self,
        request: Request<UploadChunkRequest>,
    ) -> Result<Response<UploadChunkResponse>, Status> {
        put_chunk(self, request.into_inner())
            .map(Response::new)
            .map_err(Into::into)
    }

    async fn complete_run(
        &self,
        request: Request<CompleteRunRequest>,
    ) -> Result<Response<CompletedRun>, Status> {
        complete_run(self, request.into_inner())
            .map(Response::new)
            .map_err(Into::into)
    }

    async fn download_model(
        &self,
        request: Request<DownloadModelRequest>,
    ) -> Result<Response<DownloadModelResponse>, Status> {
        get_model(self, request.into_inner().digest)
            .map(Response::new)
            .map_err(Into::into)
    }

    async fn start_job(
        &self,
        request: Request<JobRequest>,
    ) -> Result<Response<JobStarted>, Status> {
        jobs::start_job(self, request.into_inner())
            .map(Response::new)
            .map_err(Into::into)
    }

    async fn get_job(
        &self,
        request: Request<GetJobRequest>,
    ) -> Result<Response<JobStatus>, Status> {
        jobs::get_job(self, request.into_inner().job_id)
            .map(Response::new)
            .map_err(Into::into)
    }
}

fn put_chunk(home: &HomeService, request: UploadChunkRequest) -> ApiResult<UploadChunkResponse> {
    let UploadChunkRequest {
        run_digest,
        chunk_digest,
        data,
    } = request;
    if !is_digest(&run_digest) || !is_digest(&chunk_digest) {
        return Err(ApiError::invalid("invalid digest"));
    }

    if digest(&data) != chunk_digest {
        return Err(ApiError::invalid("chunk digest mismatch"));
    }

    let directory = home.config.storage_root.join("runs").join(&run_digest);
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(&chunk_digest);
    if path.exists() && std::fs::read(&path)? != data {
        return Err(ApiError::conflict("immutable chunk conflict"));
    }

    let temporary = directory.join(format!(".{chunk_digest}.{}.staging", uuid::Uuid::new_v4()));
    let mut stream = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    stream.write_all(&data)?;
    stream.sync_all()?;
    match std::fs::hard_link(&temporary, &path) {
        Ok(()) => (),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::read(&path)? != data {
                std::fs::remove_file(&temporary)?;
                return Err(ApiError::conflict("immutable chunk conflict"));
            }
        }
        Err(error) => {
            if let Err(cleanup) = std::fs::remove_file(&temporary) {
                eprintln!("chunk staging cleanup failed: {cleanup}");
            }

            return Err(error.into());
        }
    }

    std::fs::remove_file(temporary)?;
    std::fs::File::open(&directory)?.sync_all()?;
    let mut connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    sql::InsertChunk::builder()
        .run_digest(&run_digest)
        .digest(&chunk_digest)
        .path(&path.to_string_lossy())
        .build()
        .execute(&transaction)?;
    transaction.commit()?;
    Ok(UploadChunkResponse {})
}

fn complete_run(home: &HomeService, request: CompleteRunRequest) -> ApiResult<CompletedRun> {
    let CompleteRunRequest {
        run_digest,
        manifest_pb: data,
    } = request;
    if !is_digest(&run_digest) {
        return Err(ApiError::invalid("invalid digest"));
    }

    if digest(&data) != run_digest {
        return Err(ApiError::invalid("Run digest mismatch"));
    }

    let mut manifest = RunManifest::decode(data.as_slice())
        .map_err(|_| ApiError::invalid("invalid Run manifest"))?;
    if manifest.chunks.is_empty() {
        if manifest.chunk_sha256.is_empty() {
            return Err(ApiError::invalid("invalid Run manifest"));
        }

        manifest.chunks.push(RunChunk {
            sha256: manifest.chunk_sha256.clone(),
            file: String::new(),
        });
    }

    if manifest.schema_version != 1 || manifest.completion != Completion::Complete as i32 {
        return Err(ApiError::invalid(
            "only complete Run v1 manifests are admitted",
        ));
    }

    let mut last_sequence = None;
    let mut last_monotonic_ns = None;
    for chunk in &manifest.chunks {
        if !is_digest(&chunk.sha256) {
            return Err(ApiError::invalid("invalid Run chunk manifest"));
        }

        let path = home
            .config
            .storage_root
            .join("runs")
            .join(&run_digest)
            .join(&chunk.sha256);
        let chunk_bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(error.into()),
        };
        if chunk_bytes.is_empty() || digest(&chunk_bytes) != chunk.sha256 {
            return Err(ApiError::invalid("manifest chunk is absent or corrupt"));
        }

        let bounds = run::validate_records(&chunk_bytes)
            .map_err(|error| ApiError::invalid(format!("invalid Run chunk: {error}")))?;
        if last_sequence.is_some_and(|sequence| bounds.first_sequence <= sequence) {
            return Err(ApiError::invalid("Run sequence regressed across chunks"));
        }

        if last_monotonic_ns.is_some_and(|time| bounds.first_monotonic_ns < time) {
            return Err(ApiError::invalid("Run time regressed across chunks"));
        }

        last_sequence = Some(bounds.last_sequence);
        last_monotonic_ns = Some(bounds.last_monotonic_ns);
    }

    let mut connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let existing = sql::GetRun::builder()
        .digest(&run_digest)
        .build()
        .query_opt(&transaction)?
        .map(|row| row.manifest_pb);
    if existing.is_some_and(|bytes| bytes != data) {
        return Err(ApiError::conflict("immutable Run conflict"));
    }

    sql::InsertRun::builder()
        .digest(&run_digest)
        .manifest(data.as_ref())
        .build()
        .execute(&transaction)?;
    transaction.commit()?;
    Ok(CompletedRun { run_digest })
}

fn get_model(home: &HomeService, digest: String) -> ApiResult<DownloadModelResponse> {
    if !is_digest(&digest) {
        return Err(ApiError::invalid("invalid digest"));
    }

    let connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let path = sql::GetStagedModel::builder()
        .digest(&digest)
        .build()
        .query_opt(&connection)?
        .map(|row| row.path);
    let path = path.ok_or_else(|| ApiError::new(Code::NotFound, "model not found"))?;
    Ok(DownloadModelResponse {
        bundle_zip: std::fs::read(path)?,
    })
}
