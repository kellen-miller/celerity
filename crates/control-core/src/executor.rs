use crate::{
    ExperimentAbort, ExperimentDecision, ExperimentPlan, ModelCommandSelection, RunningExperiment,
    RuntimeModel, StartupMode, ValidatedBundle,
};
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
    pub accepted_model_command_observed: bool,
}

pub struct Runtime {
    bundle: ValidatedBundle,
    events: Vec<RuntimeEvent>,
    model: Option<RuntimeModel>,
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
            model: None,
        })
    }

    /// Constructs the same runtime with one graph selected and loaded before
    /// authority begins.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeStartError::CompositionModeMismatch`] for a mismatched
    /// event adapter.
    pub fn new_with_model(
        bundle: ValidatedBundle,
        adapters: ExternalAdapters,
        model: RuntimeModel,
    ) -> Result<Self, RuntimeStartError> {
        let mut runtime = Self::new(bundle, adapters)?;
        runtime.model = Some(model);
        Ok(runtime)
    }

    #[must_use]
    pub fn run(self) -> RuntimeOutcome {
        let mut session = RuntimeSession::start(self.bundle, self.model);
        let mut effects = Vec::new();
        for event in self.events {
            effects.extend(session.ingest(event));
        }
        let mut outcome = session.outcome();
        outcome.effects = effects;
        outcome
    }
}

pub struct RuntimeSession {
    bundle: ValidatedBundle,
    state: ExecutorState,
}

impl RuntimeSession {
    #[must_use]
    pub fn start(bundle: ValidatedBundle, model: Option<RuntimeModel>) -> Self {
        let state = ExecutorState::new(&bundle, model);
        Self { bundle, state }
    }

    /// Applies one ordered input to the single writer and returns only the new
    /// concrete effects that the owning composition must execute.
    #[must_use]
    pub fn ingest(&mut self, event: RuntimeEvent) -> Vec<RuntimeEffect> {
        self.state.ingest(&self.bundle, event);
        std::mem::take(&mut self.state.effects)
    }

