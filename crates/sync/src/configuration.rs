use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;

use super::{SyncError, sync_io, sync_toml};

#[derive(Clone, Debug, PartialEq)]
pub struct SyncConfiguration {
    pub spool_root: PathBuf,
    pub retention_count: usize,
    pub incomplete_retention_count: usize,
    pub home_interface: String,
    pub expected_default_gateway: String,
    pub home_api_url: String,
    pub credential_path: PathBuf,
    pub journal_path: PathBuf,
    pub model_root: PathBuf,
    pub model_abi: String,
    pub model_input_signals: Vec<String>,
    pub model_history_length: usize,
    pub model_command_lattice: Vec<u16>,
    pub model_maximum_calibration_error: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleSyncConfiguration {
    spool_root: String,
    retention_count: usize,
    incomplete_retention_count: usize,
    home_interface: String,
    expected_default_gateway: String,
    home_api_url: String,
    credential_path: String,
}

#[derive(Deserialize)]
struct BundleModelConfiguration {
    abi: String,
    input_signals: Vec<String>,
    history_length: usize,
    command_lattice: Vec<u16>,
    maximum_uncertainty: f64,
}

impl SyncConfiguration {
    /// Loads only the authority-free sync section from the strict runtime
    /// bundle and supplies sync-owned deployment paths explicitly.
    ///
    /// # Errors
    ///
    /// Returns an error for unreadable TOML, absent/unknown sync fields, or
    /// invalid sync settings.
    pub fn from_bundle(
        bundle_path: &Path,
        journal_path: PathBuf,
        model_root: PathBuf,
    ) -> Result<Self, SyncError> {
        let source = fs::read_to_string(bundle_path).map_err(sync_io)?;
        let root: toml::Value = toml::from_str(&source).map_err(sync_toml)?;
        let sync = root
            .get("sync")
            .cloned()
            .ok_or_else(|| SyncError("bundle has no sync section".to_owned()))?
            .try_into::<BundleSyncConfiguration>()
            .map_err(sync_toml)?;
        let model = root
            .get("model")
            .cloned()
            .ok_or_else(|| SyncError("bundle has no model section".to_owned()))?
            .try_into::<BundleModelConfiguration>()
            .map_err(sync_toml)?;
        if sync.spool_root.is_empty()
            || sync.retention_count == 0
            || sync.incomplete_retention_count == 0
            || sync.home_interface.is_empty()
            || sync.expected_default_gateway.is_empty()
            || !sync.home_api_url.starts_with("https://")
            || sync.credential_path.is_empty()
        {
            return Err(SyncError("invalid sync configuration".to_owned()));
        }
        Ok(Self {
            spool_root: PathBuf::from(sync.spool_root),
            retention_count: sync.retention_count,
            incomplete_retention_count: sync.incomplete_retention_count,
            home_interface: sync.home_interface,
            expected_default_gateway: sync.expected_default_gateway,
            home_api_url: sync.home_api_url,
            credential_path: PathBuf::from(sync.credential_path),
            journal_path,
            model_root,
            model_abi: model.abi,
            model_input_signals: model.input_signals,
            model_history_length: model.history_length,
            model_command_lattice: model.command_lattice,
            model_maximum_calibration_error: model.maximum_uncertainty,
        })
    }
}
