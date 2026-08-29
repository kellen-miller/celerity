use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    thread,
};

use prost::Message;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::RuntimeEvent;

#[allow(clippy::doc_markdown, clippy::trivially_copy_pass_by_ref)]
mod wire {
    include!(concat!(env!("OUT_DIR"), "/celerity.run.v1.rs"));
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Completion {
    Complete,
    Incomplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnqueueResult {
    Accepted,
    Degraded,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawCanEvidence {
    pub monotonic_ns: u64,
    pub source: &'static str,
    pub channel: &'static str,
    pub direction: i32,
    pub id: u32,
    pub fd: bool,
    pub bit_rate_switch: bool,
    pub data: Vec<u8>,
    pub hardware_timestamp_ns: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunRecord {
    sequence: u64,
    monotonic_ns: u64,
    source: String,
    payload: wire::run_event::Payload,
}

impl RunRecord {
    #[must_use]
    pub fn lifecycle(sequence: u64, monotonic_ns: u64, payload: &str) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: "runtime".to_owned(),
            payload: wire::run_event::Payload::LifecycleEvent(wire::LifecycleEvent {
                encoded: payload.to_owned(),
            }),
        }
    }

    #[must_use]
    pub fn evidence(sequence: u64, monotonic_ns: u64, source: &str, payload: &str) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: source.to_owned(),
            payload: wire::run_event::Payload::Annotation(wire::Annotation {
                encoded: payload.to_owned(),
            }),
        }
    }

    /// Encodes one ordered production runtime event into the canonical Run
    /// stream without changing its semantic fields.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the versioned event cannot be encoded.
    pub fn runtime_event(sequence: u64, event: &RuntimeEvent) -> Result<Self, serde_json::Error> {
        Ok(Self {
            sequence,
            monotonic_ns: event.monotonic_ms_for_evidence().saturating_mul(1_000_000),
            source: "runtime_event".to_owned(),
            payload: wire::run_event::Payload::LifecycleEvent(wire::LifecycleEvent {
                encoded: serde_json::to_string(event)?,
            }),
        })
    }

    #[must_use]
    pub fn raw_can(sequence: u64, evidence: RawCanEvidence) -> Self {
        Self {
            sequence,
            monotonic_ns: evidence.monotonic_ns,
            source: evidence.source.to_owned(),
            payload: wire::run_event::Payload::RawCan(wire::RawCanFrame {
                channel: evidence.channel.to_owned(),
                direction: evidence.direction,
                id: evidence.id,
                extended: evidence.id > 0x7ff,
                fd: evidence.fd,
                bit_rate_switch: evidence.bit_rate_switch,
                error_state_indicator: false,
                data: evidence.data,
                hardware_timestamp_ns: evidence.hardware_timestamp_ns,
                receive_overflow_count: 0,
                dropped_count: 0,
            }),
        }
    }