    #[must_use]
    pub fn outcome(&self) -> RuntimeOutcome {
        self.state.outcome()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct InputSnapshot {
    observed_monotonic_ms: u64,
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
    model_snapshot_at_ms: Option<u64>,
    controller: Option<ControllerTruth>,
    acknowledged_boot_session: Option<u32>,
    acknowledged_at_ms: Option<u64>,
    last_command: Option<(u32, u16, CommandSource)>,
    lease_sequence: u32,
    command_sequence: u32,
    feature_authority: FeatureAuthority,
    command_source: CommandSource,
    accepted_basis_points: Option<u16>,
    hard_fault_latched: bool,
    effects: Vec<RuntimeEffect>,
    model: Option<RuntimeModel>,
    experiment_plan: Option<ExperimentPlan>,
    experiment: Option<RunningExperiment>,
    experiment_finished: bool,
    last_experiment_command: Option<u16>,
    accepted_model_command_observed: bool,
}

impl ExecutorState {
    fn new(bundle: &ValidatedBundle, model: Option<RuntimeModel>) -> Self {
        Self {
            maximum_input_age_ms: bundle.cycle_ms().saturating_mul(2),
            last_monotonic_ms: None,
            input: None,
            model_snapshot_at_ms: None,
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
            model,
            experiment_plan: bundle.experiment_plan(),
            experiment: None,
            experiment_finished: false,
            last_experiment_command: None,
            accepted_model_command_observed: false,
        }
    }

    fn ingest(&mut self, bundle: &ValidatedBundle, event: RuntimeEvent) {
        let now_ms = event.monotonic_ms_for_evidence();
        if self.last_monotonic_ms.is_some_and(|last| now_ms < last) {
            self.abort_experiment(ExperimentAbort::TimeRegression);
            self.latch_hard_fault();
            return;
        }
        self.last_monotonic_ms = Some(now_ms);
        match event {
            RuntimeEvent::InputSnapshot {
                observed_monotonic_ms,
                coolant_c,
                iat_c,
                ..
            } => {
                if coolant_c.is_finite() && iat_c.is_finite() {
                    self.input = Some(InputSnapshot {
                        observed_monotonic_ms,
                        coolant_c,
                        iat_c,
                    });
                } else {
                    self.abort_experiment(ExperimentAbort::StaleInput);
                    self.select_fallback();
                }
            }
            RuntimeEvent::ModelSignalSnapshot {
                monotonic_ms,
                values,
            } => {
                if values.len() == bundle.model_input_signals().len()
                    && values.iter().all(|value| value.is_finite())
                {
                    self.model_snapshot_at_ms = Some(monotonic_ms);
                    if let Some(model) = &mut self.model {
                        model.observe(values);
                    }
                } else {
                    self.model_snapshot_at_ms = None;
                    if let Some(model) = &mut self.model {
                        model.clear_history();
                    }
                }
            }
            RuntimeEvent::ModelSignalsUnavailable { .. } => {
                self.model_snapshot_at_ms = None;
                if let Some(model) = &mut self.model {
                    model.clear_history();
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
                    self.abort_experiment(ExperimentAbort::AuthorityLost);
                    self.select_fallback();
                }
                self.controller = Some(ControllerTruth {
                    monotonic_ms,
                    boot_session,
                    configuration_generation,
                    identity_matches,
                });
            }
            RuntimeEvent::ControllerUnavailable { .. } => {
                self.controller = None;
                self.acknowledged_boot_session = None;
                self.acknowledged_at_ms = None;
                self.abort_experiment(ExperimentAbort::AuthorityLost);
                self.select_fallback();
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
                        .is_some_and(|(sequence, _, _)| sequence == command_sequence);
                if valid {
                    if let Some(controller) = &mut self.controller {
                        controller.monotonic_ms = monotonic_ms;
                    }
                    if self
                        .last_command
                        .is_some_and(|(_, _, source)| source == CommandSource::ModelOptimized)
                    {
                        self.accepted_model_command_observed = true;
                    }
                    self.acknowledged_boot_session = Some(boot_session);
                    self.acknowledged_at_ms = Some(monotonic_ms);
                    self.accepted_basis_points = self.last_command.map(|(_, value, _)| value);
                    self.command_source = self
                        .last_command
                        .map_or(CommandSource::Deterministic, |(_, _, source)| source);
                    self.transition(FeatureAuthority::Active);
                } else {
                    self.abort_experiment(ExperimentAbort::AuthorityLost);
                    self.select_fallback();
                }
            }
            RuntimeEvent::Cycle {
                monotonic_ms,
                remaining_cycle_ns,
            } => self.cycle(bundle, monotonic_ms, remaining_cycle_ns),
            RuntimeEvent::SharedHardFault { .. } => self.latch_hard_fault(),
            RuntimeEvent::Shutdown { .. } => {
                self.abort_experiment(ExperimentAbort::AuthorityLost);
                self.select_fallback();
            }
        }
    }

    fn cycle(&mut self, bundle: &ValidatedBundle, now_ms: u64, remaining_cycle_ns: u64) {
        if self.hard_fault_latched {
            self.select_fallback();
            return;
        }
        let Some(input) = self.input else {
            self.abort_experiment(ExperimentAbort::StaleInput);
            self.select_fallback();
            return;
        };
        let Some(controller) = self.controller else {
            self.abort_experiment(ExperimentAbort::AuthorityLost);
            self.select_fallback();
            return;
        };
        let healthy = now_ms.saturating_sub(input.observed_monotonic_ms)
            <= self.maximum_input_age_ms
            && now_ms.saturating_sub(controller.monotonic_ms) <= self.maximum_input_age_ms
            && controller.identity_matches
            && controller.configuration_generation > 0;
        if !healthy {
            let reason =
                if now_ms.saturating_sub(input.observed_monotonic_ms) > self.maximum_input_age_ms {
                    ExperimentAbort::StaleInput
                } else {
                    ExperimentAbort::AuthorityLost
                };
            self.abort_experiment(reason);
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

        let deterministic = bundle.deterministic_command(input.coolant_c, input.iat_c);
        let current = self.accepted_basis_points.unwrap_or(deterministic);
        let model_fresh = self
            .model_snapshot_at_ms
            .is_some_and(|observed| now_ms.saturating_sub(observed) <= self.maximum_input_age_ms);
        let (mut command, mut source) = self.model.as_ref().filter(|_| model_fresh).map_or(
            (deterministic, CommandSource::Deterministic),
            |model| match model.select(bundle, deterministic, current, remaining_cycle_ns) {
                ModelCommandSelection::Optimized { split_basis_points } => {
                    (split_basis_points, CommandSource::ModelOptimized)
                }
                ModelCommandSelection::Deterministic {
                    split_basis_points, ..
                } => (split_basis_points, CommandSource::Deterministic),
            },
        );
        if !self.experiment_finished && self.feature_authority == FeatureAuthority::Active {
            if self.experiment.is_none()
                && let Some(plan) = self.experiment_plan.take()
            {
                self.experiment = Some(plan.start(now_ms));
                self.effects.push(RuntimeEffect::RecordExperiment(
                    r#"{"event":"start"}"#.to_owned(),
                ));
            }
            if let Some(experiment) = &mut self.experiment {
                match experiment.advance(
                    now_ms,
                    true,
                    input.coolant_c,
                    true,
                    self.accepted_basis_points,
                ) {
                    ExperimentDecision::Apply {
                        radiator_split_basis_points,
                    } => {
                        if self.last_experiment_command != Some(radiator_split_basis_points) {
                            self.effects.push(RuntimeEffect::RecordExperiment(format!(
                                r#"{{"event":"step","radiator_split_basis_points":{radiator_split_basis_points}}}"#
                            )));
                            self.last_experiment_command = Some(radiator_split_basis_points);
                        }
                        command = radiator_split_basis_points;
                        source = CommandSource::Experiment;
                    }
                    ExperimentDecision::Complete => {
                        self.effects.push(RuntimeEffect::RecordExperiment(
                            r#"{"event":"complete"}"#.to_owned(),
                        ));
                        self.experiment = None;
                        self.experiment_finished = true;
                    }
                    ExperimentDecision::Abort(reason) => self.abort_experiment(reason),
                }
            }
        }
        self.lease_sequence = self.lease_sequence.wrapping_add(1);
        self.command_sequence = self.command_sequence.wrapping_add(1);
        self.effects.push(RuntimeEffect::RenewRuntimeLease {
            sequence: self.lease_sequence,
        });
        self.effects.push(RuntimeEffect::SendCommand {
            sequence: self.command_sequence,
            radiator_split_basis_points: command,
        });
        self.last_command = Some((self.command_sequence, command, source));
        self.command_source = source;
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

    fn abort_experiment(&mut self, reason: ExperimentAbort) {
        if self.experiment.take().is_some() {
            self.effects.push(RuntimeEffect::RecordExperiment(format!(
                r#"{{"event":"abort","reason":"{reason:?}"}}"#
            )));
            self.experiment_finished = true;
            self.last_experiment_command = None;
        }
    }

    fn latch_hard_fault(&mut self) {
        if !self.hard_fault_latched {
            self.hard_fault_latched = true;
            self.effects.push(RuntimeEffect::RecordHardFault);
        }
        self.abort_experiment(ExperimentAbort::AuthorityLost);
        self.select_fallback();
    }

    fn outcome(&self) -> RuntimeOutcome {
        RuntimeOutcome {
            final_feature_authority: self.feature_authority,
            command_source: self.command_source,
            accepted_basis_points: self.accepted_basis_points,
            effects: self.effects.clone(),
            hard_fault_latched: self.hard_fault_latched,
            accepted_model_command_observed: self.accepted_model_command_observed,
        }
    }
}
