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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunRecord {
    sequence: u64,
    monotonic_ns: u64,
    source: String,
    payload: String,
}

impl RunRecord {
    #[must_use]
    pub fn lifecycle(sequence: u64, monotonic_ns: u64, payload: &str) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: "runtime".to_owned(),
            payload: payload.to_owned(),
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
            source: "runtime".to_owned(),
            payload: serde_json::to_string(event)?,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunManifest {
    pub schema_version: u32,
    pub run_id: String,
    pub completion: Completion,
    pub incomplete_reason: Option<String>,
    pub first_sequence: Option<u64>,
    pub last_sequence: Option<u64>,
    pub chunk_file: String,
    pub chunk_sha256: String,
}

#[derive(Clone, PartialEq, Message)]
struct StoredRunEvent {
    #[prost(uint32, tag = "1")]
    schema_major: u32,
    #[prost(uint32, tag = "2")]
    schema_minor: u32,
    #[prost(bytes = "vec", tag = "3")]
    run_id: Vec<u8>,
    #[prost(uint64, tag = "4")]
    sequence: u64,
    #[prost(uint64, tag = "5")]
    monotonic_ns: u64,
    #[prost(uint64, tag = "7")]
    ingestion_monotonic_ns: u64,
    #[prost(string, tag = "8")]
    source: String,
    #[prost(string, tag = "26")]
    lifecycle_payload: String,
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
            .spawn(move || write_run(directory, owned_run_id, receiver, worker_degraded))
            .map_err(io_error)?;
        Ok(Self {
            sender: Some(sender),
            degraded_reason,
            worker: Some(worker),
        })
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
                let event = StoredRunEvent {
                    schema_major: 1,
                    schema_minor: 0,
                    run_id: run_id.as_bytes().to_vec(),
                    sequence: record.sequence,
                    monotonic_ns: record.monotonic_ns,
                    ingestion_monotonic_ns: record.monotonic_ns,
                    source: record.source,
                    lifecycle_payload: record.payload,
                };
                let mut encoded = Vec::with_capacity(event.encoded_len() + 10);
                event
                    .encode_length_delimited(&mut encoded)
                    .map_err(|error| RunError(error.to_string()))?;
                chunk.write_all(&encoded).map_err(io_error)?;
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
        let stored = StoredRunEvent::decode_length_delimited(&mut input)
            .map_err(|error| RunError(error.to_string()))?;
        if last_sequence.is_some_and(|last| stored.sequence <= last) {
            return Err(RunError("nonmonotonic replay sequence".to_owned()));
        }
        last_sequence = Some(stored.sequence);
        events.push(
            serde_json::from_str(&stored.lifecycle_payload)
                .map_err(|error| RunError(error.to_string()))?,
        );
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
