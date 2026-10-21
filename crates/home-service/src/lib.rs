//! Home owns durable admission and job state; Python owns numerical training.

use std::path::PathBuf;

mod artifact;
mod http;
pub mod jobs;
mod migration;
pub mod notifications;
mod reconcile;
pub mod run;
pub mod storage;
mod training;

/// Errors crossing filesystem, SQLite and process boundaries.
pub type HomeResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Required production dependencies for one home service instance.
#[derive(Clone, Debug)]
pub struct HomeConfig {
    pub storage_root: PathBuf,
    pub vehicle_token: String,
    pub webhook_url: String,
    pub python: PathBuf,
    pub worker_binary: PathBuf,
}

/// Operation-scoped SQLite connections share this immutable configuration.
#[derive(Clone, Debug)]
pub struct HomeService {
    pub config: HomeConfig,
}

impl HomeService {
    /// Initialize storage without starting background work.
    ///
    /// # Errors
    /// Returns invalid configuration, filesystem or migration failures.
    pub fn open(config: HomeConfig) -> HomeResult<Self> {
        if !config.webhook_url.starts_with("https://") {
            return Err("CELERITY_HOME_WEBHOOK_URL must use HTTPS".into());
        }

        if config.python.as_os_str().is_empty() || config.worker_binary.as_os_str().is_empty() {
            return Err("Python and worker executable paths are required".into());
        }

        std::fs::create_dir_all(&config.storage_root)?;
        storage::migrate(&config.storage_root.join("home.sqlite3"))?;
        Ok(Self { config })
    }

    /// Build the existing protobuf HTTP surface; recovery remains explicit.
    pub fn router(&self) -> axum::Router {
        http::router(self.clone())
    }

    /// Recover queued work and fail dead running workers.
    ///
    /// # Errors
    /// Returns SQLite, process launch or ownership inspection failures.
    pub fn recover_jobs(&self) -> HomeResult<usize> {
        jobs::recover(&self.config)
    }

    #[must_use]
    pub fn database(&self) -> PathBuf {
        self.config.storage_root.join("home.sqlite3")
    }
}

pub(crate) fn digest(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(data))
}

pub(crate) fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
