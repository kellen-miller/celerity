use std::{
    fs::{self, File},
    path::Path,
    time::Duration,
};

pub(crate) use celerity_proto::celerity::v1::{CompletedRun, ReconcileRequest, ReconcileResponse};
use prost::Message;
use reqwest::blocking::Client;

use super::{SyncError, sync_http, sync_io};

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

pub struct ReqwestHomeApi {
    base_url: String,
    token: String,
    client: Client,
}

impl ReqwestHomeApi {
    /// Creates the production HTTPS-capable client from a bearer-token file.
    ///
    /// # Errors
    ///
    /// Returns an error when the token or HTTP client cannot be loaded.
    pub fn new(base_url: &str, token_path: &Path) -> Result<Self, SyncError> {
        let token = fs::read_to_string(token_path).map_err(sync_io)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(sync_http)?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.trim().to_owned(),
            client,
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::RequestBuilder {
        self.client
            .request(method, format!("{}{path}", self.base_url))
            .bearer_auth(&self.token)
    }
}

impl HomeApi for ReqwestHomeApi {
    fn reconcile(&self, request: &ReconcileRequest) -> Result<ReconcileResponse, SyncError> {
        let response = self
            .request(reqwest::Method::POST, "/v1/reconcile")
            .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
            .body(request.encode_to_vec())
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(sync_http)?;
        ReconcileResponse::decode(response.bytes().map_err(sync_http)?.as_ref())
            .map_err(|error| SyncError(error.to_string()))
    }

    fn put_chunk(&self, run: &str, chunk: &str, path: &Path) -> Result<(), SyncError> {
        let file = File::open(path).map_err(sync_io)?;
        self.request(
            reqwest::Method::PUT,
            &format!("/v1/runs/{run}/chunks/{chunk}"),
        )
        .body(file)
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(sync_http)?;
        Ok(())
    }

    fn complete(&self, run: &str, manifest: &[u8]) -> Result<CompletedRun, SyncError> {
        let response = self
            .request(reqwest::Method::POST, &format!("/v1/runs/{run}/complete"))
            .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
            .body(manifest.to_vec())
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(sync_http)?;
        CompletedRun::decode(response.bytes().map_err(sync_http)?.as_ref())
            .map_err(|error| SyncError(error.to_string()))
    }

    fn get_model(&self, digest: &str) -> Result<Vec<u8>, SyncError> {
        self.request(reqwest::Method::GET, &format!("/v1/models/{digest}"))
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(sync_http)?
            .bytes()
            .map(|bytes| bytes.to_vec())
            .map_err(sync_http)
    }
}
