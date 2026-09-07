use std::time::{Duration, Instant};

use control_core::{Completion, FeatureAuthority, RunManifest, RuntimeEvent, RuntimeOutcome};
use vehicle_diagnostics::{
    DiagnosticStatus, DiagnosticUnknownReason, DiagnosticsSnapshot, ObservedTemperature,
    RunStorageHealth,
};

use super::{COMMAND_ACK_SUMMARY_WINDOW, LiveRuntime, map_command_source};

impl LiveRuntime {
    pub(crate) fn update_diagnostics(&self) -> Result<(), String> {
        let now = Instant::now();
        let now_ms = self.monotonic_ms();
        let outcome = self.session.outcome();
        let global_authority = if outcome.hard_fault_latched {
            vehicle_diagnostics::GlobalAuthority::HardFault
        } else if outcome.final_feature_authority == FeatureAuthority::Active {
            vehicle_diagnostics::GlobalAuthority::Active
        } else {
            vehicle_diagnostics::GlobalAuthority::Fallback
        };
        let feature_authority = match outcome.final_feature_authority {
            FeatureAuthority::Fallback => vehicle_diagnostics::FeatureAuthority::Fallback,
            FeatureAuthority::Arming => vehicle_diagnostics::FeatureAuthority::Arming,
            FeatureAuthority::Active => vehicle_diagnostics::FeatureAuthority::Active,
        };
        let controller_runtime_lease_health =
            if outcome.final_feature_authority == FeatureAuthority::Fallback {
                DiagnosticStatus::Unknown(DiagnosticUnknownReason::NotExpectedInFallback)
            } else if !self.lease_renewal_expected {
                DiagnosticStatus::Unknown(DiagnosticUnknownReason::NotRenewing)
            } else if self
                .last_matching_runtime_lease_ack_at
                .is_some_and(|acknowledged_at| {
                    now.saturating_duration_since(acknowledged_at)
                        <= Duration::from_millis(u64::from(self.controller.runtime_lease_ms))
                })
            {
                DiagnosticStatus::Healthy
            } else if self
                .lease_renewal_expected_since
                .is_some_and(|expected_since| {
                    now.saturating_duration_since(expected_since)
                        > Duration::from_millis(u64::from(self.controller.runtime_lease_ms))
                })
                || self.last_sent_runtime_lease.is_some_and(|lease| {
                    now.saturating_duration_since(lease.sent_at)
                        > Duration::from_millis(u64::from(self.controller.runtime_lease_ms))
                })
            {
                DiagnosticStatus::Unhealthy
            } else {
                DiagnosticStatus::Unknown(DiagnosticUnknownReason::AwaitingEvidence)
            };
        let controller_command_ack_health =
            if outcome.final_feature_authority == FeatureAuthority::Fallback {
                DiagnosticStatus::Unknown(DiagnosticUnknownReason::NotExpectedInFallback)
            } else if self.last_command_ack_miss_at.is_some_and(|missed_at| {
                now.saturating_duration_since(missed_at) < COMMAND_ACK_SUMMARY_WINDOW
            }) {
                DiagnosticStatus::Unhealthy
            } else if self
                .last_matching_command_ack_at
                .is_some_and(|acknowledged_at| {
                    now.saturating_duration_since(acknowledged_at) < COMMAND_ACK_SUMMARY_WINDOW
                })
            {
                DiagnosticStatus::Healthy
            } else if let Some(command) = self.last_sent_command {
                if now.saturating_duration_since(command.sent_at)
                    > Duration::from_millis(u64::from(self.controller.acknowledgement_deadline_ms))
                {
                    DiagnosticStatus::Unhealthy
                } else {
                    DiagnosticStatus::Unknown(DiagnosticUnknownReason::AwaitingEvidence)
                }
            } else {
                DiagnosticStatus::Unknown(DiagnosticUnknownReason::NoOutstandingCommand)
            };
        let observed_temperature = |signal: &str| {
            self.signal_values
                .get(signal)
                .filter(|(degrees_celsius, _, _)| degrees_celsius.is_finite())
                .map(|(degrees_celsius, observed_ms, _)| ObservedTemperature {
                    degrees_celsius: *degrees_celsius,
                    observation_age_ms: now_ms.saturating_sub(*observed_ms),
                })
        };
        self.diagnostics.publish(
            DiagnosticsSnapshot {
                schema_version: 2,
                runtime_update_age_ms: 0,
                runtime_update_stale_after_ms: 0,
                global_authority,
                feature_authority,
                command_source: map_command_source(outcome.command_source),
                accepted_radiator_split_command: self.last_accepted_radiator_split_command,
                coolant_temperature: observed_temperature("coolant_temperature_c"),
                intake_air_temperature: observed_temperature("air_temperature_c"),
                controller_runtime_lease_health,
                controller_command_ack_health,
                run_storage_health: if self.storage_degraded {
                    RunStorageHealth::Degraded
                } else {
                    RunStorageHealth::Healthy
                },
            },
            now,
        )
    }

    pub(crate) const fn clear_controller_diagnostics(&mut self) {
        self.last_sent_command = None;
        self.last_accepted_radiator_split_command = None;
        self.last_matching_command_ack_at = None;
        self.last_command_ack_miss_at = None;
        self.lease_renewal_expected = false;
        self.lease_renewal_expected_since = None;
        self.last_sent_runtime_lease = None;
        self.last_matching_runtime_lease_ack_at = None;
    }

    #[must_use]
    pub fn outcome(&self) -> RuntimeOutcome {
        self.session.outcome()
    }

    #[must_use]
    pub fn model_acceptance_observed(&self) -> bool {
        self.session.outcome().accepted_model_command_observed
    }

    /// Stops lease renewal, requests fallback, and seals the exact Run.
    ///
    /// # Errors
    ///
    /// Returns an error for final CAN transmission or Run sealing failure.
    pub fn shutdown(mut self) -> Result<RunManifest, String> {
        self.clear_controller_diagnostics();
        self.ingest(RuntimeEvent::Shutdown {
            monotonic_ms: self.monotonic_ms(),
        })?;
        if let Err(error) = self.update_diagnostics() {
            eprintln!("shutdown diagnostics publication failed: {error}");
        }
        let manifest = self
            .writer
            .take()
            .ok_or_else(|| "Run writer missing at shutdown".to_owned())?
            .seal()
            .map_err(|error| error.to_string())?;
        if self.storage_degraded && manifest.completion == Completion::Complete {
            return Err("degraded Run was incorrectly sealed complete".to_owned());
        }
        Ok(manifest)
    }

    pub(crate) fn monotonic_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}
