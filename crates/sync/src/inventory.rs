use std::{
    fmt::Write as _,
    fs,
    path::{Component, Path, PathBuf},
};

use celerity_proto::celerity::v1::{Completion, RunManifest};
use prost::Message;
use sha2::{Digest, Sha256};

use super::{SyncError, sync_io};

#[derive(Clone, Debug)]
pub(crate) struct SpoolRun {
    pub(crate) digest: String,
    pub(crate) directory: PathBuf,
    pub(crate) manifest: Vec<u8>,
    pub(crate) chunks: Vec<SpoolChunk>,
}

#[derive(Clone, Debug)]
pub(crate) struct SpoolChunk {
    pub(crate) path: PathBuf,
    pub(crate) digest: String,
}

pub(crate) fn inventory(root: &Path) -> Result<Vec<SpoolRun>, SyncError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut runs = Vec::new();
    for entry in fs::read_dir(root).map_err(sync_io)? {
        let directory = entry.map_err(sync_io)?.path();
        let manifest_path = directory.join("manifest.pb");
        if !directory.is_dir() || !manifest_path.exists() {
            continue;
        }
        let manifest = fs::read(&manifest_path).map_err(sync_io)?;
        let value = RunManifest::decode(manifest.as_slice())
            .map_err(|error| SyncError(error.to_string()))?;
        if value.completion != Completion::Complete as i32 {
            continue;
        }
        let chunks = if value.chunks.is_empty() {
            let chunk_file = &value.chunk_file;
            let chunk_digest = &value.chunk_sha256;
            if chunk_file.is_empty() {
                return Err(SyncError("complete Run has no chunk_file".to_owned()));
            }
            if !is_digest(chunk_digest) {
                return Err(SyncError(
                    "complete Run has an invalid chunk digest".to_owned(),
                ));
            }
            let relative_path = Path::new(chunk_file);
            if relative_path.is_absolute()
                || relative_path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
            {
                return Err(SyncError("Run chunk path escapes its directory".to_owned()));
            }
            vec![SpoolChunk {
                path: directory.join(relative_path),
                digest: chunk_digest.clone(),
            }]
        } else {
            value
                .chunks
                .iter()
                .map(|chunk| {
                    let file = &chunk.file;
                    let digest = &chunk.sha256;
                    if !is_digest(digest) {
                        return Err(SyncError("Run chunk has an invalid digest".to_owned()));
                    }
                    let relative_path = Path::new(file);
                    if relative_path.is_absolute()
                        || relative_path
                            .components()
                            .any(|component| matches!(component, Component::ParentDir))
                    {
                        return Err(SyncError("Run chunk path escapes its directory".to_owned()));
                    }
                    Ok(SpoolChunk {
                        path: directory.join(relative_path),
                        digest: digest.clone(),
                    })
                })
                .collect::<Result<Vec<_>, SyncError>>()?
        };
        if chunks.is_empty() || chunks.iter().any(|chunk| !chunk.path.is_file()) {
            return Err(SyncError("complete Run has missing chunks".to_owned()));
        }
        runs.push(SpoolRun {
            digest: digest(&manifest),
            directory: directory.clone(),
            manifest,
            chunks,
        });
    }
    runs.sort_by(|left, right| left.digest.cmp(&right.digest));
    Ok(runs)
}

pub(crate) fn incomplete_inventory(root: &Path) -> Result<Vec<PathBuf>, SyncError> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut directories = Vec::new();
    for entry in fs::read_dir(root).map_err(sync_io)? {
        let directory = entry.map_err(sync_io)?.path();
        if !directory.is_dir() {
            continue;
        }
        let manifest_path = directory.join("manifest.pb");
        if !manifest_path.exists() {
            continue;
        }
        let manifest = fs::read(&manifest_path).map_err(sync_io)?;
        let value = RunManifest::decode(manifest.as_slice())
            .map_err(|error| SyncError(error.to_string()))?;
        if value.completion != Completion::Complete as i32 {
            directories.push(directory);
        }
    }
    Ok(directories)
}

pub(crate) fn digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(output, "{byte:02x}").expect("String write");
    }
    output
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
