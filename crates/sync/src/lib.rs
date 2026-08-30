//! Authority-free, content-addressed vehicle-to-home synchronization.

use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use reqwest::blocking::Client;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Sync owns transfer state but cannot grant vehicle authority.
pub const HAS_CONTROL_AUTHORITY: bool = false;

#[derive(Clone, Debug, PartialEq)]
pub struct SyncConfiguration {
    pub spool_root: PathBuf,
    pub retention_count: usize,
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

#[derive(Debug, Eq, PartialEq)]
pub struct SyncError(String);

impl std::fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SyncError {}

pub trait NetworkPresence {
    /// Reports whether the configured link is up with the configured default route.
    ///
    /// # Errors
    ///
    /// Returns an error when platform network state cannot be read.
    fn is_home(&self, interface: &str, expected_gateway: &str) -> Result<bool, SyncError>;
}

/// Reads Linux link and default-route state. HTTP availability is deliberately
/// not part of this decision.
pub struct LinuxNetworkPresence {
    sys_class_net: PathBuf,
    proc_net_route: PathBuf,
}

impl LinuxNetworkPresence {
    #[must_use]
    pub fn host() -> Self {
        Self {
            sys_class_net: PathBuf::from("/sys/class/net"),
            proc_net_route: PathBuf::from("/proc/net/route"),
        }
    }

    #[must_use]
    pub fn from_paths(sys_class_net: PathBuf, proc_net_route: PathBuf) -> Self {
        Self {
            sys_class_net,
            proc_net_route,
        }
    }
}

impl NetworkPresence for LinuxNetworkPresence {
    fn is_home(&self, interface: &str, expected_gateway: &str) -> Result<bool, SyncError> {
        let operstate = fs::read_to_string(self.sys_class_net.join(interface).join("operstate"))
            .map_err(sync_io)?;
        if operstate.trim() != "up" {
            return Ok(false);
        }
        let route = fs::read_to_string(&self.proc_net_route).map_err(sync_io)?;
        for line in route.lines().skip(1) {
            let columns: Vec<_> = line.split_ascii_whitespace().collect();
            if columns.len() < 4 || columns[0] != interface || columns[1] != "00000000" {
                continue;
            }
            let flags = u16::from_str_radix(columns[3], 16).unwrap_or_default();
            if flags & 0x3 != 0x3 {
                continue;
            }
            let encoded = u32::from_str_radix(columns[2], 16).unwrap_or_default();
            let gateway = encoded
                .to_le_bytes()
                .map(|octet| octet.to_string())
                .join(".");
            return Ok(gateway == expected_gateway);
        }
        Ok(false)
    }
}

pub struct TransferJournal {
    connection: Connection,
}

impl TransferJournal {
    /// Opens and migrates the sync-owned `SQLite` transfer journal.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or migrated.
    pub fn open(path: &Path) -> Result<Self, SyncError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(sync_io)?;
        }
        let connection = Connection::open(path).map_err(sync_sql)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS run_transfer (
                   run_digest TEXT PRIMARY KEY,
                   run_path TEXT NOT NULL,
                   acknowledged INTEGER NOT NULL DEFAULT 0,
                   attempts INTEGER NOT NULL DEFAULT 0,
                   last_error TEXT
                 );
                 CREATE TABLE IF NOT EXISTS model_transfer (
                   digest TEXT PRIMARY KEY,
                   state TEXT NOT NULL,
                   last_error TEXT
                 );",
            )
            .map_err(sync_sql)?;
        Ok(Self { connection })
    }

    fn observe_run(&self, run: &SpoolRun) -> Result<(), SyncError> {
        self.connection
            .execute(
                "INSERT INTO run_transfer(run_digest, run_path) VALUES (?1, ?2)
                 ON CONFLICT(run_digest) DO UPDATE SET run_path=excluded.run_path",
                params![run.digest, run.directory.to_string_lossy()],
            )
            .map_err(sync_sql)?;
        Ok(())
    }

    fn record_attempt(
        &self,
        digest: &str,
        result: &Result<(), SyncError>,
    ) -> Result<(), SyncError> {
        let (acknowledged, error) = match result {
            Ok(()) => (1, None),
            Err(error) => (0, Some(error.to_string())),
        };
        self.connection
            .execute(
                "UPDATE run_transfer SET attempts=attempts+1, acknowledged=?2, last_error=?3
                 WHERE run_digest=?1",
                params![digest, acknowledged, error],
            )
            .map_err(sync_sql)?;
        Ok(())
    }

    #[must_use]
    pub fn acknowledged(&self, digest: &str) -> bool {
        self.connection
            .query_row(
                "SELECT acknowledged FROM run_transfer WHERE run_digest=?1",
                [digest],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
    }
}

