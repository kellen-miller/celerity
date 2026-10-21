use std::{
    io::Read,
    path::{Path, PathBuf},
};

use celerity_proto::celerity::v1::ModelBundleManifest;
use prost::Message;

use crate::{
    HomeResult, digest, is_digest,
    training::{ModelContract, TrainingResult},
};

pub(crate) struct Candidate {
    pub(crate) digest: String,
    pub(crate) path: PathBuf,
}

impl Candidate {
    pub(crate) fn read(
        output: &Path,
        result: &TrainingResult,
        contract: &ModelContract,
        run_digests: &[String],
    ) -> HomeResult<Self> {
        let bundle_digest = result
            .bundle_digest
            .as_deref()
            .filter(|value| is_digest(value))
            .ok_or("candidate bundle digest is invalid")?;
        let path = output.join(format!("{bundle_digest}.zip"));
        let bytes = std::fs::read(&path)?;
        if digest(&bytes) != bundle_digest {
            return Err("candidate bundle digest mismatch".into());
        }

        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        if archive.len() != 2 {
            return Err("candidate bundle must contain manifest.pb and model.onnx".into());
        }

        let mut manifest_bytes = Vec::new();
        archive
            .by_name("manifest.pb")?
            .read_to_end(&mut manifest_bytes)?;
        let manifest = ModelBundleManifest::decode(manifest_bytes.as_slice())?;
        let mut model = Vec::new();
        archive.by_name("model.onnx")?.read_to_end(&mut model)?;
        if model.is_empty() || digest(&model) != manifest.onnx_sha256 {
            return Err("candidate ONNX digest mismatch".into());
        }

        let compatibility = manifest
            .compatibility
            .as_ref()
            .ok_or("candidate compatibility is absent")?;
        if manifest.schema_version != 1
            || manifest.signal_order != contract.input_signals
            || manifest.sample_period_ms != contract.sample_period_ms
            || manifest.history_length != contract.history_length
            || manifest.command_lattice != contract.command_lattice
            || compatibility.model_abi != contract.model_abi
            || compatibility.input_runs != run_digests
            || !is_digest(&compatibility.derivation_digest)
        {
            return Err("candidate model contract is incompatible".into());
        }

        let width = u64::try_from(contract.input_signals.len())?
            .checked_mul(contract.history_length)
            .and_then(|width| width.checked_add(1))
            .ok_or("candidate input shape overflow")?;
        if compatibility.input_shape != [1, width]
            || manifest.horizons != [1]
            || manifest.output_order != ["coolant", "post_intercooler_iat"]
            || manifest.units.len() != contract.input_signals.len()
            || manifest.units.iter().any(|unit| unit != "native")
            || manifest.input_ranges.len() != contract.input_signals.len()
            || manifest.normalization.len() != contract.input_signals.len()
        {
            return Err("candidate model shape or preprocessing is incompatible".into());
        }

        if manifest.input_ranges.iter().any(|range| {
            !range.minimum.is_finite()
                || !range.maximum.is_finite()
                || range.minimum > range.maximum
        }) || manifest.normalization.iter().any(|normalization| {
            !normalization.mean.is_finite()
                || !normalization.scale.is_finite()
                || normalization.scale <= 0.0
        }) {
            return Err("candidate model preprocessing is invalid".into());
        }

        let parity = result
            .maximum_parity_error
            .ok_or("candidate parity is absent")?;
        let mse = result
            .held_out_mse
            .ok_or("candidate held-out MSE is absent")?;
        if compatibility.maximum_parity_error.to_bits() != parity.to_bits()
            || compatibility.held_out_mse.to_bits() != mse.to_bits()
            || !manifest.calibration_error.is_finite()
            || manifest.calibration_error < 0.0
            || (manifest.calibration_error - mse.sqrt()).abs()
                > 1e-9_f64.max(mse.sqrt().abs() * 1e-9)
        {
            return Err("candidate evaluation disagrees with its bundle".into());
        }

        Ok(Self {
            digest: bundle_digest.into(),
            path,
        })
    }
}
