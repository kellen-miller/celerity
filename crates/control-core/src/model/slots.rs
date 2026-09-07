use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::TractModel;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelSlot {
    A,
    B,
}

impl ModelSlot {
    const fn directory(self) -> &'static str {
        match self {
            Self::A => "slot-a",
            Self::B => "slot-b",
        }
    }

    const fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SlotManifest {
    schema_version: u32,
    onnx_sha256: String,
    compatibility: SlotCompatibility,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SlotCompatibility {
    model_abi: String,
    input_shape: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlotError {
    Io(String),
    DigestMismatch,
    Manifest(String),
    NoValidSlot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedModel {
    pub slot: ModelSlot,
    pub digest: String,
    pub onnx_path: PathBuf,
}

pub struct ModelSlots {
    root: PathBuf,
}

impl ModelSlots {
    /// Opens exactly two startup-only model slots under one owned root.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the root or fixed slot directories cannot be
    /// created.
    pub fn open(root: &Path) -> Result<Self, SlotError> {
        fs::create_dir_all(root).map_err(slot_io)?;
        for slot in [ModelSlot::A, ModelSlot::B] {
            fs::create_dir_all(root.join(slot.directory())).map_err(slot_io)?;
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Stages bytes only into the inactive slot, verifies the exact digest,
    /// fsyncs both files, and atomically publishes the manifest last.
    ///
    /// # Errors
    ///
    /// Returns a digest or filesystem error without modifying the active slot.
    pub fn stage_inactive(
        &self,
        active: ModelSlot,
        onnx_bytes: &[u8],
        expected_digest: &str,
        model_abi: &str,
    ) -> Result<ModelSlot, SlotError> {
        let actual = digest_bytes(onnx_bytes);
        if actual != expected_digest {
            return Err(SlotError::DigestMismatch);
        }
        let inactive = active.other();
        let directory = self.root.join(inactive.directory());
        let staging = directory.join("model.onnx.staging");
        let final_model = directory.join("model.onnx");
        let mut model_file = File::create(&staging).map_err(slot_io)?;
        model_file.write_all(onnx_bytes).map_err(slot_io)?;
        model_file.sync_all().map_err(slot_io)?;
        drop(model_file);
        fs::rename(staging, final_model).map_err(slot_io)?;

        let manifest = SlotManifest {
            schema_version: 1,
            onnx_sha256: actual,
            compatibility: SlotCompatibility {
                model_abi: model_abi.to_owned(),
                input_shape: Vec::new(),
            },
        };
        let serialized = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| SlotError::Manifest(error.to_string()))?;
        let temporary_manifest = directory.join("manifest.json.staging");
        let mut manifest_file = File::create(&temporary_manifest).map_err(slot_io)?;
        manifest_file.write_all(&serialized).map_err(slot_io)?;
        manifest_file.sync_all().map_err(slot_io)?;
        drop(manifest_file);
        fs::rename(temporary_manifest, directory.join("manifest.json")).map_err(slot_io)?;
        let mut bundle_digest = File::create(directory.join("bundle-digest")).map_err(slot_io)?;
        bundle_digest
            .write_all(format!("{expected_digest}\n").as_bytes())
            .map_err(slot_io)?;
        bundle_digest.sync_all().map_err(slot_io)?;
        Ok(inactive)
    }

    /// Selects and validates at most one graph before authority begins. The
    /// returned value has no staging or swap operation.
    ///
    /// # Errors
    ///
    /// Returns `NoValidSlot` when neither desired nor known-good contains a
    /// digest-valid compatible model.
    pub fn select_startup(
        &self,
        desired_digest: Option<&str>,
        known_good_digest: Option<&str>,
        expected_abi: &str,
        input_length: usize,
    ) -> Result<Option<SelectedModel>, SlotError> {
        if desired_digest.is_none() && known_good_digest.is_none() {
            return Ok(None);
        }
        for wanted in [desired_digest, known_good_digest].into_iter().flatten() {
            for slot in [ModelSlot::A, ModelSlot::B] {
                if let Some(selected) = self.valid_slot(slot, wanted, expected_abi, input_length)? {
                    return Ok(Some(selected));
                }
            }
        }
        Err(SlotError::NoValidSlot)
    }

    /// Reads the atomically published desired and known-good digests and
    /// selects one compatible slot exactly once at process startup.
    ///
    /// # Errors
    ///
    /// Returns a filesystem, manifest, digest, or compatibility failure.
    pub fn select_configured_startup(
        &self,
        expected_abi: &str,
        input_length: usize,
    ) -> Result<Option<SelectedModel>, SlotError> {
        let read_digest = |name: &str| -> Result<Option<String>, SlotError> {
            let path = self.root.join(name);
            if !path.exists() {
                return Ok(None);
            }
            let digest = fs::read_to_string(path).map_err(slot_io)?.trim().to_owned();
            if digest.is_empty() {
                return Err(SlotError::Manifest(format!("{name} is empty")));
            }
            Ok(Some(digest))
        };
        let desired = read_digest("desired-digest")?;
        let known_good = read_digest("known-good-digest")?;
        let rejected = fs::read_to_string(self.root.join("rejected-digests"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>();
        if let Some(desired) = desired
            .as_deref()
            .filter(|digest| !rejected.contains(*digest))
        {
            match self.select_startup(Some(desired), None, expected_abi, input_length) {
                Ok(Some(selected)) => return Ok(Some(selected)),
                Ok(None) => {}
                Err(SlotError::NoValidSlot) => self.mark_rejected(desired)?,
                Err(error) => return Err(error),
            }
        }
        let Some(known_good) = known_good
            .as_deref()
            .filter(|digest| !rejected.contains(*digest))
        else {
            return Ok(None);
        };
        match self.select_startup(None, Some(known_good), expected_abi, input_length) {
            Ok(selected) => Ok(selected),
            Err(SlotError::NoValidSlot) => {
                self.mark_rejected(known_good)?;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Selects only the published known-good bundle for startup recovery.
    ///
    /// # Errors
    ///
    /// Returns an error when the marker or selected slot is unreadable,
    /// corrupt, or incompatible with the configured model contract.
    pub fn select_known_good_startup(
        &self,
        expected_abi: &str,
        input_length: usize,
    ) -> Result<Option<SelectedModel>, SlotError> {
        let path = self.root.join("known-good-digest");
        if !path.exists() {
            return Ok(None);
        }
        let digest = fs::read_to_string(path).map_err(slot_io)?;
        let digest = digest.trim();
        let rejected = fs::read_to_string(self.root.join("rejected-digests"))
            .unwrap_or_default()
            .lines()
            .any(|rejected| rejected == digest);
        if rejected {
            return Ok(None);
        }
        match self.select_startup(None, Some(digest), expected_abi, input_length) {
            Ok(selected) => Ok(selected),
            Err(SlotError::NoValidSlot) => {
                self.mark_rejected(digest)?;
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Persists only a structurally or inferentially invalid model digest.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the rejected-digest set cannot be published.
    pub fn mark_rejected(&self, digest: &str) -> Result<(), SlotError> {
        let path = self.root.join("rejected-digests");
        let mut rejected = fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>();
        if !rejected.insert(digest.to_owned()) {
            return Ok(());
        }
        let temporary = self.root.join("rejected-digests.staging");
        let mut file = File::create(&temporary).map_err(slot_io)?;
        for rejected_digest in rejected {
            writeln!(file, "{rejected_digest}").map_err(slot_io)?;
        }
        file.sync_all().map_err(slot_io)?;
        drop(file);
        fs::rename(temporary, path).map_err(slot_io)
    }

    /// Records the startup-selected digest as active without changing the
    /// graph in the running process. Acceptance promotes this digest to
    /// known-good only after a clean completed Run.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the marker cannot be atomically published.
    pub fn mark_active(&self, selected: &SelectedModel) -> Result<(), SlotError> {
        let temporary = self.root.join("active-digest.staging");
        let mut file = File::create(&temporary).map_err(slot_io)?;
        file.write_all(selected.digest.as_bytes())
            .map_err(slot_io)?;
        file.write_all(b"\n").map_err(slot_io)?;
        file.sync_all().map_err(slot_io)?;
        drop(file);
        fs::rename(temporary, self.root.join("active-digest")).map_err(slot_io)
    }

    /// Clears stale active-model evidence when startup selects no model.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if an existing marker cannot be removed.
    pub fn clear_active(&self) -> Result<(), SlotError> {
        let path = self.root.join("active-digest");
        if path.exists() {
            fs::remove_file(path).map_err(slot_io)?;
        }
        Ok(())
    }

    /// Promotes the active digest only after its runtime produced acceptance
    /// evidence in a clean completed Run.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected model is not active or the marker
    /// cannot be atomically published.
    pub fn promote_known_good(&self, selected: &SelectedModel) -> Result<(), SlotError> {
        let active = fs::read_to_string(self.root.join("active-digest")).map_err(slot_io)?;
        if active.trim() != selected.digest {
            return Err(SlotError::Manifest(
                "known-good promotion requires the active digest".to_owned(),
            ));
        }
        let temporary = self.root.join("known-good-digest.staging");
        let mut file = File::create(&temporary).map_err(slot_io)?;
        file.write_all(selected.digest.as_bytes())
            .map_err(slot_io)?;
        file.write_all(b"\n").map_err(slot_io)?;
        file.sync_all().map_err(slot_io)?;
        drop(file);
        fs::rename(temporary, self.root.join("known-good-digest")).map_err(slot_io)
    }

    fn valid_slot(
        &self,
        slot: ModelSlot,
        wanted_digest: &str,
        expected_abi: &str,
        input_length: usize,
    ) -> Result<Option<SelectedModel>, SlotError> {
        let directory = self.root.join(slot.directory());
        let manifest_path = directory.join("manifest.json");
        let onnx_path = directory.join("model.onnx");
        let bundle_digest_path = directory.join("bundle-digest");
        if !manifest_path.exists() || !onnx_path.exists() || !bundle_digest_path.exists() {
            return Ok(None);
        }
        let manifest: SlotManifest =
            match serde_json::from_slice(&fs::read(manifest_path).map_err(slot_io)?) {
                Ok(manifest) => manifest,
                Err(_) => return Ok(None),
            };
        let bundle_digest = fs::read_to_string(bundle_digest_path).map_err(slot_io)?;
        if manifest.schema_version != 1
            || bundle_digest.trim() != wanted_digest
            || manifest.compatibility.model_abi != expected_abi
            || (!manifest.compatibility.input_shape.is_empty()
                && manifest.compatibility.input_shape != [1, input_length])
        {
            return Ok(None);
        }
        let bytes = fs::read(&onnx_path).map_err(slot_io)?;
        if digest_bytes(&bytes) != manifest.onnx_sha256 {
            return Ok(None);
        }
        if TractModel::load(&onnx_path, input_length, 1).is_err() {
            return Ok(None);
        }
        Ok(Some(SelectedModel {
            slot,
            digest: wanted_digest.to_owned(),
            onnx_path,
        }))
    }
}

fn digest_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("String write");
    }
    output
}

fn slot_io(error: std::io::Error) -> SlotError {
    SlotError::Io(error.to_string())
}
