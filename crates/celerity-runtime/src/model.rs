use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tract_onnx::prelude::{
    Framework, InferenceModelExt, IntoRunnable, Tensor, TypedRunnableModel, tvec,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelError {
    Load(String),
    InputShape,
    NonfiniteInput,
    NonfiniteOutput,
    Inference(String),
}

pub struct TractModel {
    runnable: std::sync::Arc<TypedRunnableModel>,
    input_length: usize,
    measured_ceiling_ns: u64,
}

impl TractModel {
    /// Loads, optimizes, warms, and benchmarks the exact ONNX graph selected at
    /// startup. The returned object exposes no graph swap operation.
    ///
    /// # Errors
    ///
    /// Returns a typed error for missing/corrupt/incompatible graphs, invalid
    /// shape, nonfinite values, or tract execution failures.
    pub fn load(
        path: &Path,
        input_length: usize,
        warmup_iterations: usize,
    ) -> Result<Self, ModelError> {
        if input_length == 0 || warmup_iterations == 0 {
            return Err(ModelError::InputShape);
        }
        let runnable = tract_onnx::onnx()
            .model_for_path(path)
            .and_then(InferenceModelExt::into_optimized)
            .and_then(IntoRunnable::into_runnable)
            .map_err(|error| ModelError::Load(error.to_string()))?;
        let mut model = Self {
            runnable,
            input_length,
            measured_ceiling_ns: 0,
        };
        let input = vec![0.0_f32; input_length];
        for _ in 0..warmup_iterations {
            let started = Instant::now();
            model.infer(&input)?;
            let elapsed = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
            model.measured_ceiling_ns = model.measured_ceiling_ns.max(elapsed.max(1));
        }
        Ok(model)
    }

    /// Runs synchronous non-cancellable inference on one typed finite window.
    ///
    /// # Errors
    ///
    /// Returns a typed error for wrong shape, nonfinite input/output, or tract
    /// execution failure.
    pub fn infer(&self, input: &[f32]) -> Result<Vec<f32>, ModelError> {
        if input.len() != self.input_length {
            return Err(ModelError::InputShape);
        }
        if input.iter().any(|value| !value.is_finite()) {
            return Err(ModelError::NonfiniteInput);
        }
        let tensor = Tensor::from_shape(&[1, self.input_length], input)
            .map_err(|error| ModelError::Inference(error.to_string()))?;
        let outputs = self
            .runnable
            .run(tvec!(tensor.into()))
            .map_err(|error| ModelError::Inference(error.to_string()))?;
        let view = outputs[0]
            .to_plain_array_view::<f32>()
            .map_err(|error| ModelError::Inference(error.to_string()))?;
        let values = view.iter().copied().collect::<Vec<_>>();
        if values.iter().any(|value| !value.is_finite()) {
            return Err(ModelError::NonfiniteOutput);
        }
        Ok(values)
    }

    #[must_use]
    pub const fn measured_ceiling_ns(&self) -> u64 {
        self.measured_ceiling_ns
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InferenceBudget {
    pub measured_ceiling_ns: u64,
    pub required_post_work_ns: u64,
}

impl InferenceBudget {
    #[must_use]
    pub const fn allows(self, remaining_cycle_ns: u64) -> bool {
        remaining_cycle_ns
            >= self
                .measured_ceiling_ns
                .saturating_add(self.required_post_work_ns)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CandidatePrediction {
    pub split_basis_points: u16,
    pub coolant_peak_c: f64,
    pub coolant_target_excess_c: f64,
    pub iat_cost: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OptimizerDecision {
    Selected { split_basis_points: u16 },
    NoFeasibleCandidate,
}

pub struct ThermalOptimizer;

impl ThermalOptimizer {
    #[must_use]
    pub fn select(
        candidates: &[CandidatePrediction],
        current_split_basis_points: u16,
        hard_coolant_ceiling_c: f64,
    ) -> OptimizerDecision {
        let mut selected: Option<CandidatePrediction> = None;
        for candidate in candidates {
            if !candidate.coolant_peak_c.is_finite()
                || !candidate.coolant_target_excess_c.is_finite()
                || !candidate.iat_cost.is_finite()
                || candidate.coolant_peak_c > hard_coolant_ceiling_c
            {
                continue;
            }
            let replace = selected.is_none_or(|prior| {
                compare_candidate(candidate, &prior, current_split_basis_points).is_lt()
            });
            if replace {
                selected = Some(*candidate);
            }
        }
        selected.map_or(OptimizerDecision::NoFeasibleCandidate, |candidate| {
            OptimizerDecision::Selected {
                split_basis_points: candidate.split_basis_points,
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelEligibilityInput {
    pub graph_present: bool,
    pub compatible: bool,
    pub history_complete: bool,
    pub uncertainty: f64,
    pub maximum_uncertainty: f64,
    pub ood_score: f64,
    pub maximum_ood_score: f64,
    pub remaining_cycle_ns: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelIneligibility {
    Absent,
    Incompatible,
    IncompleteHistory,
    NonfiniteEvidence,
    Uncertain,
    OutOfDistribution,
    InsufficientBudget,
    NoFeasibleCandidate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelCommandSelection {
    Optimized {
        split_basis_points: u16,
    },
    Deterministic {
        split_basis_points: u16,
        reason: ModelIneligibility,
    },
}

/// Applies the complete model-eligibility gate and finite-control-set result.
/// Every failure names why deterministic protection owns the command.
#[must_use]
pub fn select_model_or_deterministic(
    input: ModelEligibilityInput,
    budget: InferenceBudget,
    candidates: &[CandidatePrediction],
    deterministic_split_basis_points: u16,
    current_split_basis_points: u16,
    hard_coolant_ceiling_c: f64,
) -> ModelCommandSelection {
    let reason = if !input.graph_present {
        Some(ModelIneligibility::Absent)
    } else if !input.compatible {
        Some(ModelIneligibility::Incompatible)
    } else if !input.history_complete {
        Some(ModelIneligibility::IncompleteHistory)
    } else if !input.uncertainty.is_finite() || !input.ood_score.is_finite() {
        Some(ModelIneligibility::NonfiniteEvidence)
    } else if input.uncertainty > input.maximum_uncertainty {
        Some(ModelIneligibility::Uncertain)
    } else if input.ood_score > input.maximum_ood_score {
        Some(ModelIneligibility::OutOfDistribution)
    } else if !budget.allows(input.remaining_cycle_ns) {
        Some(ModelIneligibility::InsufficientBudget)
    } else {
        None
    };
    if let Some(reason) = reason {
        return ModelCommandSelection::Deterministic {
            split_basis_points: deterministic_split_basis_points,
            reason,
        };
    }
    match ThermalOptimizer::select(
        candidates,
        current_split_basis_points,
        hard_coolant_ceiling_c,
    ) {
        OptimizerDecision::Selected { split_basis_points } => {
            ModelCommandSelection::Optimized { split_basis_points }
        }
        OptimizerDecision::NoFeasibleCandidate => ModelCommandSelection::Deterministic {
            split_basis_points: deterministic_split_basis_points,
            reason: ModelIneligibility::NoFeasibleCandidate,
        },
    }
}

fn compare_candidate(
    candidate: &CandidatePrediction,
    prior: &CandidatePrediction,
    current: u16,
) -> std::cmp::Ordering {
    candidate
        .coolant_target_excess_c
        .total_cmp(&prior.coolant_target_excess_c)
        .then_with(|| candidate.iat_cost.total_cmp(&prior.iat_cost))
        .then_with(|| {
            candidate
                .split_basis_points
                .abs_diff(current)
                .cmp(&prior.split_basis_points.abs_diff(current))
        })
        .then_with(|| candidate.split_basis_points.cmp(&prior.split_basis_points))
}

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
    digest: String,
    model_abi: String,
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
            digest: actual,
            model_abi: model_abi.to_owned(),
        };
        let serialized = serde_json::to_vec_pretty(&manifest)
            .map_err(|error| SlotError::Manifest(error.to_string()))?;
        let temporary_manifest = directory.join("manifest.json.staging");
        let mut manifest_file = File::create(&temporary_manifest).map_err(slot_io)?;
        manifest_file.write_all(&serialized).map_err(slot_io)?;
        manifest_file.sync_all().map_err(slot_io)?;
        drop(manifest_file);
        fs::rename(temporary_manifest, directory.join("manifest.json")).map_err(slot_io)?;
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
    ) -> Result<Option<SelectedModel>, SlotError> {
        if desired_digest.is_none() && known_good_digest.is_none() {
            return Ok(None);
        }
        for wanted in [desired_digest, known_good_digest].into_iter().flatten() {
            for slot in [ModelSlot::A, ModelSlot::B] {
                if let Some(selected) = self.valid_slot(slot, wanted, expected_abi)? {
                    return Ok(Some(selected));
                }
            }
        }
        Err(SlotError::NoValidSlot)
    }

    fn valid_slot(
        &self,
        slot: ModelSlot,
        wanted_digest: &str,
        expected_abi: &str,
    ) -> Result<Option<SelectedModel>, SlotError> {
        let directory = self.root.join(slot.directory());
        let manifest_path = directory.join("manifest.json");
        let onnx_path = directory.join("model.onnx");
        if !manifest_path.exists() || !onnx_path.exists() {
            return Ok(None);
        }
        let manifest: SlotManifest =
            serde_json::from_slice(&fs::read(manifest_path).map_err(slot_io)?)
                .map_err(|error| SlotError::Manifest(error.to_string()))?;
        if manifest.schema_version != 1
            || manifest.digest != wanted_digest
            || manifest.model_abi != expected_abi
        {
            return Ok(None);
        }
        let bytes = fs::read(&onnx_path).map_err(slot_io)?;
        if digest_bytes(&bytes) != manifest.digest {
            return Ok(None);
        }
        TractModel::load(&onnx_path, 2, 1)
            .map_err(|error| SlotError::Manifest(format!("{error:?}")))?;
        Ok(Some(SelectedModel {
            slot,
            digest: manifest.digest,
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
