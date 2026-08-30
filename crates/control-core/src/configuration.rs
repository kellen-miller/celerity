use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    ExperimentPlan, PowertrainDecodeError, PowertrainDecoder, powertrain::CantcuReception,
};

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
    cycle_ms: u64,
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
        self.runtime.cycle_ms
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

    fn validate(&self, requested_mode: StartupMode) -> Result<(), BundleError> {
        if self.schema_version != 1 {
            return Err(BundleError::UnsupportedSchema(self.schema_version));
        }
        if requested_mode != self.mode {
            return Err(BundleError::RequestedModeMismatch {
                requested: requested_mode,
                configured: self.mode,
            });
        }
        if !matches!(
            (self.mode, &self.composition),
            (StartupMode::Simulation, Composition::Simulation { .. })
                | (StartupMode::Replay, Composition::Replay { .. })
                | (StartupMode::Live, Composition::Live { .. })
        ) {
            return Err(BundleError::CompositionModeMismatch);
        }
        if self.runtime.cycle_ms == 0 || self.powertrain.decoder_generation == 0 {
            return Err(BundleError::InvalidRuntimeTiming);
        }

        let mut addresses = [false; 64];
        let mut identities = std::collections::BTreeSet::new();
        for controller in self.controllers.values() {
            if !(1..=63).contains(&controller.address) {
                return Err(BundleError::InvalidControllerAddress(controller.address));
            }
            if addresses[usize::from(controller.address)] {
                return Err(BundleError::DuplicateControllerAddress(controller.address));
            }
            if !identities.insert(controller.identity) {
                return Err(BundleError::DuplicateControllerIdentity(
                    controller.identity,
                ));
            }
            addresses[usize::from(controller.address)] = true;
        }
        if !self.controllers.contains_key("duct") {
            return Err(BundleError::MissingDuctController);
        }

        if !self.duct.radiator_split_minimum.is_finite()
            || !self.duct.radiator_split_maximum.is_finite()
            || self.duct.radiator_split_minimum < 0.0
            || self.duct.radiator_split_minimum >= self.duct.radiator_split_maximum
            || self.duct.radiator_split_maximum > 1.0
        {
            return Err(BundleError::InvalidDuctBounds);
        }
        if !strictly_increasing(&self.duct.coolant_breakpoints_c)
            || !strictly_increasing(&self.duct.iat_breakpoints_c)
            || !policy_is_monotonic(
                &self.duct.policy_basis_points,
                self.duct.radiator_split_minimum,
                self.duct.radiator_split_maximum,
            )
        {
            return Err(BundleError::NonmonotonicPolicy);
        }

        if self.run_storage.root.is_empty() || self.run_storage.minimum_free_bytes == 0 {
            return Err(BundleError::InvalidStorage);
        }
        let controller = &self.controllers["duct"];
        if controller.configuration_generation == 0
            || controller.capability_generation == 0
            || controller.resource_id == 0
            || controller.minimum_basis_points > controller.maximum_basis_points
            || controller.maximum_basis_points > 10_000
            || controller.maximum_command_rate_hz == 0
            || controller.fallback_basis_points > 10_000
            || controller.pwm_endpoint_a_us >= controller.pwm_endpoint_b_us
            || !matches!(controller.direction, 1 | 2)
            || controller.runtime_lease_ms == 0
            || controller.command_lease_ms == 0
            || controller.heartbeat_period_ms == 0
            || controller.acknowledgement_deadline_ms == 0
            || controller.normal_slew_basis_points_per_second == 0
            || controller.protection_slew_basis_points_per_second == 0
        {
            return Err(BundleError::InvalidRuntimeTiming);
        }
        if self.model.slots_root.is_empty()
            || self.model.abi.is_empty()
            || self.model.input_signals.is_empty()
            || !self
                .model
                .input_signals
                .iter()
                .any(|signal| signal == "coolant_temperature_c")
            || !self
                .model
                .input_signals
                .iter()
                .any(|signal| signal == "air_temperature_c")
            || self
                .model
                .input_signals
                .iter()
                .enumerate()
                .any(|(index, signal)| {
                    signal.is_empty() || self.model.input_signals[..index].contains(signal)
                })
            || self.model.history_length == 0
            || self.model.warmup_iterations == 0
            || !self.model.maximum_uncertainty.is_finite()
            || self.model.maximum_uncertainty < 0.0
            || !self.model.maximum_ood_score.is_finite()
            || self.model.maximum_ood_score < 0.0
            || self.model.required_post_work_ns == 0
            || !self.model.hard_coolant_ceiling_c.is_finite()
            || !self.model.coolant_target_c.is_finite()
            || !self.model.coolant_input_minimum_c.is_finite()
            || !self.model.coolant_input_maximum_c.is_finite()
            || self.model.coolant_input_minimum_c >= self.model.coolant_input_maximum_c
            || !self.model.iat_input_minimum_c.is_finite()
            || !self.model.iat_input_maximum_c.is_finite()
            || self.model.iat_input_minimum_c >= self.model.iat_input_maximum_c
            || self.model.command_lattice.is_empty()
            || self.model.command_lattice.iter().any(|command| {
                !(controller.minimum_basis_points..=controller.maximum_basis_points)
                    .contains(command)
            })
            || self
                .model
                .command_lattice
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(BundleError::InvalidRuntimeTiming);
        }
        if self.loaded_experiment_plan.as_ref().is_some_and(|plan| {
            !plan.commands_within(
                controller.minimum_basis_points,
                controller.maximum_basis_points,
            )
        }) {
            return Err(BundleError::InvalidRuntimeTiming);
        }
        if self.sync.spool_root != self.run_storage.root
            || self.sync.retention_count == 0
            || self.sync.home_interface.is_empty()
            || self.sync.expected_default_gateway.is_empty()
            || !self.sync.home_api_url.starts_with("https://")
            || self.sync.credential_path.is_empty()
            || self.diagnostics.socket.is_empty()
        {
            return Err(BundleError::InvalidSync);
        }

        let composition_valid = match &self.composition {
            Composition::Simulation { scenario } => !scenario.is_empty(),
            Composition::Replay { run, behavior } => {
                let _ = behavior;
                !run.is_empty()
            }
            Composition::Live {
                powertrain_interface,
                actuator_interface,
                watchdog,
            } => {
                !powertrain_interface.is_empty()
                    && !actuator_interface.is_empty()
                    && powertrain_interface != actuator_interface
                    && *watchdog
            }
        };
        if composition_valid {
            Ok(())
        } else {
            Err(BundleError::CompositionModeMismatch)
        }
    }
}

