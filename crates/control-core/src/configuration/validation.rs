use std::path::Path;

use sha2::{Digest, Sha256};

use super::{
    BundleError, Composition, POWERTRAIN_TEMPERATURE_PERIOD_MS, StartupMode, ValidatedBundle,
};
use crate::ExperimentPlan;

impl ValidatedBundle {
    pub(super) fn validate(&self, requested_mode: StartupMode) -> Result<(), BundleError> {
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
        if self.runtime.cycle == 0
            || self.runtime.input_stale_after == 0
            || self.runtime.model_signals_stale_after == 0
            || self.runtime.controller_truth_stale_after == 0
            || self.powertrain.decoder_generation == 0
        {
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
        let heartbeat_window_ms = u64::from(controller.heartbeat_period_ms).saturating_mul(2);
        if self.runtime.input_stale_after <= POWERTRAIN_TEMPERATURE_PERIOD_MS
            || self.runtime.model_signals_stale_after <= POWERTRAIN_TEMPERATURE_PERIOD_MS
            || self.runtime.cycle >= u64::from(controller.acknowledgement_deadline_ms)
            || controller.acknowledgement_deadline_ms >= controller.command_lease_ms
            || heartbeat_window_ms >= self.runtime.controller_truth_stale_after
            || self.runtime.controller_truth_stale_after >= u64::from(controller.runtime_lease_ms)
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
            || self.sync.incomplete_retention_count == 0
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

pub(super) fn strictly_increasing(values: &[f64; 3]) -> bool {
    values.iter().all(|value| value.is_finite())
        && values.windows(2).all(|window| window[0] < window[1])
}

pub(super) fn breakpoint_index(value: f64, breakpoints: &[f64; 3]) -> usize {
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

pub(super) fn digest(source: &str) -> String {
    use std::fmt::Write as _;

    Sha256::digest(source.as_bytes())
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to String cannot fail");
            output
        })
}

pub(super) fn load_experiment(
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
