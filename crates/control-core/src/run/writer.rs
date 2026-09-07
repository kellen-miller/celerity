use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU8, AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

use prost::Message;

use super::storage::{digest_file, io_error, write_manifest};
use super::{
    Completion, EnqueueResult, RunChunk, RunContext, RunError, RunManifest, RunRecord, wire,
};

enum WorkerMessage {
    Record(RunRecord),
    FirmwareGeneration(u32),
    Seal,
}

pub struct RunWriter {
    sender: Option<mpsc::SyncSender<WorkerMessage>>,
    degraded_reason: Arc<AtomicU8>,
    dropped_record_count: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<Result<RunManifest, RunError>>>,
}

const DEGRADE_NONE: u8 = 0;
const DEGRADE_MINIMUM_FREE_SPACE: u8 = 1;
const DEGRADE_QUEUE_FAILURE: u8 = 2;
const CHUNK_ROTATION_BYTES: usize = 8 * 1024 * 1024;

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
        let degraded_reason = Arc::new(AtomicU8::new(DEGRADE_NONE));
        if fs4::available_space(root).map_err(io_error)? < minimum_free_bytes {
            degraded_reason.store(DEGRADE_MINIMUM_FREE_SPACE, Ordering::Release);
        }

        let (sender, receiver) = mpsc::sync_channel(queue_capacity);
        let worker_degraded = Arc::clone(&degraded_reason);
        let dropped_record_count = Arc::new(AtomicU64::new(0));
        let worker_dropped_record_count = Arc::clone(&dropped_record_count);
        let owned_run_id = run_id.to_owned();
        let worker = thread::Builder::new()
            .name("celerity-run-writer".to_owned())
            .spawn(move || {
                write_run(
                    directory,
                    owned_run_id,
                    context,
                    receiver,
                    worker_degraded,
                    worker_dropped_record_count,
                )
            })
            .map_err(io_error)?;
        Ok(Self {
            sender: Some(sender),
            degraded_reason,
            dropped_record_count,
            worker: Some(worker),
        })
    }

    #[must_use]
    pub fn record_firmware_generation(&self, generation: u32) -> EnqueueResult {
        if self.degraded_reason.load(Ordering::Acquire) != DEGRADE_NONE {
            return EnqueueResult::Degraded;
        }
        let Some(sender) = &self.sender else {
            self.mark_degraded(DEGRADE_QUEUE_FAILURE);
            return EnqueueResult::Degraded;
        };
        if sender
            .try_send(WorkerMessage::FirmwareGeneration(generation))
            .is_ok()
        {
            EnqueueResult::Accepted
        } else {
            self.mark_degraded(DEGRADE_QUEUE_FAILURE);
            EnqueueResult::Degraded
        }
    }

    #[must_use]
    pub fn enqueue(&self, record: RunRecord) -> EnqueueResult {
        if self.degraded_reason.load(Ordering::Acquire) != DEGRADE_NONE {
            return EnqueueResult::Degraded;
        }
        let Some(sender) = &self.sender else {
            self.mark_degraded(DEGRADE_QUEUE_FAILURE);
            return EnqueueResult::Degraded;
        };
        if sender.try_send(WorkerMessage::Record(record)).is_ok() {
            EnqueueResult::Accepted
        } else {
            self.mark_degraded(DEGRADE_QUEUE_FAILURE);
            EnqueueResult::Degraded
        }
    }

    #[must_use]
    pub fn enqueue_bulk(&self, record: RunRecord) -> EnqueueResult {
        if self.degraded_reason.load(Ordering::Acquire) != DEGRADE_NONE {
            return EnqueueResult::Degraded;
        }
        let Some(sender) = &self.sender else {
            self.mark_degraded(DEGRADE_QUEUE_FAILURE);
            return EnqueueResult::Degraded;
        };
        if sender.try_send(WorkerMessage::Record(record)).is_ok() {
            EnqueueResult::Accepted
        } else {
            self.dropped_record_count.fetch_add(1, Ordering::Relaxed);
            EnqueueResult::Dropped
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

    fn mark_degraded(&self, reason: u8) {
        let _ = self.degraded_reason.compare_exchange(
            DEGRADE_NONE,
            reason,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

fn write_run(
    directory: PathBuf,
    run_id: String,
    mut context: RunContext,
    receiver: mpsc::Receiver<WorkerMessage>,
    degraded_reason: Arc<AtomicU8>,
    dropped_record_count: Arc<AtomicU64>,
) -> Result<RunManifest, RunError> {
    let mut chunk_index = 0;
    let mut partial_paths = vec![partial_chunk_path(&directory, chunk_index)];
    let mut chunk = BufWriter::new(File::create(&partial_paths[0]).map_err(io_error)?);
    let mut chunk_bytes = 0_usize;
    let mut first_sequence = None;
    let mut last_sequence = None;
    let mut local_reason = None;
    let mut sealed = false;

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
                if chunk_bytes > 0
                    && chunk_bytes.saturating_add(encoded.len()) > CHUNK_ROTATION_BYTES
                {
                    flush_chunk(&mut chunk)?;
                    chunk_index += 1;
                    let path = partial_chunk_path(&directory, chunk_index);
                    partial_paths.push(path.clone());
                    chunk = BufWriter::new(File::create(path).map_err(io_error)?);
                    chunk_bytes = 0;
                }
                chunk.write_all(&encoded).map_err(io_error)?;
                chunk_bytes = chunk_bytes.saturating_add(encoded.len());
            }
            WorkerMessage::FirmwareGeneration(generation) => {
                context.firmware_generation = generation;
            }
            WorkerMessage::Seal => {
                sealed = true;
                break;
            }
        }
    }
    if !sealed {
        local_reason = Some("writer_channel_closed");
    }
    flush_chunk(&mut chunk)?;
    drop(chunk);

    let reason = degraded_reason_text(degraded_reason.load(Ordering::Acquire)).or(local_reason);
    let completion = if reason.is_some() {
        Completion::Incomplete
    } else {
        Completion::Complete
    };
    let chunk_paths = if completion == Completion::Complete {
        partial_paths
            .iter()
            .enumerate()
            .map(|(index, partial)| {
                let final_path = final_chunk_path(&directory, index);
                fs::rename(partial, &final_path).map_err(io_error)?;
                Ok(final_path)
            })
            .collect::<Result<Vec<_>, RunError>>()?
    } else {
        partial_paths
    };
    let chunks = chunk_paths
        .iter()
        .map(|path| {
            Ok(RunChunk {
                file: path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or("events.chunk.partial")
                    .to_owned(),
                sha256: digest_file(path)?,
            })
        })
        .collect::<Result<Vec<_>, RunError>>()?;
    let first_chunk = chunks
        .first()
        .ok_or_else(|| RunError("Run has no evidence chunks".to_owned()))?;
    let manifest = RunManifest {
        schema_version: 1,
        run_id,
        completion,
        incomplete_reason: reason.map(str::to_owned),
        first_sequence,
        last_sequence,
        chunk_file: first_chunk.file.clone(),
        chunk_sha256: first_chunk.sha256.clone(),
        chunks,
        dropped_record_count: dropped_record_count.load(Ordering::Acquire),
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

fn partial_chunk_path(directory: &Path, index: usize) -> PathBuf {
    if index == 0 {
        directory.join("events.chunk.partial")
    } else {
        directory.join(format!("events-{index:03}.chunk.partial"))
    }
}

fn final_chunk_path(directory: &Path, index: usize) -> PathBuf {
    if index == 0 {
        directory.join("events.chunk")
    } else {
        directory.join(format!("events-{index:03}.chunk"))
    }
}

fn flush_chunk(chunk: &mut BufWriter<File>) -> Result<(), RunError> {
    chunk.flush().map_err(io_error)?;
    chunk.get_ref().sync_data().map_err(io_error)
}

const fn degraded_reason_text(code: u8) -> Option<&'static str> {
    match code {
        DEGRADE_MINIMUM_FREE_SPACE => Some("minimum_free_space"),
        DEGRADE_QUEUE_FAILURE => Some("writer_queue_failure"),
        _ => None,
    }
}