fn strictly_increasing(values: &[f64; 3]) -> bool {
    values.iter().all(|value| value.is_finite())
        && values.windows(2).all(|window| window[0] < window[1])
}

fn breakpoint_index(value: f64, breakpoints: &[f64; 3]) -> usize {
    if value < breakpoints[1] {
        0
    } else if value < breakpoints[2] {
        1
    } else {
        2
    }
}

fn policy_is_monotonic(table: &[[u16; 3]; 3], minimum: f64, maximum: f64) -> bool {
    for row in table {
        if row.iter().any(|value| {
            let normalized = f64::from(*value) / 10_000.0;
            normalized < minimum || normalized > maximum
        }) || row.windows(2).any(|window| window[0] > window[1])
        {
            return false;
        }
    }
    for ((first, second), third) in table[0].iter().zip(&table[1]).zip(&table[2]) {
        if first > second || second > third {
            return false;
        }
    }
    true
}

fn digest(source: &str) -> String {
    use std::fmt::Write as _;

    Sha256::digest(source.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        })
}

fn load_experiment(
    bundle: &ValidatedBundle,
    bundle_path: &Path,
) -> Result<Option<ExperimentPlan>, BundleError> {
    let Some(configuration) = &bundle.experiment else {
        return Ok(None);
    };
    if configuration.plan.is_empty() {
        return Err(BundleError::InvalidRuntimeTiming);
    }
    let configured = Path::new(&configuration.plan);
    let path = if configured.is_absolute() {
        configured.to_path_buf()
    } else {
        bundle_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(configured)
    };
    ExperimentPlan::load(&path)
        .map(Some)
        .map_err(|_| BundleError::InvalidRuntimeTiming)
}
