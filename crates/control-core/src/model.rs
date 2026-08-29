use std::{
    collections::VecDeque,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};

use crate::ValidatedBundle;

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
    InferenceFailure,
}

pub struct RuntimeModel {
    graph: TractModel,
    history_length: usize,
    history: VecDeque<Vec<f64>>,
    calibration_error: f64,
    input_ranges: Vec<(f64, f64)>,
}

#[derive(Deserialize)]
struct RuntimeModelManifest {
    calibration_error: f64,
    input_ranges: Vec<RuntimeInputRange>,
    normalization: Vec<RuntimeNormalization>,
}

#[derive(Deserialize)]
struct RuntimeInputRange {
    minimum: f64,
    maximum: f64,
}

#[derive(Deserialize)]
struct RuntimeNormalization {
    mean: f64,
    scale: f64,
}

impl RuntimeModel {
    /// Loads the exact startup-selected graph against the model policy in the
    /// validated bundle. The returned model cannot be swapped after startup.
    ///
    /// # Errors
    ///
    /// Returns a model error when the graph is incompatible with the explicit
    /// history shape or cannot warm successfully.
    pub fn load(path: &Path, bundle: &ValidatedBundle) -> Result<Self, ModelError> {
        let manifest_path = path
            .parent()
            .ok_or_else(|| ModelError::Load("model has no parent directory".to_owned()))?
            .join("manifest.json");
        let metadata: RuntimeModelManifest = serde_json::from_slice(
            &fs::read(manifest_path).map_err(|error| ModelError::Load(error.to_string()))?,
        )
        .map_err(|error| ModelError::Load(error.to_string()))?;
        let metadata_valid = metadata.calibration_error.is_finite()
            && metadata.calibration_error >= 0.0
            && metadata.input_ranges.len() == bundle.model.input_signals.len()
            && metadata.normalization.len() == bundle.model.input_signals.len()
            && metadata.input_ranges.iter().all(|range| {
                range.minimum.is_finite()
                    && range.maximum.is_finite()
                    && range.minimum <= range.maximum
            })
            && metadata.normalization.iter().all(|value| {
                value.mean.is_finite() && value.scale.is_finite() && value.scale > 0.0
            });
        if !metadata_valid {
            return Err(ModelError::Load(
                "invalid model eligibility metadata".to_owned(),
            ));
        }
        let input_ranges = metadata
            .input_ranges
            .iter()
            .map(|range| (range.minimum, range.maximum))
            .collect();
        Ok(Self {
            graph: TractModel::load(
                path,
                bundle
                    .model
                    .history_length
                    .saturating_mul(bundle.model.input_signals.len())
                    .saturating_add(1),
                bundle.model.warmup_iterations,
            )?,
            history_length: bundle.model.history_length,
            history: VecDeque::with_capacity(bundle.model.history_length),
            calibration_error: metadata.calibration_error,
            input_ranges,
        })
    }

    pub(crate) fn observe(&mut self, values: Vec<f64>) {
        if self.history.len() == self.history_length {
            self.history.pop_front();
        }
        self.history.push_back(values);
    }

    pub(crate) fn clear_history(&mut self) {
        self.history.clear();
    }

    pub(crate) fn select(
        &self,
        bundle: &ValidatedBundle,
        deterministic_split_basis_points: u16,
        current_split_basis_points: u16,
        remaining_cycle_ns: u64,
    ) -> ModelCommandSelection {
        if self.history.len() != self.history_length {
            return ModelCommandSelection::Deterministic {
                split_basis_points: deterministic_split_basis_points,
                reason: ModelIneligibility::IncompleteHistory,
            };
        }
        let current = self
            .history
            .back()
            .expect("complete history has a current sample");
        let coolant_c = current[bundle
            .model
            .input_signals
            .iter()
            .position(|signal| signal == "coolant_temperature_c")
            .expect("validated coolant input")];
        let iat_c = current[bundle
            .model
            .input_signals
            .iter()
            .position(|signal| signal == "air_temperature_c")
            .expect("validated IAT input")];
        let coolant_range =
            bundle.model.coolant_input_maximum_c - bundle.model.coolant_input_minimum_c;
        let iat_range = bundle.model.iat_input_maximum_c - bundle.model.iat_input_minimum_c;
        let outside = |value: f64, minimum: f64, maximum: f64, range: f64| {
            if value < minimum {
                (minimum - value) / range
            } else if value > maximum {
                (value - maximum) / range
            } else {
                0.0
            }
        };
        let configured_ood_score = outside(
            coolant_c,
            bundle.model.coolant_input_minimum_c,
            bundle.model.coolant_input_maximum_c,
            coolant_range,
        )
        .max(outside(
            iat_c,
            bundle.model.iat_input_minimum_c,
            bundle.model.iat_input_maximum_c,
            iat_range,
        ));
        let ood_score = current
            .iter()
            .zip(&self.input_ranges)
            .map(|(value, (minimum, maximum))| {
                outside(
                    *value,
                    *minimum,
                    *maximum,
                    (*maximum - *minimum).max(f64::EPSILON),
                )
            })
            .fold(configured_ood_score, f64::max);
        let budget = InferenceBudget {
            measured_ceiling_ns: self.graph.measured_ceiling_ns().saturating_mul(
                u64::try_from(bundle.model.command_lattice.len()).unwrap_or(u64::MAX),
            ),
            required_post_work_ns: bundle.model.required_post_work_ns,
        };
        if !budget.allows(remaining_cycle_ns) {
            return ModelCommandSelection::Deterministic {
                split_basis_points: deterministic_split_basis_points,
                reason: ModelIneligibility::InsufficientBudget,
            };
        }
        let history = (0..bundle.model.input_signals.len())
            .flat_map(|signal| self.history.iter().map(move |sample| sample[signal] as f32))
            .collect::<Vec<_>>();
        let mut candidates = Vec::with_capacity(bundle.model.command_lattice.len());
        let uncertainty = self.calibration_error;
        for command in &bundle.model.command_lattice {
            let mut input = history.clone();
            input.push(f32::from(*command) / 10_000.0);
            let output = match self.graph.infer(&input) {
                Ok(output) if output.len() >= 2 => output,
                Ok(_) | Err(_) => {
                    return ModelCommandSelection::Deterministic {
                        split_basis_points: deterministic_split_basis_points,
                        reason: ModelIneligibility::InferenceFailure,
                    };
                }
            };
            let coolant_peak_c = f64::from(output[0]) + self.calibration_error;
            candidates.push(CandidatePrediction {
                split_basis_points: *command,
                coolant_peak_c,
                coolant_target_excess_c: (coolant_peak_c - bundle.model.coolant_target_c).max(0.0),
                iat_cost: f64::from(output[1]),
            });
        }
        select_model_or_deterministic(
            ModelEligibilityInput {
                graph_present: true,
                compatible: true,
                history_complete: true,
                uncertainty,
                maximum_uncertainty: bundle.model.maximum_uncertainty,
                ood_score,
                maximum_ood_score: bundle.model.maximum_ood_score,
                remaining_cycle_ns,
            },
            budget,
            &candidates,
            deterministic_split_basis_points,
            current_split_basis_points,
            bundle.model.hard_coolant_ceiling_c,
        )
    }
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
            onnx_sha256: actual.clone(),
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