#[derive(Clone, Debug)]
struct SpoolRun {
    digest: String,
    directory: PathBuf,
    manifest: Vec<u8>,
    chunk_path: PathBuf,
    chunk_digest: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ReconcileRequest {
    schema_version: u32,
    completed_run_digests: Vec<String>,
    active_model_digest: Option<String>,
    staged_model_digest: Option<String>,
    rejected_model_digests: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReconcileResponse {
    missing_run_digests: Vec<String>,
    desired_model_digest: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelBundleManifest {
    schema_version: u32,
    onnx_sha256: String,
    signal_order: Vec<String>,
    units: Vec<String>,
    sample_period_ms: u64,
    history_length: usize,
    horizons: Vec<u64>,
    output_order: Vec<String>,
    command_lattice: Vec<u16>,
    compatibility: ModelCompatibility,
    input_ranges: Vec<ModelInputRange>,
    normalization: Vec<ModelNormalization>,
    calibration_error: f64,
}

#[derive(Deserialize)]
struct ModelCompatibility {
    model_abi: String,
    input_shape: Vec<usize>,
}

#[derive(Deserialize)]
struct ModelInputRange {
    minimum: f64,
    maximum: f64,
}

#[derive(Deserialize)]
struct ModelNormalization {
    mean: f64,
    scale: f64,
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
    fn put_chunk(&self, run: &str, chunk: &str, bytes: &[u8]) -> Result<(), SyncError>;
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
            .timeout(Duration::from_secs(30))
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

    fn put_chunk(&self, run: &str, chunk: &str, bytes: &[u8]) -> Result<(), SyncError> {
        self.request(
            reqwest::Method::PUT,
            &format!("/v1/runs/{run}/chunks/{chunk}"),
        )
        .body(bytes.to_vec())
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconcileOutcome {
    Away,
    Synchronized { uploaded: usize, model_staged: bool },
}

pub struct Synchronizer {
    configuration: SyncConfiguration,
    journal: TransferJournal,
}

impl Synchronizer {
    /// Creates a synchronizer and migrates its durable transfer journal.
    ///
    /// # Errors
    ///
    /// Returns an error when the journal cannot be opened.
    pub fn new(configuration: SyncConfiguration) -> Result<Self, SyncError> {
        let journal = TransferJournal::open(&configuration.journal_path)?;
        Ok(Self {
            configuration,
            journal,
        })
    }

    /// Performs one bounded inventory, transfer, retention, and model pass.
    ///
    /// # Errors
    ///
    /// Returns an error when local artifacts, network state, journal state, or
    /// the home API cannot be validated.
    pub fn reconcile_once(
        &mut self,
        network: &impl NetworkPresence,
        home: &impl HomeApi,
        active_model_digest: Option<&str>,
        staged_model_digest: Option<&str>,
        rejected_model_digests: &[String],
    ) -> Result<ReconcileOutcome, SyncError> {
        if !network.is_home(
            &self.configuration.home_interface,
            &self.configuration.expected_default_gateway,
        )? {
            return Ok(ReconcileOutcome::Away);
        }
        let runs = inventory(&self.configuration.spool_root)?;
        for run in &runs {
            self.journal.observe_run(run)?;
        }
        let request = serde_json::to_value(ReconcileRequest {
            schema_version: 1,
            completed_run_digests: runs.iter().map(|run| run.digest.clone()).collect(),
            active_model_digest: active_model_digest.map(str::to_owned),
            staged_model_digest: staged_model_digest.map(str::to_owned),
            rejected_model_digests: rejected_model_digests.to_vec(),
        })
        .map_err(sync_json)?;
        let response: ReconcileResponse =
            serde_json::from_value(home.reconcile(&request)?).map_err(sync_json)?;
        let missing: BTreeSet<_> = response.missing_run_digests.into_iter().collect();
        let mut uploaded = 0;
        for run in runs.iter().filter(|run| missing.contains(&run.digest)) {
            let result = Self::upload(home, run);
            self.journal.record_attempt(&run.digest, &result)?;
            result?;
            uploaded += 1;
        }
        self.apply_retention(&runs)?;
        let model_staged = if let Some(digest) = response.desired_model_digest {
            if active_model_digest == Some(digest.as_str())
                || staged_model_digest == Some(digest.as_str())
            {
                false
            } else {
                self.stage_model(home, &digest)?
            }
        } else {
            false
        };
        Ok(ReconcileOutcome::Synchronized {
            uploaded,
            model_staged,
        })
    }

    fn upload(home: &impl HomeApi, run: &SpoolRun) -> Result<(), SyncError> {
        let bytes = fs::read(&run.chunk_path).map_err(sync_io)?;
        if digest(&bytes) != run.chunk_digest {
            return Err(SyncError(
                "Run chunk digest changed before upload".to_owned(),
            ));
        }
        home.put_chunk(&run.digest, &run.chunk_digest, &bytes)?;
        let acknowledgement = home.complete(&run.digest, &run.manifest)?;
        if acknowledgement != run.digest {
            return Err(SyncError(
                "home acknowledged a different Run digest".to_owned(),
            ));
        }
        Ok(())
    }

    fn stage_model(&self, home: &impl HomeApi, expected_digest: &str) -> Result<bool, SyncError> {
        fs::create_dir_all(&self.configuration.model_root).map_err(sync_io)?;
        let read_marker = |name: &str| {
            fs::read_to_string(self.configuration.model_root.join(name))
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let active = read_marker("active-digest");
        let known_good = read_marker("known-good-digest");
        let slot_digest = |slot: &str| {
            fs::read_to_string(
                self.configuration
                    .model_root
                    .join(slot)
                    .join("bundle-digest"),
            )
            .ok()
            .map(|value| value.trim().to_owned())
        };
        let active_slot = active.as_ref().and_then(|active_digest| {
            ["slot-a", "slot-b"]
                .into_iter()
                .find(|slot| slot_digest(slot).as_ref() == Some(active_digest))
        });
        let protected_known_good = if active == known_good {
            None
        } else {
            known_good.as_deref()
        };
        let Some(inactive) = ["slot-a", "slot-b"].into_iter().find(|slot| {
            Some(*slot) != active_slot
                && protected_known_good
                    .is_none_or(|digest| slot_digest(slot).as_deref() != Some(digest))
        }) else {
            return Ok(false);
        };

        let bytes = home.get_model(expected_digest)?;
        if digest(&bytes) != expected_digest {
            return Err(SyncError("downloaded model digest mismatch".to_owned()));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
            .map_err(|error| SyncError(format!("invalid model bundle: {error}")))?;
        if archive.len() != 2 {
            return Err(SyncError(
                "model bundle must contain exactly two entries".to_owned(),
            ));
        }
        let manifest_bytes = read_zip_entry(&mut archive, "manifest.json")?;
        let onnx_bytes = read_zip_entry(&mut archive, "model.onnx")?;
        let manifest: ModelBundleManifest =
            serde_json::from_slice(&manifest_bytes).map_err(sync_json)?;
        let expected_input = self
            .configuration
            .model_history_length
            .saturating_mul(self.configuration.model_input_signals.len())
            .saturating_add(1);
        let valid = manifest.schema_version == 1
            && digest(&onnx_bytes) == manifest.onnx_sha256
            && manifest.signal_order == self.configuration.model_input_signals
            && manifest.units.len() == manifest.signal_order.len()
            && manifest.sample_period_ms > 0
            && manifest.history_length == self.configuration.model_history_length
            && manifest.horizons == [1]
            && manifest.output_order == ["coolant", "post_intercooler_iat"]
            && manifest.command_lattice == self.configuration.model_command_lattice
            && manifest.compatibility.model_abi == self.configuration.model_abi
            && manifest.compatibility.input_shape == [1, expected_input]
            && manifest.input_ranges.len() == manifest.signal_order.len()
            && manifest.normalization.len() == manifest.signal_order.len()
            && manifest.input_ranges.iter().all(|range| {
                range.minimum.is_finite()
                    && range.maximum.is_finite()
                    && range.minimum <= range.maximum
            })
            && manifest.normalization.iter().all(|value| {
                value.mean.is_finite() && value.scale.is_finite() && value.scale > 0.0
            })
            && manifest.calibration_error.is_finite()
            && manifest.calibration_error >= 0.0
            && manifest.calibration_error <= self.configuration.model_maximum_calibration_error;
        if !valid {
            return Err(SyncError(
                "model bundle is incompatible with live configuration".to_owned(),
            ));
        }
        let slot = self.configuration.model_root.join(inactive);
        fs::create_dir_all(&slot).map_err(sync_io)?;
        atomic_write(&slot.join("model.onnx"), &onnx_bytes)?;
        atomic_write(&slot.join("manifest.json"), &manifest_bytes)?;
        atomic_write(
            &slot.join("bundle-digest"),
            format!("{expected_digest}\n").as_bytes(),
        )?;
        atomic_write(
            &self.configuration.model_root.join("desired-digest"),
            format!("{expected_digest}\n").as_bytes(),
        )?;
        Ok(true)
    }

    fn apply_retention(&self, runs: &[SpoolRun]) -> Result<(), SyncError> {
        let mut acknowledged: Vec<_> = runs
            .iter()
            .filter(|run| self.journal.acknowledged(&run.digest))
            .collect();
        acknowledged.sort_by(|left, right| left.directory.cmp(&right.directory));
        let remove_count = acknowledged
            .len()
            .saturating_sub(self.configuration.retention_count);
        for run in acknowledged.into_iter().take(remove_count) {
            fs::remove_dir_all(&run.directory).map_err(sync_io)?;
        }
        Ok(())
    }
}

fn inventory(root: &Path) -> Result<Vec<SpoolRun>, SyncError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(root).map_err(sync_io)? {
        let directory = entry.map_err(sync_io)?.path();
        let manifest_path = directory.join("manifest.json");
        if !directory.is_dir() || !manifest_path.exists() {
            continue;
        }
        let manifest = fs::read(&manifest_path).map_err(sync_io)?;
        let value: serde_json::Value = serde_json::from_slice(&manifest).map_err(sync_json)?;
        if value.get("completion").and_then(serde_json::Value::as_str) != Some("complete") {
            continue;
        }
        let chunk_file = value
            .get("chunk_file")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| SyncError("complete Run has no chunk_file".to_owned()))?;
        let chunk_digest = value
            .get("chunk_sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| SyncError("complete Run has no chunk_sha256".to_owned()))?;
        runs.push(SpoolRun {
            digest: digest(&manifest),
            directory: directory.clone(),
            manifest,
            chunk_path: directory.join(chunk_file),
            chunk_digest: chunk_digest.to_owned(),
        });
    }
    runs.sort_by(|left, right| left.digest.cmp(&right.digest));
    Ok(runs)
}

fn digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(output, "{byte:02x}").expect("String write");
    }
    output
}

fn read_zip_entry(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
) -> Result<Vec<u8>, SyncError> {
    let mut entry = archive
        .by_name(name)
        .map_err(|error| SyncError(format!("model bundle is missing {name}: {error}")))?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).map_err(sync_io)?;
    Ok(bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SyncError> {
    let temporary = path.with_extension("staging");
    let mut file = File::create(&temporary).map_err(sync_io)?;
    file.write_all(bytes).map_err(sync_io)?;
    file.sync_all().map_err(sync_io)?;
    drop(file);
    fs::rename(temporary, path).map_err(sync_io)
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

fn sync_json(error: serde_json::Error) -> SyncError {
    SyncError(error.to_string())
}

fn sync_toml(error: toml::de::Error) -> SyncError {
    SyncError(error.to_string())
}
