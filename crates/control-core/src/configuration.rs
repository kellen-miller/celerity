mod validation;

use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;
use validation::{breakpoint_index, digest, load_experiment};

use crate::{
    ExperimentPlan, PowertrainDecodeError, PowertrainDecoder, powertrain::CantcuReception,
};

const POWERTRAIN_TEMPERATURE_PERIOD_MS: u64 = 200;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StartupMode {
    Simulation,
    Replay,
    Live,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BundleError {
    Read(String),
    Parse(String),
    UnsupportedSchema(u32),
    RequestedModeMismatch {
        requested: StartupMode,
        configured: StartupMode,
    },
    CompositionModeMismatch,
    InvalidControllerAddress(u8),
    DuplicateControllerAddress(u8),
    DuplicateControllerIdentity(u64),
    MissingDuctController,
    InvalidDuctBounds,
    NonmonotonicPolicy,
    InvalidRuntimeTiming,
    InvalidCantcuConfiguration(PowertrainDecodeError),
    InvalidStorage,
    InvalidSync,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ValidatedBundle {
    #[serde(skip)]
    configuration_sha256: String,
    #[serde(skip)]
    loaded_experiment_plan: Option<ExperimentPlan>,
    #[serde(skip, default = "PowertrainDecoder::disabled")]
    powertrain_decoder: PowertrainDecoder,
    schema_version: u32,
    generation: u64,
    mode: StartupMode,
    runtime: RuntimeConfiguration,
    powertrain: PowertrainConfiguration,
    controllers: BTreeMap<String, ControllerConfiguration>,
    duct: DuctConfiguration,
    pub(crate) model: ModelConfiguration,
    run_storage: RunStorageConfiguration,
    diagnostics: DiagnosticsConfiguration,
    sync: SyncConfiguration,
    #[serde(default)]
    experiment: Option<ExperimentConfiguration>,
    composition: Composition,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct RuntimeConfiguration {
    #[serde(rename = "cycle_ms")]
    cycle: u64,
    #[serde(rename = "input_stale_after_ms")]
    input_stale_after: u64,
    #[serde(rename = "model_signals_stale_after_ms")]
    model_signals_stale_after: u64,
    #[serde(rename = "controller_truth_stale_after_ms")]
    controller_truth_stale_after: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimingModel {
    pub cycle_ms: u64,
    pub input_stale_after_ms: u64,
    pub model_signals_stale_after_ms: u64,
    pub controller_truth_stale_after_ms: u64,
    pub acknowledgement_deadline_ms: u16,
    pub command_lease_ms: u16,
    pub heartbeat_period_ms: u16,
    pub runtime_lease_ms: u16,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PowertrainConfiguration {
    decoder_generation: u64,
    cantcu: CantcuReception,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ControllerConfiguration {
    address: u8,
    identity: u64,
    configuration_generation: u32,
    capability_generation: u32,
    resource_id: u32,
    minimum_basis_points: u16,
    maximum_basis_points: u16,
    maximum_command_rate_hz: u16,
    fallback_basis_points: u16,
    pwm_endpoint_a_us: u16,
    pwm_endpoint_b_us: u16,
    direction: u8,
    runtime_lease_ms: u16,
    command_lease_ms: u16,
    heartbeat_period_ms: u16,
    acknowledgement_deadline_ms: u16,
    normal_slew_basis_points_per_second: u16,
    protection_slew_basis_points_per_second: u16,
    digest_prefix: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct DuctConfiguration {
    radiator_split_minimum: f64,
    radiator_split_maximum: f64,
    coolant_breakpoints_c: [f64; 3],
    iat_breakpoints_c: [f64; 3],
    policy_basis_points: [[u16; 3]; 3],
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelConfiguration {
    pub(crate) required: bool,
    pub(crate) slots_root: String,
    pub(crate) abi: String,
    pub(crate) input_signals: Vec<String>,
    pub(crate) history_length: usize,
    pub(crate) warmup_iterations: usize,
    pub(crate) maximum_uncertainty: f64,
    pub(crate) maximum_ood_score: f64,
    pub(crate) required_post_work_ns: u64,
    pub(crate) hard_coolant_ceiling_c: f64,
    pub(crate) coolant_target_c: f64,
    pub(crate) coolant_input_minimum_c: f64,
    pub(crate) coolant_input_maximum_c: f64,
    pub(crate) iat_input_minimum_c: f64,
    pub(crate) iat_input_maximum_c: f64,
    pub(crate) command_lattice: Vec<u16>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControllerRuntimeConfiguration {
    pub address: u8,
    pub identity: u64,
    pub configuration_generation: u32,
    pub capability_generation: u32,
    pub resource_id: u32,
    pub minimum_basis_points: u16,
    pub maximum_basis_points: u16,
    pub maximum_command_rate_hz: u16,
    pub fallback_basis_points: u16,
    pub pwm_endpoint_a_us: u16,
    pub pwm_endpoint_b_us: u16,
    pub direction: u8,
    pub runtime_lease_ms: u16,
    pub command_lease_ms: u16,
    pub heartbeat_period_ms: u16,
    pub acknowledgement_deadline_ms: u16,
    pub normal_slew_basis_points_per_second: u16,
    pub protection_slew_basis_points_per_second: u16,
    pub digest_prefix: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct RunStorageConfiguration {
    root: String,
    minimum_free_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct DiagnosticsConfiguration {
    socket: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct SyncConfiguration {
    spool_root: String,
    retention_count: usize,
    incomplete_retention_count: usize,
    home_interface: String,
    expected_default_gateway: String,
    home_api_url: String,
    credential_path: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ExperimentConfiguration {
    plan: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Composition {
    Simulation {
        scenario: String,
    },
    Replay {
        run: String,
        behavior: ReplayBehavior,
    },
    Live {
        powertrain_interface: String,
        actuator_interface: String,
        watchdog: bool,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ReplayBehavior {
    Verify,
    Explore,
}

impl ValidatedBundle {
    /// Loads and validates a bundle using its declared startup mode.
    ///
    /// # Errors
    ///
    /// Returns the same typed boundary failures as [`Self::load`].
    pub fn load_configured(path: &Path) -> Result<Self, BundleError> {
        let bundle = Self::read(path)?;
        bundle.validate(bundle.mode)?;
        Ok(bundle)
    }

    /// Loads, normalizes, and validates the only supported startup bundle path.
    ///
    /// # Errors
    ///
    /// Returns a typed error for filesystem, syntax, schema, mode, ownership,
    /// timing, policy, storage, or sync boundary violations.
    pub fn load(path: &Path, requested_mode: StartupMode) -> Result<Self, BundleError> {
        let bundle = Self::read(path)?;
        bundle.validate(requested_mode)?;
        Ok(bundle)
    }

    fn read(path: &Path) -> Result<Self, BundleError> {
        let source =
            fs::read_to_string(path).map_err(|error| BundleError::Read(error.to_string()))?;
        let mut bundle: Self =
            toml::from_str(&source).map_err(|error| BundleError::Parse(error.to_string()))?;
        bundle.configuration_sha256 = digest(&source);
        bundle.loaded_experiment_plan = load_experiment(&bundle, path)?;
        bundle.powertrain_decoder = PowertrainDecoder::from_reception(bundle.powertrain.cantcu)
            .map_err(BundleError::InvalidCantcuConfiguration)?;
        Ok(bundle)
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub fn configuration_sha256(&self) -> &str {
        &self.configuration_sha256
    }

    #[must_use]
    pub const fn decoder_generation(&self) -> u64 {
        self.powertrain.decoder_generation
    }

    #[must_use]
    pub const fn powertrain_decoder(&self) -> PowertrainDecoder {
        self.powertrain_decoder
    }

    #[must_use]
    pub const fn model_history_length(&self) -> usize {
        self.model.history_length
    }

    #[must_use]
    pub fn model_command_lattice(&self) -> &[u16] {
        &self.model.command_lattice
    }

    #[must_use]
    pub const fn model_maximum_calibration_error(&self) -> f64 {
        self.model.maximum_uncertainty
    }

    #[must_use]
    pub fn experiment_plan(&self) -> Option<ExperimentPlan> {
        self.loaded_experiment_plan.clone()
    }

    #[must_use]
    pub const fn mode(&self) -> StartupMode {
        self.mode
    }

    #[must_use]
    pub fn controller_count(&self) -> usize {
        self.controllers.len()
    }

    #[must_use]
    pub const fn model_required(&self) -> bool {
        self.model.required
    }

    #[must_use]
    pub const fn cycle_ms(&self) -> u64 {
        self.runtime.cycle
    }

    #[must_use]
    pub fn timing_model(&self) -> TimingModel {
        let controller = &self.controllers["duct"];
        TimingModel {
            cycle_ms: self.runtime.cycle,
            input_stale_after_ms: self.runtime.input_stale_after,
            model_signals_stale_after_ms: self.runtime.model_signals_stale_after,
            controller_truth_stale_after_ms: self.runtime.controller_truth_stale_after,
            acknowledgement_deadline_ms: controller.acknowledgement_deadline_ms,
            command_lease_ms: controller.command_lease_ms,
            heartbeat_period_ms: controller.heartbeat_period_ms,
            runtime_lease_ms: controller.runtime_lease_ms,
        }
    }

    #[must_use]
    pub fn controller_runtime_configuration(&self) -> ControllerRuntimeConfiguration {
        let controller = &self.controllers["duct"];
        ControllerRuntimeConfiguration {
            address: controller.address,
            identity: controller.identity,
            configuration_generation: controller.configuration_generation,
            capability_generation: controller.capability_generation,
            resource_id: controller.resource_id,
            minimum_basis_points: controller.minimum_basis_points,
            maximum_basis_points: controller.maximum_basis_points,
            maximum_command_rate_hz: controller.maximum_command_rate_hz,
            fallback_basis_points: controller.fallback_basis_points,
            pwm_endpoint_a_us: controller.pwm_endpoint_a_us,
            pwm_endpoint_b_us: controller.pwm_endpoint_b_us,
            direction: controller.direction,
            runtime_lease_ms: controller.runtime_lease_ms,
            command_lease_ms: controller.command_lease_ms,
            heartbeat_period_ms: controller.heartbeat_period_ms,
            acknowledgement_deadline_ms: controller.acknowledgement_deadline_ms,
            normal_slew_basis_points_per_second: controller.normal_slew_basis_points_per_second,
            protection_slew_basis_points_per_second: controller
                .protection_slew_basis_points_per_second,
            digest_prefix: controller.digest_prefix,
        }
    }

    #[must_use]
    pub fn model_slots_root(&self) -> &Path {
        Path::new(&self.model.slots_root)
    }

    #[must_use]
    pub fn model_abi(&self) -> &str {
        &self.model.abi
    }

    #[must_use]
    pub const fn model_input_length(&self) -> usize {
        self.model
            .history_length
            .saturating_mul(self.model.input_signals.len())
            .saturating_add(1)
    }

    #[must_use]
    pub fn model_input_signals(&self) -> &[String] {
        &self.model.input_signals
    }

    #[must_use]
    pub const fn minimum_run_free_bytes(&self) -> u64 {
        self.run_storage.minimum_free_bytes
    }

    pub(crate) fn deterministic_command(&self, coolant_c: f64, iat_c: f64) -> u16 {
        let row = breakpoint_index(coolant_c, &self.duct.coolant_breakpoints_c);
        let column = breakpoint_index(iat_c, &self.duct.iat_breakpoints_c);
        self.duct.policy_basis_points[row][column]
    }

    #[must_use]
    pub fn simulation_scenario(&self) -> Option<&str> {
        match &self.composition {
            Composition::Simulation { scenario } => Some(scenario),
            _ => None,
        }
    }

    #[must_use]
    pub fn run_storage_root(&self) -> &Path {
        Path::new(&self.run_storage.root)
    }

    #[must_use]
    pub fn diagnostics_socket(&self) -> &Path {
        Path::new(&self.diagnostics.socket)
    }

    #[must_use]
    pub fn live_interfaces(&self) -> Option<(&str, &str)> {
        match &self.composition {
            Composition::Live {
                powertrain_interface,
                actuator_interface,
                ..
            } => Some((powertrain_interface, actuator_interface)),
            _ => None,
        }
    }
}
