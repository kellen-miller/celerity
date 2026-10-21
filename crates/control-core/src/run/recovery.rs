use std::{
    fs,
    path::{Path, PathBuf},
};

use prost::Message;
use sha2::{Digest, Sha256};

use crate::RuntimeEvent;

use super::storage::{digest_file, hex_bytes, io_error, write_manifest};
use super::{Completion, RunChunk, RunError, RunManifest, wire};

/// Recovers interrupted partial Runs by writing an explicit incomplete
/// manifest while preserving the exact partial bytes.
///
/// # Errors
///
/// Returns an error when the root cannot be read or a recovery manifest cannot
/// be written.
pub fn recover_incomplete_runs(root: &Path) -> Result<Vec<PathBuf>, RunError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut directories = fs::read_dir(root)
        .map_err(io_error)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    directories.sort();
    let mut recovered = Vec::new();
    for directory in directories {
        let manifest_path = directory.join("manifest.pb");
        let mut partials = fs::read_dir(&directory)
            .map_err(io_error)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .is_some_and(|name| name.ends_with(".chunk.partial"))
            })
            .collect::<Vec<_>>();
        partials.sort();
        if !partials.is_empty() && !manifest_path.exists() {
            let chunks = partials
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
                .ok_or_else(|| RunError("recovered Run has no chunks".to_owned()))?;
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
                chunk_file: first_chunk.file.clone(),
                chunk_sha256: first_chunk.sha256.clone(),
                chunks,
                dropped_record_count: 0,
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
    let manifest = wire::RunManifest::decode(
        fs::read(run_directory.join("manifest.pb"))
            .map_err(io_error)?
            .as_slice(),
    )
    .map_err(|error| RunError(error.to_string()))?;
    if manifest.completion != wire::Completion::Complete as i32 {
        return Err(RunError("incomplete Run cannot be replayed".to_owned()));
    }
    let mut events = Vec::new();
    let mut last_sequence = None;
    let chunks = if manifest.chunks.is_empty() {
        vec![wire::RunChunk {
            file: manifest.chunk_file,
            sha256: manifest.chunk_sha256,
        }]
    } else {
        manifest.chunks
    };
    for chunk in chunks {
        let bytes = fs::read(run_directory.join(&chunk.file)).map_err(io_error)?;
        if hex_bytes(&Sha256::digest(&bytes)) != chunk.sha256 {
            return Err(RunError("chunk digest mismatch".to_owned()));
        }
        let mut input = bytes.as_slice();
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
    }
    Ok(events)
}
