use std::{
    fs::{self, File},
    path::Path,
    time::Duration,
};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

use super::{SyncError, sync_http, sync_io};

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct ReconcileRequest {
    pub(crate) schema_version: u32,
    pub(crate) completed_run_digests: Vec<String>,
    pub(crate) active_model_digest: Option<String>,
    pub(crate) staged_model_digest: Option<String>,
    pub(crate) rejected_model_digests: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ReconcileResponse {
    pub(crate) missing_run_digests: Vec<String>,
    pub(crate) desired_model_digest: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelBundleManifest {
    pub(crate) schema_version: u32,
    pub(crate) onnx_sha256: String,
    pub(crate) signal_order: Vec<String>,
    pub(crate) units: Vec<String>,
    pub(crate) sample_period_ms: u64,
    pub(crate) history_length: usize,
    pub(crate) horizons: Vec<u64>,
    pub(crate) output_order: Vec<String>,
    pub(crate) command_lattice: Vec<u16>,
    pub(crate) compatibility: ModelCompatibility,
    pub(crate) input_ranges: Vec<ModelInputRange>,
    pub(crate) normalization: Vec<ModelNormalization>,
    pub(crate) calibration_error: f64,
}

#[derive(Deserialize)]
pub(crate) struct ModelCompatibility {
    pub(crate) model_abi: String,
    pub(crate) input_shape: Vec<usize>,
}

#[derive(Deserialize)]
pub(crate) struct ModelInputRange {
    pub(crate) minimum: f64,
    pub(crate) maximum: f64,
}

#[derive(Deserialize)]
pub(crate) struct ModelNormalization {
    pub(crate) mean: f64,
    pub(crate) scale: f64,
}

pub trait HomeApi {
    /// Sends the content inventory and obtains missing/desired digests.
    ///
    /// # Errors
    ///
    /// Returns an error for transport, status, or response failures.
    fn reconcile(&self, request: &serde_json::Value) -> Result<serde_json::Value, SyncError>;
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
    fn complete(&self, run: &str, manifest: &[u8]) -> Result<String, SyncError>;
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
    fn reconcile(&self, request: &serde_json::Value) -> Result<serde_json::Value, SyncError> {
        self.request(reqwest::Method::POST, "/v1/reconcile")
            .json(request)
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(sync_http)?
            .json()
            .map_err(sync_http)
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

    fn complete(&self, run: &str, manifest: &[u8]) -> Result<String, SyncError> {
        #[derive(Deserialize)]
        struct Acknowledgement {
            run_digest: String,
        }
        let acknowledgement: Acknowledgement = self
            .request(reqwest::Method::POST, &format!("/v1/runs/{run}/complete"))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(manifest.to_vec())
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .map_err(sync_http)?
            .json()
            .map_err(sync_http)?;
        Ok(acknowledgement.run_digest)
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
