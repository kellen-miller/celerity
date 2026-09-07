use crate::StartupMode;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeatureAuthority {
    Fallback,
    Arming,
    Active,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandSource {
    ControllerLocalFallback,
    Deterministic,
    ModelOptimized,
    Experiment,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeEvent {
    InputSnapshot {
        monotonic_ms: u64,
        observed_monotonic_ms: u64,
        coolant_c: f64,
        iat_c: f64,
    },
    ModelSignalSnapshot {
        monotonic_ms: u64,
        values: Vec<f64>,
    },
    ModelSignalsUnavailable {
        monotonic_ms: u64,
    },
    ControllerTruth {
        monotonic_ms: u64,
        boot_session: u32,
        configuration_generation: u32,
        identity_matches: bool,
    },
    ControllerUnavailable {
        monotonic_ms: u64,
    },
    CommandAcknowledged {
        monotonic_ms: u64,
        boot_session: u32,
        command_sequence: u32,
        accepted: bool,
    },
    Cycle {
        monotonic_ms: u64,
        remaining_cycle_ns: u64,
    },
    SharedHardFault {
        monotonic_ms: u64,
    },
    Shutdown {
        monotonic_ms: u64,
    },
}

impl RuntimeEvent {
    pub(crate) const fn monotonic_ms_for_evidence(&self) -> u64 {
        match self {
            Self::InputSnapshot { monotonic_ms, .. }
            | Self::ModelSignalSnapshot { monotonic_ms, .. }
            | Self::ModelSignalsUnavailable { monotonic_ms }
            | Self::ControllerTruth { monotonic_ms, .. }
            | Self::ControllerUnavailable { monotonic_ms }
            | Self::CommandAcknowledged { monotonic_ms, .. }
            | Self::Cycle { monotonic_ms, .. }
            | Self::SharedHardFault { monotonic_ms }
            | Self::Shutdown { monotonic_ms } => *monotonic_ms,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeEffect {
    RenewRuntimeLease {
        sequence: u32,
    },
    SendCommand {
        sequence: u32,
        radiator_split_basis_points: u16,
    },
    RecordAuthority(FeatureAuthority),
    StopLeaseRenewal,
    RequestFallback,
    RecordHardFault,
    RecordExperiment(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExternalAdapters {
    Simulation(Vec<RuntimeEvent>),
    Replay(Vec<RuntimeEvent>),
    Live(Vec<RuntimeEvent>),
}

impl ExternalAdapters {
    #[must_use]
    pub const fn simulation(events: Vec<RuntimeEvent>) -> Self {
        Self::Simulation(events)
    }

    #[must_use]
    pub const fn replay(events: Vec<RuntimeEvent>) -> Self {
        Self::Replay(events)
    }

    #[must_use]
    pub const fn live(events: Vec<RuntimeEvent>) -> Self {
        Self::Live(events)
    }

    pub(super) const fn mode(&self) -> StartupMode {
        match self {
            Self::Simulation(_) => StartupMode::Simulation,
            Self::Replay(_) => StartupMode::Replay,
            Self::Live(_) => StartupMode::Live,
        }
    }

    pub(super) fn into_events(self) -> Vec<RuntimeEvent> {
        match self {
            Self::Simulation(events) | Self::Replay(events) | Self::Live(events) => events,
        }
    }
}
