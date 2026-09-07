use std::{
    fmt::Write as _,
    fs::{self, File},
    io::{Cursor, Read, Write},
    path::Path,
};

use sha2::{Digest, Sha256};

use super::{SyncError, sync_io};

pub(crate) fn digest_file(path: &Path) -> Result<String, SyncError> {
    let mut file = File::open(path).map_err(sync_io)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let read = file.read(&mut buffer).map_err(sync_io)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let mut output = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(output, "{byte:02x}").expect("String write");
    }
    Ok(output)
}

pub(crate) fn read_zip_entry(
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

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), SyncError> {
    let temporary = path.with_extension("staging");
    let mut file = File::create(&temporary).map_err(sync_io)?;
    file.write_all(bytes).map_err(sync_io)?;
    file.sync_all().map_err(sync_io)?;
    drop(file);
    fs::rename(temporary, path).map_err(sync_io)
}
