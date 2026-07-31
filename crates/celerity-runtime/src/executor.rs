use crate::{StartupMode, ValidatedBundle};
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
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RuntimeEvent {
    InputSnapshot {
        monotonic_ms: u64,
        coolant_c: f64,
        iat_c: f64,
    },
    ControllerTruth {
        monotonic_ms: u64,
        boot_session: u32,
        configuration_generation: u32,
        identity_matches: bool,
    },
    CommandAcknowledged {
        monotonic_ms: u64,
        boot_session: u32,
        command_sequence: u32,
        accepted: bool,
    },
    Cycle {
        monotonic_ms: u64,
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
            | Self::ControllerTruth { monotonic_ms, .. }
            | Self::CommandAcknowledged { monotonic_ms, .. }
            | Self::Cycle { monotonic_ms }
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

    const fn mode(&self) -> StartupMode {
        match self {
            Self::Simulation(_) => StartupMode::Simulation,
            Self::Replay(_) => StartupMode::Replay,
            Self::Live(_) => StartupMode::Live,
        }
    }

    fn into_events(self) -> Vec<RuntimeEvent> {
        match self {
            Self::Simulation(events) | Self::Replay(events) | Self::Live(events) => events,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RuntimeStartError {
    CompositionModeMismatch,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeOutcome {
    pub final_feature_authority: FeatureAuthority,
    pub command_source: CommandSource,
    pub accepted_basis_points: Option<u16>,
    pub effects: Vec<RuntimeEffect>,
    pub hard_fault_latched: bool,
}

pub struct Runtime {
    bundle: ValidatedBundle,
    events: Vec<RuntimeEvent>,
}

impl Runtime {
    /// Constructs the single-writer runtime with one concrete production
    /// composition. The event source varies; policy and authority do not.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeStartError::CompositionModeMismatch`] if the adapter
    /// kind does not match the validated startup bundle.
    pub fn new(
        bundle: ValidatedBundle,
        adapters: ExternalAdapters,
    ) -> Result<Self, RuntimeStartError> {
        if bundle.mode() != adapters.mode() {
            return Err(RuntimeStartError::CompositionModeMismatch);
        }
        Ok(Self {
            bundle,
            events: adapters.into_events(),
        })
    }

    #[must_use]
    pub fn run(self) -> RuntimeOutcome {
        let mut state = ExecutorState::new(&self.bundle);
        for event in self.events {
            state.ingest(&self.bundle, event);
        }
        state.outcome()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct InputSnapshot {
    monotonic_ms: u64,
    coolant_c: f64,
    iat_c: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ControllerTruth {
    monotonic_ms: u64,
    boot_session: u32,
    configuration_generation: u32,
    identity_matches: bool,
}

struct ExecutorState {
    maximum_input_age_ms: u64,
    last_monotonic_ms: Option<u64>,
    input: Option<InputSnapshot>,
    controller: Option<ControllerTruth>,
    acknowledged_boot_session: Option<u32>,
    acknowledged_at_ms: Option<u64>,
    last_command: Option<(u32, u16)>,
    lease_sequence: u32,
    command_sequence: u32,
    feature_authority: FeatureAuthority,
    command_source: CommandSource,
    accepted_basis_points: Option<u16>,
    hard_fault_latched: bool,
    effects: Vec<RuntimeEffect>,
}

impl ExecutorState {
    fn new(bundle: &ValidatedBundle) -> Self {
        Self {
            maximum_input_age_ms: bundle.cycle_ms().saturating_mul(2),
            last_monotonic_ms: None,
            input: None,
            controller: None,
            acknowledged_boot_session: None,
            acknowledged_at_ms: None,
            last_command: None,
            lease_sequence: 0,
            command_sequence: 0,
            feature_authority: FeatureAuthority::Fallback,
            command_source: CommandSource::ControllerLocalFallback,
            accepted_basis_points: None,
            hard_fault_latched: false,
            effects: Vec::new(),
        }
    }

    fn ingest(&mut self, bundle: &ValidatedBundle, event: RuntimeEvent) {
        let now_ms = event.monotonic_ms_for_evidence();
        if self.last_monotonic_ms.is_some_and(|last| now_ms < last) {
            self.latch_hard_fault();
            return;
        }
        self.last_monotonic_ms = Some(now_ms);
        match event {
            RuntimeEvent::InputSnapshot {
                monotonic_ms,
                coolant_c,
                iat_c,
            } => {
                if coolant_c.is_finite() && iat_c.is_finite() {
                    self.input = Some(InputSnapshot {
                        monotonic_ms,
                        coolant_c,
                        iat_c,
                    });
                } else {
                    self.select_fallback();
                }
            }
            RuntimeEvent::ControllerTruth {
                monotonic_ms,
                boot_session,
                configuration_generation,
                identity_matches,
                ..
            } => {
                if self
                    .controller
                    .is_some_and(|controller| controller.boot_session != boot_session)
                {
                    self.acknowledged_boot_session = None;
                    self.acknowledged_at_ms = None;
                    self.accepted_basis_points = None;
                    self.select_fallback();
                }
                self.controller = Some(ControllerTruth {
                    monotonic_ms,
                    boot_session,
                    configuration_generation,
                    identity_matches,
                });
            }
            RuntimeEvent::CommandAcknowledged {
                monotonic_ms,
                boot_session,
                command_sequence,
                accepted,
                ..
            } => {
                let valid = accepted
                    && self.controller.is_some_and(|controller| {
                        controller.boot_session == boot_session && controller.identity_matches
                    })
                    && self
                        .last_command
                        .is_some_and(|(sequence, _)| sequence == command_sequence);
                if valid {
                    self.acknowledged_boot_session = Some(boot_session);
                    self.acknowledged_at_ms = Some(monotonic_ms);
                    self.accepted_basis_points = self.last_command.map(|(_, value)| value);
                    self.command_source = CommandSource::Deterministic;
                    self.transition(FeatureAuthority::Active);
                } else {
                    self.select_fallback();
                }
            }
            RuntimeEvent::Cycle { monotonic_ms } => self.cycle(bundle, monotonic_ms),
            RuntimeEvent::SharedHardFault { .. } => self.latch_hard_fault(),
            RuntimeEvent::Shutdown { .. } => self.select_fallback(),
        }
    }

    fn cycle(&mut self, bundle: &ValidatedBundle, now_ms: u64) {
        if self.hard_fault_latched {
            self.select_fallback();
            return;
        }
        let Some(input) = self.input else {
            self.select_fallback();
            return;
        };
        let Some(controller) = self.controller else {
            self.select_fallback();
            return;
        };
        let healthy = now_ms.saturating_sub(input.monotonic_ms) <= self.maximum_input_age_ms
            && now_ms.saturating_sub(controller.monotonic_ms) <= self.maximum_input_age_ms
            && controller.identity_matches
            && controller.configuration_generation > 0;
        if !healthy {
            self.select_fallback();
            return;
        }
        if self.feature_authority == FeatureAuthority::Active
            && self.acknowledged_at_ms.is_none_or(|acknowledged| {
                now_ms.saturating_sub(acknowledged) > self.maximum_input_age_ms
            })
        {
            self.select_fallback();
            return;
        }

        let command = bundle.deterministic_command(input.coolant_c, input.iat_c);
        self.lease_sequence = self.lease_sequence.wrapping_add(1);
        self.command_sequence = self.command_sequence.wrapping_add(1);
        self.effects.push(RuntimeEffect::RenewRuntimeLease {
            sequence: self.lease_sequence,
        });
        self.effects.push(RuntimeEffect::SendCommand {
            sequence: self.command_sequence,
            radiator_split_basis_points: command,
        });
        self.last_command = Some((self.command_sequence, command));
        self.command_source = CommandSource::Deterministic;
        if self.acknowledged_boot_session == Some(controller.boot_session) {
            self.transition(FeatureAuthority::Active);
        } else {
            self.transition(FeatureAuthority::Arming);
        }
    }

    fn transition(&mut self, authority: FeatureAuthority) {
        if self.feature_authority != authority {
            self.feature_authority = authority;
            self.effects.push(RuntimeEffect::RecordAuthority(authority));
        }
    }

    fn select_fallback(&mut self) {
        if self.feature_authority != FeatureAuthority::Fallback {
            self.effects.push(RuntimeEffect::StopLeaseRenewal);
            self.effects.push(RuntimeEffect::RequestFallback);
            self.transition(FeatureAuthority::Fallback);
        }
        self.command_source = CommandSource::ControllerLocalFallback;
        self.accepted_basis_points = None;
    }

    fn latch_hard_fault(&mut self) {
        if !self.hard_fault_latched {
            self.hard_fault_latched = true;
            self.effects.push(RuntimeEffect::RecordHardFault);
        }
        self.select_fallback();
    }

    fn outcome(self) -> RuntimeOutcome {
        RuntimeOutcome {
            final_feature_authority: self.feature_authority,
            command_source: self.command_source,
            accepted_basis_points: self.accepted_basis_points,
            effects: self.effects,
            hard_fault_latched: self.hard_fault_latched,
        }
    }
}