    #[must_use]
    pub fn signal(
        sequence: u64,
        monotonic_ns: u64,
        signal: &str,
        value: f64,
        decoder_generation: u64,
        age_ns: u64,
        source_event_sequence: u64,
    ) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: "model_signal".to_owned(),
            payload: wire::run_event::Payload::SignalObservation(wire::SignalObservation {
                signal: signal.to_owned(),
                value,
                unit: "native".to_owned(),
                reference: "vehicle".to_owned(),
                quality: wire::Quality::Valid as i32,
                reason: String::new(),
                source_event_sequence,
                decoder_generation,
                age_ns,
            }),
        }
    }

    #[must_use]
    pub fn control(sequence: u64, monotonic_ns: u64, payload: &str) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: "control".to_owned(),
            payload: wire::run_event::Payload::ControlDecision(wire::ControlDecision {
                encoded: payload.to_owned(),
            }),
        }
    }

    #[must_use]
    pub fn experiment(sequence: u64, monotonic_ns: u64, payload: &str) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: "experiment".to_owned(),
            payload: wire::run_event::Payload::ExperimentEvent(wire::ExperimentEvent {
                encoded: payload.to_owned(),
            }),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunManifest {
    pub schema_version: u32,
    pub run_id: String,
    pub completion: Completion,
    pub incomplete_reason: Option<String>,
    pub first_sequence: Option<u64>,
    pub last_sequence: Option<u64>,
    pub chunk_file: String,
    pub chunk_sha256: String,
    pub configuration_generation: u64,
    pub configuration_sha256: String,
    pub model_bundle_digest: Option<String>,
    pub protocol_major: u8,
    pub firmware_generation: u32,
    pub decoder_generation: u64,
    pub model_abi: String,
    pub model_input_signals: Vec<String>,
    pub model_history_length: usize,
    pub sample_period_ms: u64,
    pub command_lattice: Vec<u16>,
    pub maximum_calibration_error: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunContext {
    pub configuration_generation: u64,
    pub configuration_sha256: String,
    pub model_bundle_digest: Option<String>,
    pub protocol_major: u8,
    pub firmware_generation: u32,
    pub decoder_generation: u64,
    pub model_abi: String,
    pub model_input_signals: Vec<String>,
    pub model_history_length: usize,
    pub sample_period_ms: u64,
    pub command_lattice: Vec<u16>,
    pub maximum_calibration_error: f64,
}

#[derive(Debug)]
pub struct RunError(String);

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RunError {}

enum WorkerMessage {
    Record(RunRecord),
    FirmwareGeneration(u32),
    Seal,
}

pub struct RunWriter {
    sender: Option<mpsc::SyncSender<WorkerMessage>>,
    degraded_reason: Arc<Mutex<Option<&'static str>>>,
    worker: Option<thread::JoinHandle<Result<RunManifest, RunError>>>,
}

impl RunWriter {
    /// Starts the only Run writer worker and opens an append-only partial
    /// chunk under `root/run_id`.
    ///
    /// # Errors
    ///
    /// Returns an error when the Run directory or worker cannot be created.
    pub fn start(
        root: &Path,
        run_id: &str,
        queue_capacity: usize,
        minimum_free_bytes: u64,
    ) -> Result<Self, RunError> {
        Self::start_with_context(
            root,
            run_id,
            queue_capacity,
            minimum_free_bytes,
            RunContext::default(),
        )
    }

    /// Starts a writer with the complete derivation provenance persisted in
    /// the final Run manifest.
    ///
    /// # Errors
    ///
    /// Returns an error when storage cannot be inspected or initialized, or
    /// when the bounded writer worker cannot be started.
    pub fn start_with_context(
        root: &Path,
        run_id: &str,
        queue_capacity: usize,
        minimum_free_bytes: u64,
        context: RunContext,
    ) -> Result<Self, RunError> {
        fs::create_dir_all(root).map_err(io_error)?;
        let directory = root.join(run_id);
        fs::create_dir_all(&directory).map_err(io_error)?;
        let degraded_reason = Arc::new(Mutex::new(None));
        if fs4::available_space(root).map_err(io_error)? < minimum_free_bytes {
            *degraded_reason.lock().map_err(lock_error)? = Some("minimum_free_space");
        }

        let (sender, receiver) = mpsc::sync_channel(queue_capacity);
        let worker_degraded = Arc::clone(&degraded_reason);
        let owned_run_id = run_id.to_owned();
        let worker = thread::Builder::new()
            .name("celerity-run-writer".to_owned())
            .spawn(move || write_run(directory, owned_run_id, context, receiver, worker_degraded))
            .map_err(io_error)?;
        Ok(Self {
            sender: Some(sender),
            degraded_reason,
            worker: Some(worker),
        })
    }

    #[must_use]
    pub fn record_firmware_generation(&self, generation: u32) -> EnqueueResult {
        if self
            .degraded_reason
            .lock()
            .map_or(true, |reason| reason.is_some())
        {
            return EnqueueResult::Degraded;
        }
        let Some(sender) = &self.sender else {
            if let Ok(mut reason) = self.degraded_reason.lock() {
                *reason = Some("writer_queue_failure");
            }
            return EnqueueResult::Degraded;
        };
        if sender
            .try_send(WorkerMessage::FirmwareGeneration(generation))
            .is_ok()
        {
            EnqueueResult::Accepted
        } else {
            if let Ok(mut reason) = self.degraded_reason.lock() {
                *reason = Some("writer_queue_failure");
            }
            EnqueueResult::Degraded
        }
    }

    #[must_use]
    pub fn enqueue(&self, record: RunRecord) -> EnqueueResult {
        if self
            .degraded_reason
            .lock()
            .map_or(true, |reason| reason.is_some())
        {
            return EnqueueResult::Degraded;
        }
        let Some(sender) = &self.sender else {
            if let Ok(mut reason) = self.degraded_reason.lock() {
                *reason = Some("writer_queue_failure");
            }
            return EnqueueResult::Degraded;
        };
        if sender.try_send(WorkerMessage::Record(record)).is_ok() {
            EnqueueResult::Accepted
        } else {
            if let Ok(mut reason) = self.degraded_reason.lock() {
                *reason = Some("writer_queue_failure");
            }
            EnqueueResult::Degraded
        }
    }

    /// Flushes and seals the Run. This bounded lifecycle operation is outside
    /// the control executor's nonblocking enqueue path.
    ///
    /// # Errors
    ///
    /// Returns an error for channel, worker, filesystem, or serialization
    /// failure. A known data-quality failure instead seals an incomplete Run.
    pub fn seal(mut self) -> Result<RunManifest, RunError> {
        let sender = self
            .sender
            .take()
            .ok_or_else(|| RunError("writer already sealed".to_owned()))?;
        sender
            .send(WorkerMessage::Seal)
            .map_err(|error| RunError(error.to_string()))?;
        drop(sender);
        self.worker
            .take()
            .ok_or_else(|| RunError("writer worker missing".to_owned()))?
            .join()
            .map_err(|_| RunError("writer worker panicked".to_owned()))?
    }
}

fn write_run(
    directory: PathBuf,
    run_id: String,
    mut context: RunContext,
    receiver: mpsc::Receiver<WorkerMessage>,
    degraded_reason: Arc<Mutex<Option<&'static str>>>,
) -> Result<RunManifest, RunError> {
    let partial_path = directory.join("events.chunk.partial");
    let mut chunk = File::create(&partial_path).map_err(io_error)?;
    let mut first_sequence = None;
    let mut last_sequence = None;
    let mut local_reason = None;

    while let Ok(message) = receiver.recv() {
        match message {
            WorkerMessage::Record(record) => {
                if last_sequence.is_some_and(|last| record.sequence <= last) {
                    local_reason = Some("nonmonotonic_sequence");
                }
                first_sequence.get_or_insert(record.sequence);
                last_sequence = Some(record.sequence);
                let event = wire::RunEvent {
                    schema_major: 1,
                    schema_minor: 0,
                    run_id: run_id.as_bytes().to_vec(),
                    sequence: record.sequence,
                    monotonic_ns: record.monotonic_ns,
                    ingestion_monotonic_ns: record.monotonic_ns,
                    source: record.source,
                    payload: Some(record.payload),
                    source_monotonic_ns: None,
                };
                let mut encoded = Vec::with_capacity(event.encoded_len() + 10);
                event
                    .encode_length_delimited(&mut encoded)
                    .map_err(|error| RunError(error.to_string()))?;
                chunk.write_all(&encoded).map_err(io_error)?;
            }
            WorkerMessage::FirmwareGeneration(generation) => {
                context.firmware_generation = generation;
            }
            WorkerMessage::Seal => break,
        }
    }
    chunk.sync_all().map_err(io_error)?;
    drop(chunk);

    let shared_reason = *degraded_reason.lock().map_err(lock_error)?;
    let reason = shared_reason.or(local_reason);
    let completion = if reason.is_some() {
        Completion::Incomplete
    } else {
        Completion::Complete
    };
    let final_chunk_path = if completion == Completion::Complete {
        let final_path = directory.join("events.chunk");
        fs::rename(&partial_path, &final_path).map_err(io_error)?;
        final_path
    } else {
        partial_path
    };
    let chunk_sha256 = digest_file(&final_chunk_path)?;
    let manifest = RunManifest {
        schema_version: 1,
        run_id,
        completion,
        incomplete_reason: reason.map(str::to_owned),
        first_sequence,
        last_sequence,
        chunk_file: final_chunk_path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("events.chunk.partial")
            .to_owned(),
        chunk_sha256,
        configuration_generation: context.configuration_generation,
        configuration_sha256: context.configuration_sha256,
        model_bundle_digest: context.model_bundle_digest,
        protocol_major: context.protocol_major,
        firmware_generation: context.firmware_generation,
        decoder_generation: context.decoder_generation,
        model_abi: context.model_abi,
        model_input_signals: context.model_input_signals,
        model_history_length: context.model_history_length,
        sample_period_ms: context.sample_period_ms,
        command_lattice: context.command_lattice,
        maximum_calibration_error: context.maximum_calibration_error,
    };
    write_manifest(&directory, &manifest)?;
    Ok(manifest)
}

/// Recovers interrupted partial Runs by writing an explicit incomplete
/// manifest while preserving the exact partial bytes.
///
/// # Errors
///
/// Returns an error when the root cannot be read or a recovery manifest cannot
/// be written.
pub fn recover_incomplete_runs(root: &Path) -> Result<Vec<PathBuf>, RunError> {
    let mut directories = fs::read_dir(root)
        .map_err(io_error)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    directories.sort();
    let mut recovered = Vec::new();
    for directory in directories {
        let partial = directory.join("events.chunk.partial");
        let manifest_path = directory.join("manifest.json");
        if partial.exists() && !manifest_path.exists() {
            let manifest = RunManifest {
                schema_version: 1,
                run_id: directory
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                completion: Completion::Incomplete,
                incomplete_reason: Some("interrupted".to_owned()),
                first_sequence: None,
                last_sequence: None,
                chunk_file: "events.chunk.partial".to_owned(),
                chunk_sha256: digest_file(&partial)?,
                configuration_generation: 0,
                configuration_sha256: String::new(),
                model_bundle_digest: None,
                protocol_major: 0,
                firmware_generation: 0,
                decoder_generation: 0,
                model_abi: String::new(),
                model_input_signals: Vec::new(),
                model_history_length: 0,
                sample_period_ms: 0,
                command_lattice: Vec::new(),
                maximum_calibration_error: 0.0,
            };
            write_manifest(&directory, &manifest)?;
            recovered.push(manifest_path);
        }
    }
    Ok(recovered)
}

/// Reads ordered runtime events from a sealed canonical chunk for verification
/// or exploratory replay.
///
/// # Errors
///
/// Returns an error for missing, malformed, nonmonotonic, or non-event data.
pub fn replay_events(run_directory: &Path) -> Result<Vec<RuntimeEvent>, RunError> {
    let manifest: RunManifest =
        serde_json::from_slice(&fs::read(run_directory.join("manifest.json")).map_err(io_error)?)
            .map_err(|error| RunError(error.to_string()))?;
    if manifest.completion != Completion::Complete {
        return Err(RunError("incomplete Run cannot be replayed".to_owned()));
    }
    let bytes = fs::read(run_directory.join(&manifest.chunk_file)).map_err(io_error)?;
    if hex_bytes(&Sha256::digest(&bytes)) != manifest.chunk_sha256 {
        return Err(RunError("chunk digest mismatch".to_owned()));
    }
    let mut input = bytes.as_slice();
    let mut events = Vec::new();
    let mut last_sequence = None;
    while !input.is_empty() {
        let stored = wire::RunEvent::decode_length_delimited(&mut input)
            .map_err(|error| RunError(error.to_string()))?;
        if last_sequence.is_some_and(|last| stored.sequence <= last) {
            return Err(RunError("nonmonotonic replay sequence".to_owned()));
        }
        last_sequence = Some(stored.sequence);
        if stored.source == "runtime_event" {
            let Some(wire::run_event::Payload::LifecycleEvent(payload)) = stored.payload else {
                return Err(RunError(
                    "runtime event used the wrong Run payload".to_owned(),
                ));
            };
            events.push(
                serde_json::from_str(&payload.encoded)
                    .map_err(|error| RunError(error.to_string()))?,
            );
        }
    }
    Ok(events)
}

fn write_manifest(directory: &Path, manifest: &RunManifest) -> Result<(), RunError> {
    let temporary = directory.join("manifest.json.tmp");
    let final_path = directory.join("manifest.json");
    let mut serialized =
        serde_json::to_vec_pretty(manifest).map_err(|error| RunError(error.to_string()))?;
    serialized.push(b'\n');
    let mut file = File::create(&temporary).map_err(io_error)?;
    file.write_all(&serialized).map_err(io_error)?;
    file.sync_all().map_err(io_error)?;
    drop(file);
    fs::rename(temporary, final_path).map_err(io_error)
}

fn digest_file(path: &Path) -> Result<String, RunError> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex_bytes(&digest.finalize()))
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

