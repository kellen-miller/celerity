use std::{fs, path::Path};

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExperimentPlan {
    schema_version: u32,
    name: String,
    maximum_duration_ms: u64,
    maximum_coolant_c: i16,
    settling_ms: u64,
    steps: Vec<ExperimentStep>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct ExperimentStep {
    duration_ms: u64,
    radiator_split_basis_points: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExperimentError {
    Read(String),
    Parse(String),
    InvalidPlan,
}

impl ExperimentPlan {
    /// Loads a bounded declarative plan. It cannot name authority or leases.
    ///
    /// # Errors
    ///
    /// Returns an error for filesystem, syntax, version, range, or total
    /// duration failures.
    pub fn load(path: &Path) -> Result<Self, ExperimentError> {
        let source =
            fs::read_to_string(path).map_err(|error| ExperimentError::Read(error.to_string()))?;
        let plan: Self =
            toml::from_str(&source).map_err(|error| ExperimentError::Parse(error.to_string()))?;
        let total_duration = plan
            .steps
            .iter()
            .fold(0_u64, |total, step| total.saturating_add(step.duration_ms));
        if plan.schema_version != 1
            || plan.name.is_empty()
            || plan.maximum_duration_ms == 0
            || plan.maximum_duration_ms > 300_000
            || plan.maximum_coolant_c <= 0
            || plan.settling_ms == 0
            || plan.steps.is_empty()
            || plan
                .steps
                .iter()
                .any(|step| step.duration_ms == 0 || step.radiator_split_basis_points > 10_000)
            || total_duration > plan.maximum_duration_ms
        {
            return Err(ExperimentError::InvalidPlan);
        }
        Ok(plan)
    }

    #[must_use]
    pub fn start(self, now_ms: u64) -> RunningExperiment {
        RunningExperiment {
            plan: self,
            started_ms: now_ms,
            step_index: 0,
            phase: ExperimentPhase::WaitingForAcceptance,
            last_observed_ms: now_ms,
            terminal: false,
        }
    }

    pub(crate) fn commands_within(&self, minimum: u16, maximum: u16) -> bool {
        self.steps
            .iter()
            .all(|step| (minimum..=maximum).contains(&step.radiator_split_basis_points))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExperimentDecision {
    Apply { radiator_split_basis_points: u16 },
    Complete,
    Abort(ExperimentAbort),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExperimentAbort {
    StaleInput,
    CoolantCeiling,
    AuthorityLost,
    TimeRegression,
    DurationExceeded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExperimentPhase {
    WaitingForAcceptance,
    Settling { started_ms: u64 },
    Holding { started_ms: u64 },
}

pub struct RunningExperiment {
    plan: ExperimentPlan,
    started_ms: u64,
    step_index: usize,
    phase: ExperimentPhase,
    last_observed_ms: u64,
    terminal: bool,
}

impl RunningExperiment {
    #[must_use]
    pub fn advance(
        &mut self,
        now_ms: u64,
        input_fresh: bool,
        coolant_c: f64,
        authority_active: bool,
        accepted_basis_points: Option<u16>,
    ) -> ExperimentDecision {
        if self.terminal {
            return ExperimentDecision::Complete;
        }
        let abort = if now_ms < self.last_observed_ms {
            Some(ExperimentAbort::TimeRegression)
        } else if now_ms.saturating_sub(self.started_ms) > self.plan.maximum_duration_ms {
            Some(ExperimentAbort::DurationExceeded)
        } else if !input_fresh || !coolant_c.is_finite() {
            Some(ExperimentAbort::StaleInput)
        } else if coolant_c > f64::from(self.plan.maximum_coolant_c) {
            Some(ExperimentAbort::CoolantCeiling)
        } else if !authority_active {
            Some(ExperimentAbort::AuthorityLost)
        } else {
            None
        };
        if let Some(abort) = abort {
            self.terminal = true;
            return ExperimentDecision::Abort(abort);
        }
        self.last_observed_ms = now_ms;
        let Some(step) = self.plan.steps.get(self.step_index) else {
            self.terminal = true;
            return ExperimentDecision::Complete;
        };
        if self.phase != ExperimentPhase::WaitingForAcceptance
            && accepted_basis_points != Some(step.radiator_split_basis_points)
        {
            self.terminal = true;
            return ExperimentDecision::Abort(ExperimentAbort::AuthorityLost);
        }
        self.phase = match self.phase {
            ExperimentPhase::WaitingForAcceptance
                if accepted_basis_points == Some(step.radiator_split_basis_points) =>
            {
                ExperimentPhase::Settling { started_ms: now_ms }
            }
            ExperimentPhase::Settling { started_ms }
                if now_ms.saturating_sub(started_ms) >= self.plan.settling_ms =>
            {
                ExperimentPhase::Holding {
                    started_ms: started_ms.saturating_add(self.plan.settling_ms),
                }
            }
            ExperimentPhase::Holding { started_ms }
                if now_ms.saturating_sub(started_ms) >= step.duration_ms =>
            {
                self.step_index += 1;
                if self.step_index == self.plan.steps.len() {
                    self.terminal = true;
                    return ExperimentDecision::Complete;
                }
                ExperimentPhase::WaitingForAcceptance
            }
            phase => phase,
        };
        let step = &self.plan.steps[self.step_index];
        ExperimentDecision::Apply {
            radiator_split_basis_points: step.radiator_split_basis_points,
        }
    }
}
