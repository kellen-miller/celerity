//! Authority-free, content-addressed vehicle-to-home synchronization.

use std::time::{SystemTime, UNIX_EPOCH};

mod api;
mod artifact;
mod configuration;
pub(crate) mod inventory;
mod journal;
mod network;
mod transfer;

pub use api::{HomeApi, ReqwestHomeApi};
pub use configuration::SyncConfiguration;
pub use journal::TransferJournal;
pub use network::{LinuxNetworkPresence, NetworkPresence};
pub use transfer::{ReconcileOutcome, Synchronizer};

/// Sync owns transfer state but cannot grant vehicle authority.
pub const HAS_CONTROL_AUTHORITY: bool = false;

#[derive(Debug, Eq, PartialEq)]
pub struct SyncError(String);

impl std::fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SyncError {}

fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            i64::try_from(duration.as_secs()).unwrap_or(i64::MAX)
        })
}

fn sync_io(error: std::io::Error) -> SyncError {
    SyncError(error.to_string())
}

fn sync_sql(error: rusqlite::Error) -> SyncError {
    SyncError(error.to_string())
}

fn sync_http(error: reqwest::Error) -> SyncError {
    SyncError(error.to_string())
}

fn sync_toml(error: toml::de::Error) -> SyncError {
    SyncError(error.to_string())
}
