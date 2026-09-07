//! Typed, read-only vehicle diagnostics and bounded Unix transport.

use std::{
    path::Path,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use celerity_proto::celerity::v1::{
    self as proto, CommandSource as ProtoCommandSource,
    DiagnosticHealthState as ProtoDiagnosticHealthState, DiagnosticStatus as ProtoDiagnosticStatus,
    DiagnosticUnknownReason as ProtoDiagnosticUnknownReason,
    FeatureAuthority as ProtoFeatureAuthority, GlobalAuthority as ProtoGlobalAuthority,
    RunStorageHealth as ProtoRunStorageHealth,
};
use prost::Message;
use serde::{Deserialize, Serialize};

const IO_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GlobalAuthority {
    Fallback,
    Active,
    HardFault,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureAuthority {
    Fallback,
    Arming,
    Active,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    ControllerLocalFallback,
    Deterministic,
    ModelOptimized,
    Experiment,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticUnknownReason {
    NeverObserved,
    NotExpectedInFallback,
    NotRenewing,
    NoOutstandingCommand,
    AwaitingEvidence,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", content = "reason", rename_all = "snake_case")]
pub enum DiagnosticStatus {
    Unknown(DiagnosticUnknownReason),
    Healthy,
    Unhealthy,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStorageHealth {
    Unknown,
    Healthy,
    Degraded,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ObservedTemperature {
    pub degrees_celsius: f64,
    pub observation_age_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AcceptedRadiatorSplitCommand {
    pub basis_points: u16,
    pub source: CommandSource,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DiagnosticsSnapshot {
    pub schema_version: u32,
    pub runtime_update_age_ms: u64,
    pub runtime_update_stale_after_ms: u64,
    pub global_authority: GlobalAuthority,
    pub feature_authority: FeatureAuthority,
    pub command_source: CommandSource,
    pub accepted_radiator_split_command: Option<AcceptedRadiatorSplitCommand>,
    pub coolant_temperature: Option<ObservedTemperature>,
    pub intake_air_temperature: Option<ObservedTemperature>,
    pub controller_runtime_lease_health: DiagnosticStatus,
    pub controller_command_ack_health: DiagnosticStatus,
    pub run_storage_health: RunStorageHealth,
}

impl DiagnosticsSnapshot {
    #[must_use]
    pub fn to_proto(&self) -> proto::DiagnosticsSnapshot {
        protobuf::to_proto(self)
    }

    fn from_proto(snapshot: proto::DiagnosticsSnapshot) -> Result<Self, String> {
        protobuf::from_proto(snapshot)
    }
}
mod protobuf;

impl DiagnosticsSnapshot {
    #[must_use]
    pub const fn startup_fallback() -> Self {
        Self {
            schema_version: 2,
            runtime_update_age_ms: 0,
            runtime_update_stale_after_ms: 0,
            global_authority: GlobalAuthority::Fallback,
            feature_authority: FeatureAuthority::Fallback,
            command_source: CommandSource::ControllerLocalFallback,
            accepted_radiator_split_command: None,
            coolant_temperature: None,
            intake_air_temperature: None,
            controller_runtime_lease_health: DiagnosticStatus::Unknown(
                DiagnosticUnknownReason::NotExpectedInFallback,
            ),
            controller_command_ack_health: DiagnosticStatus::Unknown(
                DiagnosticUnknownReason::NoOutstandingCommand,
            ),
            run_storage_health: RunStorageHealth::Unknown,
        }
    }
}

#[derive(Clone)]
pub struct DiagnosticsStore {
    shared: Arc<RwLock<PublishedSnapshot>>,
}

struct PublishedSnapshot {
    snapshot: DiagnosticsSnapshot,
    published_at: Instant,
    runtime_update_stale_after_ms: u64,
}

impl DiagnosticsStore {
    #[must_use]
    pub fn new(mut snapshot: DiagnosticsSnapshot, cycle_ms: u64, published_at: Instant) -> Self {
        let runtime_update_stale_after_ms = cycle_ms.saturating_mul(5).max(100);
        snapshot.runtime_update_age_ms = 0;
        snapshot.runtime_update_stale_after_ms = runtime_update_stale_after_ms;
        Self {
            shared: Arc::new(RwLock::new(PublishedSnapshot {
                snapshot,
                published_at,
                runtime_update_stale_after_ms,
            })),
        }
    }

    /// Replaces the published evidence and its monotonic publication stamp.
    ///
    /// # Errors
    ///
    /// Returns an error when another thread poisoned the diagnostics lock.
    pub fn publish(
        &self,
        mut snapshot: DiagnosticsSnapshot,
        published_at: Instant,
    ) -> Result<(), String> {
        let mut published = self.shared.write().map_err(|error| error.to_string())?;
        snapshot.runtime_update_age_ms = 0;
        snapshot.runtime_update_stale_after_ms = published.runtime_update_stale_after_ms;
        published.snapshot = snapshot;
        published.published_at = published_at;
        drop(published);
        Ok(())
    }

    fn snapshot(&self, now: Instant) -> Result<DiagnosticsSnapshot, String> {
        let published = self.shared.read().map_err(|error| error.to_string())?;
        let mut snapshot = published.snapshot.clone();
        snapshot.runtime_update_age_ms = u64::try_from(
            now.saturating_duration_since(published.published_at)
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        drop(published);
        Ok(snapshot)
    }
}

#[cfg(unix)]
/// Starts the read-only diagnostics socket worker.
///
/// # Errors
///
/// Returns an error when the socket cannot be prepared, bound, or configured.
pub fn serve_diagnostics(
    socket_path: &Path,
    stopping: Arc<AtomicBool>,
    store: DiagnosticsStore,
) -> Result<thread::JoinHandle<Result<(), String>>, std::io::Error> {
    use std::{fs, os::unix::fs::PermissionsExt, os::unix::net::UnixListener};

    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }
    if socket_path.exists() {
        fs::remove_file(socket_path)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    fs::set_permissions(socket_path, fs::Permissions::from_mode(0o660))?;
    listener.set_nonblocking(true)?;
    let socket_path = socket_path.to_path_buf();
    thread::Builder::new()
        .name("celerity-read-only-diagnostics".to_owned())
        .spawn(move || {
            while !stopping.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        use std::io::{Read, Write};

                        if let Err(error) = stream
                            .set_read_timeout(Some(IO_TIMEOUT))
                            .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
                        {
                            eprintln!("diagnostics connection configuration failed: {error}");
                            continue;
                        }
                        let mut request = [0_u8; 64];
                        let read = match stream.read(&mut request) {
                            Ok(read) => read,
                            Err(error) => {
                                eprintln!("diagnostics request read failed: {error}");
                                continue;
                            }
                        };
                        if &request[..read] != b"status\n" {
                            if let Err(error) = stream
                                .write_all(b"{\"error\":\"read-only status request required\"}\n")
                            {
                                eprintln!("diagnostics rejection write failed: {error}");
                            }
                            continue;
                        }
                        let current = match store.snapshot(Instant::now()) {
                            Ok(current) => current,
                            Err(error) => {
                                eprintln!("diagnostics snapshot read failed: {error}");
                                continue;
                            }
                        };
                        let response = current.to_proto().encode_to_vec();
                        if let Err(error) = stream.write_all(&response) {
                            eprintln!("diagnostics response write failed: {error}");
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(error) => {
                        drop(listener);
                        let _ = fs::remove_file(&socket_path);
                        return Err(error.to_string());
                    }
                }
            }
            drop(listener);
            if socket_path.exists() {
                fs::remove_file(socket_path).map_err(|error| error.to_string())?;
            }
            Ok(())
        })
}

#[cfg(unix)]
/// Reads one typed status snapshot from the diagnostics socket.
///
/// # Errors
///
/// Returns an error for socket, protocol, timeout, or protobuf failures.
pub fn read_diagnostics(socket_path: &Path) -> Result<DiagnosticsSnapshot, String> {
    use std::{io::Read, io::Write, os::unix::net::UnixStream};

    let mut stream = UnixStream::connect(socket_path).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(IO_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(IO_TIMEOUT)))
        .map_err(|error| error.to_string())?;
    stream
        .write_all(b"status\n")
        .map_err(|error| error.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|error| error.to_string())?;
    let snapshot = proto::DiagnosticsSnapshot::decode(response.as_slice())
        .map_err(|error| error.to_string())?;
    DiagnosticsSnapshot::from_proto(snapshot)
}
