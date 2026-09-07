use std::time::Instant;

use serde::{Deserialize, Serialize};
use vehicle_diagnostics::DiagnosticsSnapshot;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationState {
    Current,
    Stale,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct StatusEnvelope {
    pub schema_version: u32,
    pub state: PresentationState,
    pub consecutive_failures: u32,
    pub last_success_age_ms: Option<u64>,
    pub snapshot: Option<DiagnosticsSnapshot>,
}

pub struct StatusObserver {
    snapshot: Option<DiagnosticsSnapshot>,
    last_success_at: Option<Instant>,
    consecutive_failures: u32,
}

impl StatusObserver {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            snapshot: None,
            last_success_at: None,
            consecutive_failures: 0,
        }
    }

    pub fn observe(&mut self, result: Result<DiagnosticsSnapshot, String>, observed_at: Instant) {
        match result {
            Ok(snapshot) => {
                self.snapshot = Some(snapshot);
                self.last_success_at = Some(observed_at);
                self.consecutive_failures = 0;
            }
            Err(_) => {
                self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            }
        }
    }

    #[must_use]
    pub fn envelope(&self, now: Instant) -> StatusEnvelope {
        let last_success_age_ms = self.last_success_at.map(|last_success_at| {
            u64::try_from(now.saturating_duration_since(last_success_at).as_millis())
                .unwrap_or(u64::MAX)
        });
        let state = if self.snapshot.is_none() || self.consecutive_failures >= 3 {
            PresentationState::Unavailable
        } else if self.consecutive_failures > 0
            || self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot.runtime_update_age_ms > snapshot.runtime_update_stale_after_ms
            })
        {
            PresentationState::Stale
        } else {
            PresentationState::Current
        };
        StatusEnvelope {
            schema_version: 1,
            state,
            consecutive_failures: self.consecutive_failures,
            last_success_age_ms,
            snapshot: (state != PresentationState::Unavailable)
                .then(|| self.snapshot.clone())
                .flatten(),
        }
    }
}

impl Default for StatusObserver {
    fn default() -> Self {
        Self::new()
    }
}