fn io_error(error: std::io::Error) -> RunError {
    RunError(error.to_string())
}

fn lock_error<T>(error: std::sync::PoisonError<T>) -> RunError {
    RunError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{RawCanEvidence, RunRecord, wire};

    #[test]
    fn typed_records_preserve_exact_run_v1_payloads() {
        let raw = RunRecord::raw_can(
            1,
            RawCanEvidence {
                monotonic_ns: 10,
                source: "actuator_rx",
                channel: "actuator",
                direction: wire::Direction::Receive as i32,
                id: 0x18ff_0101,
                fd: true,
                bit_rate_switch: true,
                data: vec![1, 2, 3, 4],
                hardware_timestamp_ns: Some(99),
            },
        );
        let wire::run_event::Payload::RawCan(raw) = raw.payload else {
            panic!("raw CAN oneof");
        };
        assert_eq!(raw.id, 0x18ff_0101);
        assert!(raw.extended && raw.fd && raw.bit_rate_switch);
        assert_eq!(raw.data, [1, 2, 3, 4]);
        assert_eq!(raw.hardware_timestamp_ns, Some(99));
        assert_eq!((raw.receive_overflow_count, raw.dropped_count), (0, 0));

        let signal = RunRecord::signal(2, 20, "coolant_temperature_c", 91.5, 7, 3, 1);
        let wire::run_event::Payload::SignalObservation(signal) = signal.payload else {
            panic!("signal oneof");
        };
        assert_eq!(signal.source_event_sequence, 1);
        assert_eq!(signal.decoder_generation, 7);
        assert_eq!(signal.age_ns, 3);

        assert!(matches!(
            RunRecord::control(3, 30, "{}").payload,
            wire::run_event::Payload::ControlDecision(_)
        ));
        assert!(matches!(
            RunRecord::experiment(4, 40, "{}").payload,
            wire::run_event::Payload::ExperimentEvent(_)
        ));
    }
}
