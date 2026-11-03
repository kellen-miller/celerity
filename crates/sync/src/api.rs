use std::{fs, future::Future, path::Path, time::Duration};

pub(crate) use celerity_proto::celerity::v1::{CompletedRun, ReconcileRequest, ReconcileResponse};
use celerity_proto::{
    HOME_MAX_MESSAGE_BYTES,
    celerity::v1::{
        CompleteRunRequest, DownloadModelRequest, UploadChunkRequest, home_client::HomeClient,
    },
};
use tokio::runtime::{Builder, Handle, Runtime};
use tonic::{
    Request, Status,
    metadata::{Ascii, MetadataValue},
    transport::{Channel, ClientTlsConfig, Endpoint},
};

use super::{SyncError, sync_io};

const RPC_TIMEOUT: Duration = Duration::from_secs(30);

pub trait HomeApi {
    /// Sends the content inventory and obtains missing/desired digests.
    ///
    /// # Errors
    ///
    /// Returns an error for transport, status, or response failures.
    fn reconcile(&self, request: &ReconcileRequest) -> Result<ReconcileResponse, SyncError>;

    /// Idempotently uploads one exact content-addressed chunk.
    ///
    /// # Errors
    ///
    /// Returns an error for transport or unsuccessful status.
    fn put_chunk(&self, run: &str, chunk: &str, path: &Path) -> Result<(), SyncError>;

    /// Completes a Run and returns the server's exact acknowledgement digest.
    ///
    /// # Errors
    ///
    /// Returns an error for transport, status, or response failures.
    fn complete(&self, run: &str, manifest: &[u8]) -> Result<CompletedRun, SyncError>;

    /// Downloads one immutable model bundle.
    ///
    /// # Errors
    ///
    /// Returns an error for transport or unsuccessful status.
    fn get_model(&self, digest: &str) -> Result<Vec<u8>, SyncError>;
}

/// Synchronous Home RPC transport with an explicitly owned current-thread runtime.
/// Construct, call, and drop this client outside an asynchronous runtime.
pub struct GrpcHomeApi {
    client: HomeClient<Channel>,
    authorization: MetadataValue<Ascii>,
    runtime: Runtime,
}

impl GrpcHomeApi {
    /// Creates a lazy HTTPS-capable channel from a bearer-token file.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid credentials, endpoint, runtime, or nested runtime use.
    pub fn new(base_url: &str, token_path: &Path) -> Result<Self, SyncError> {
        if Handle::try_current().is_ok() {
            return Err(SyncError(
                "synchronous Home client requires a caller outside a Tokio runtime".into(),
            ));
        }

        let token = fs::read_to_string(token_path).map_err(sync_io)?;
        if token.trim().is_empty() {
            return Err(SyncError("empty Home bearer token".into()));
        }

        let authorization = format!("Bearer {}", token.trim())
            .parse()
            .map_err(|error| SyncError(format!("invalid Home bearer token: {error}")))?;
        let mut endpoint = Endpoint::from_shared(base_url.trim_end_matches('/').to_owned())
            .map_err(|error| SyncError(error.to_string()))?
            .connect_timeout(Duration::from_secs(5))
            .timeout(RPC_TIMEOUT);
        if endpoint.uri().scheme_str() == Some("https") {
            endpoint = endpoint
                .tls_config(ClientTlsConfig::new().with_native_roots())
                .map_err(|error| SyncError(error.to_string()))?;
        }

        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(sync_io)?;
        let client = {
            let _entered = runtime.enter();
            HomeClient::new(endpoint.connect_lazy())
                .max_encoding_message_size(HOME_MAX_MESSAGE_BYTES)
                .max_decoding_message_size(HOME_MAX_MESSAGE_BYTES)
        };
        Ok(Self {
            client,
            authorization,
            runtime,
        })
    }

    fn authenticated<T>(&self, message: T) -> Result<Request<T>, SyncError> {
        if Handle::try_current().is_ok() {
            return Err(SyncError(
                "synchronous Home RPC requires a caller outside a Tokio runtime".into(),
            ));
        }

        let mut request = Request::new(message);
        request
            .metadata_mut()
            .insert("authorization", self.authorization.clone());
        request.set_timeout(RPC_TIMEOUT);
        Ok(request)
    }

    fn wait_for_rpc<T>(
        &self,
        rpc: impl Future<Output = Result<tonic::Response<T>, Status>>,
    ) -> Result<T, SyncError> {
        self.runtime
            .block_on(async { tokio::time::timeout(RPC_TIMEOUT, rpc).await })
            .map_err(|_| {
                SyncError("home gRPC DeadlineExceeded: full RPC exceeded 30 seconds".into())
            })?
            .map(tonic::Response::into_inner)
            .map_err(sync_status)
    }
}

impl HomeApi for GrpcHomeApi {
    fn reconcile(&self, request: &ReconcileRequest) -> Result<ReconcileResponse, SyncError> {
        let request = self.authenticated(request.clone())?;
        let mut client = self.client.clone();
        self.wait_for_rpc(client.reconcile(request))
    }

    fn put_chunk(&self, run: &str, chunk: &str, path: &Path) -> Result<(), SyncError> {
        let request = self.authenticated(UploadChunkRequest {
            run_digest: run.to_owned(),
            chunk_digest: chunk.to_owned(),
            data: fs::read(path).map_err(sync_io)?,
        })?;
        let mut client = self.client.clone();
        self.wait_for_rpc(client.upload_chunk(request)).map(|_| ())
    }

    fn complete(&self, run: &str, manifest: &[u8]) -> Result<CompletedRun, SyncError> {
        let request = self.authenticated(CompleteRunRequest {
            run_digest: run.to_owned(),
            manifest_pb: manifest.to_vec(),
        })?;
        let mut client = self.client.clone();
        self.wait_for_rpc(client.complete_run(request))
    }

    fn get_model(&self, digest: &str) -> Result<Vec<u8>, SyncError> {
        let request = self.authenticated(DownloadModelRequest {
            digest: digest.to_owned(),
        })?;
        let mut client = self.client.clone();
        self.wait_for_rpc(client.download_model(request))
            .map(|response| response.bundle_zip)
    }
}

fn sync_status(status: Status) -> SyncError {
    SyncError(format!(
        "home gRPC {:?}: {}",
        status.code(),
        status.message()
    ))
}
