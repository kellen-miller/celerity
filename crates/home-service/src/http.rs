use std::{io::Write, sync::Arc};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use celerity_proto::celerity::v1::{CompletedRun, Completion, Health, RunChunk, RunManifest};
use prost::Message;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use serde_json::Value;
use subtle::ConstantTimeEq;

use crate::{HomeService, digest, is_digest, jobs, reconcile, run, storage};

pub(crate) const PROTOBUF: &str = "application/x-protobuf";
pub(crate) type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    detail: Value,
}

#[derive(Serialize)]
struct ErrorBody {
    detail: Value,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, detail: impl Into<Value>) -> Self {
        Self {
            status,
            detail: detail.into(),
        }
    }

    pub(crate) fn invalid(detail: impl Into<Value>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, detail)
    }

    pub(crate) fn conflict(detail: impl Into<Value>) -> Self {
        Self::new(StatusCode::CONFLICT, detail)
    }

    pub(crate) fn internal(error: impl std::fmt::Display) -> Self {
        eprintln!("home request failed: {error}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                detail: self.detail,
            }),
        )
            .into_response()
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

pub(crate) fn protobuf(message: &impl Message, status: StatusCode) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, PROTOBUF)],
        message.encode_to_vec(),
    )
        .into_response()
}

pub(crate) fn router(home: HomeService) -> Router {
    let home = Arc::new(home);
    let protected = Router::new()
        .route("/v1/reconcile", post(reconcile::reconcile))
        .route(
            "/v1/runs/{run_digest}/chunks/{chunk_digest}",
            put(put_chunk),
        )
        .route("/v1/runs/{run_digest}/complete", post(complete_run))
        .route("/v1/models/{digest}", get(get_model))
        .route("/v1/jobs", post(jobs::start_job))
        .route("/v1/jobs/{job_id}", get(jobs::get_job))
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&home),
            authenticate,
        ));
    Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready))
        .merge(protected)
        .layer(DefaultBodyLimit::disable())
        .with_state(home)
}

async fn authenticate(
    State(home): State<Arc<HomeService>>,
    request: Request,
    next: Next,
) -> Response {
    let expected = format!("Bearer {}", home.config.vehicle_token);
    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .map_or(&[][..], |value| value.as_bytes());
    if !bool::from(provided.ct_eq(expected.as_bytes())) {
        return ApiError::new(StatusCode::UNAUTHORIZED, "invalid vehicle token").into_response();
    }

    next.run(request).await
}

async fn live() -> Response {
    protobuf(
        &Health {
            status: "live".into(),
        },
        StatusCode::OK,
    )
}

async fn ready(State(home): State<Arc<HomeService>>) -> ApiResult<Response> {
    storage::connect(&home.database())
        .map_err(ApiError::internal)?
        .query_row("SELECT 1", [], |_| Ok(()))?;
    Ok(protobuf(
        &Health {
            status: "ready".into(),
        },
        StatusCode::OK,
    ))
}

async fn put_chunk(
    State(home): State<Arc<HomeService>>,
    Path((run_digest, chunk_digest)): Path<(String, String)>,
    data: Bytes,
) -> ApiResult<StatusCode> {
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
    transaction.execute(
        "INSERT OR IGNORE INTO chunks(run_digest, digest, path) VALUES (?, ?, ?)",
        params![run_digest, chunk_digest, path.to_string_lossy()],
    )?;
    transaction.commit()?;
    Ok(StatusCode::NO_CONTENT)
}

async fn complete_run(
    State(home): State<Arc<HomeService>>,
    Path(run_digest): Path<String>,
    data: Bytes,
) -> ApiResult<Response> {
    if !is_digest(&run_digest) {
        return Err(ApiError::invalid("invalid digest"));
    }

    if digest(&data) != run_digest {
        return Err(ApiError::invalid("Run digest mismatch"));
    }

    let mut manifest = RunManifest::decode(data.as_ref())
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
    let existing: Option<Vec<u8>> = transaction
        .query_row(
            "SELECT manifest_pb FROM runs WHERE digest=?",
            [&run_digest],
            |row| row.get(0),
        )
        .optional()?;
    if existing.is_some_and(|bytes| bytes != data) {
        return Err(ApiError::conflict("immutable Run conflict"));
    }

    transaction.execute(
        "INSERT OR IGNORE INTO runs(digest, manifest_pb) VALUES (?, ?)",
        params![run_digest, data.as_ref()],
    )?;
    transaction.commit()?;
    Ok(protobuf(&CompletedRun { run_digest }, StatusCode::OK))
}

async fn get_model(
    State(home): State<Arc<HomeService>>,
    Path(digest): Path<String>,
) -> ApiResult<Response> {
    if !is_digest(&digest) {
        return Err(ApiError::invalid("invalid digest"));
    }

    let connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let path: Option<String> = connection
        .query_row(
            "SELECT path FROM models WHERE digest=? AND state='staged'",
            [digest],
            |row| row.get(0),
        )
        .optional()?;
    let path = path.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "model not found"))?;
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        std::fs::read(path)?,
    )
        .into_response())
}
