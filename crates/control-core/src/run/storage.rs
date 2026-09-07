use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};

use sha2::{Digest, Sha256};

use super::{RunError, RunManifest};

pub(super) fn write_manifest(directory: &Path, manifest: &RunManifest) -> Result<(), RunError> {
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

pub(super) fn digest_file(path: &Path) -> Result<String, RunError> {
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

pub(super) fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

pub(super) fn io_error(error: std::io::Error) -> RunError {
    RunError(error.to_string())
}
