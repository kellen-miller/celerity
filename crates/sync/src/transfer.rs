use std::{collections::BTreeSet, fs, io::Cursor};

use super::api::{ModelBundleManifest, ReconcileRequest, ReconcileResponse};
use super::artifact::{atomic_write, digest_file, read_zip_entry};
use super::inventory::{SpoolRun, digest, incomplete_inventory, inventory};
use super::{
    HomeApi, NetworkPresence, SyncConfiguration, SyncError, TransferJournal, sync_io, sync_json,
};

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
        for run in runs
            .iter()
            .filter(|run| missing.contains(&run.digest) && self.journal.retry_allowed(&run.digest))
        {
            let result = Self::upload(home, run);
            self.journal.record_attempt(&run.digest, &result)?;
            if result.is_ok() {
                uploaded += 1;
            }
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
        for chunk in &run.chunks {
            if digest_file(&chunk.path)? != chunk.digest {
                return Err(SyncError(
                    "Run chunk digest changed before upload".to_owned(),
                ));
            }
            home.put_chunk(&run.digest, &chunk.digest, &chunk.path)?;
        }
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
        acknowledged.sort_by_key(|run| {
            fs::metadata(&run.directory)
                .and_then(|metadata| metadata.modified())
                .ok()
        });
        let remove_count = acknowledged
            .len()
            .saturating_sub(self.configuration.retention_count);
        for run in acknowledged.into_iter().take(remove_count) {
            fs::remove_dir_all(&run.directory).map_err(sync_io)?;
        }
        let mut incomplete = incomplete_inventory(&self.configuration.spool_root)?;
        incomplete.sort_by_key(|directory| {
            fs::metadata(directory)
                .and_then(|metadata| metadata.modified())
                .ok()
        });
        let remove_count = incomplete
            .len()
            .saturating_sub(self.configuration.incomplete_retention_count);
        for directory in incomplete.into_iter().take(remove_count) {
            fs::remove_dir_all(directory).map_err(sync_io)?;
        }
        Ok(())
    }
}
