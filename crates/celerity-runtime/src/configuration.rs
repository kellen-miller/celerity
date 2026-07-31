use std::{collections::BTreeMap, fs, path::Path};

use serde::Deserialize;

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
    InvalidDuctBounds,
    NonmonotonicPolicy,
    InvalidRuntimeTiming,
    InvalidStorage,
    InvalidSync,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ValidatedBundle {
    schema_version: u32,
    generation: u64,
    mode: StartupMode,
    runtime: RuntimeConfiguration,
    powertrain: PowertrainConfiguration,
    controllers: BTreeMap<String, ControllerConfiguration>,
    duct: DuctConfiguration,
    model: ModelConfiguration,
    run_storage: RunStorageConfiguration,
    diagnostics: DiagnosticsConfiguration,
    sync: SyncConfiguration,
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
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct ControllerConfiguration {
    address: u8,
    identity: u64,
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
struct ModelConfiguration {
    required: bool,
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
        let source =
            fs::read_to_string(path).map_err(|error| BundleError::Read(error.to_string()))?;
        let bundle: Self =
            toml::from_str(&source).map_err(|error| BundleError::Parse(error.to_string()))?;
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
        let source =
            fs::read_to_string(path).map_err(|error| BundleError::Read(error.to_string()))?;
        let bundle: Self =
            toml::from_str(&source).map_err(|error| BundleError::Parse(error.to_string()))?;
        bundle.validate(requested_mode)?;
        Ok(bundle)
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
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

    pub(crate) const fn cycle_ms(&self) -> u64 {
        self.runtime.cycle_ms
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
