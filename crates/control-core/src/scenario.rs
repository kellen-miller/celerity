use std::{fs, path::Path};

use serde::Deserialize;

use crate::RuntimeEvent;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScenarioError {
    Read(String),
    Parse(String),
    Empty,
    NonmonotonicTime,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SimulationScenario {
    name: String,
    events: Vec<RuntimeEvent>,
}

impl SimulationScenario {
    /// Loads a declarative scenario containing only typed production events.
    ///
    /// # Errors
    ///
    /// Returns a typed error for filesystem, syntax, empty, or nonmonotonic
    /// logical-time input.
    pub fn load(path: &Path) -> Result<Self, ScenarioError> {
        let source =
            fs::read_to_string(path).map_err(|error| ScenarioError::Read(error.to_string()))?;
        let scenario: Self =
            toml::from_str(&source).map_err(|error| ScenarioError::Parse(error.to_string()))?;
        if scenario.name.is_empty() || scenario.events.is_empty() {
            return Err(ScenarioError::Empty);
        }
        if scenario.events.windows(2).any(|events| {
            events[0].monotonic_ms_for_evidence() > events[1].monotonic_ms_for_evidence()
        }) {
            return Err(ScenarioError::NonmonotonicTime);
        }
        Ok(scenario)
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn events(&self) -> &[RuntimeEvent] {
        &self.events
    }

    #[must_use]
    pub fn into_events(self) -> Vec<RuntimeEvent> {
        self.events
    }
}
