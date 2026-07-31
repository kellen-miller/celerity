//! Deterministic vehicle authority and evidence lifecycle.

mod configuration;
mod controller;
mod executor;
mod experiment;
mod model;
mod run;
mod scenario;

pub use configuration::{
    BundleError, ControllerRuntimeConfiguration, StartupMode, ValidatedBundle,
};
pub use controller::{ControllerEmulator, ControllerMode, EmulatorProvisioning, EmulatorResponse};
pub use executor::{
    CommandSource, ExternalAdapters, FeatureAuthority, Runtime, RuntimeEffect, RuntimeEvent,
    RuntimeOutcome, RuntimeSession, RuntimeStartError,
};
pub use experiment::{
    ExperimentAbort, ExperimentDecision, ExperimentError, ExperimentPlan, RunningExperiment,
};
pub use model::{
    CandidatePrediction, InferenceBudget, ModelCommandSelection, ModelEligibilityInput, ModelError,
    ModelIneligibility, ModelSlot, ModelSlots, OptimizerDecision, RuntimeModel, SelectedModel,
    SlotError, ThermalOptimizer, TractModel, select_model_or_deterministic,
};
pub use run::{
    Completion, EnqueueResult, RawCanEvidence, RunContext, RunManifest, RunRecord, RunWriter,
    recover_incomplete_runs, replay_events,
};
pub use scenario::{ScenarioError, SimulationScenario};

/// Runtime contract version used by configuration and diagnostics.
pub const RUNTIME_SCHEMA_VERSION: &str = "1";
