use std::time::{Duration, Instant};

use control_core::{EnqueueResult, RawCanEvidence, RunRecord, RuntimeEffect, RuntimeEvent};
use control_protocol::{Command, Configuration, FallbackRequest, Frame, RuntimeLease};

use super::{LiveRuntime, SentCommand, SentRuntimeLease};

impl LiveRuntime {
    pub(crate) fn configure_reconciled_controller(&mut self) -> Result<(), String> {
        let boot_session = self
            .announced_boot_session
            .filter(|session| Some(*session) == self.capability_boot_session)
            .ok_or_else(|| "configuration requires announce and capability truth".to_owned())?;
        self.send_controller_frame(&Frame::Configuration {
            node: self.controller.address,
            message: Configuration {
                boot_session,
                configuration_generation: self.controller.configuration_generation,
                fallback_basis_points: self.controller.fallback_basis_points,
                pwm_endpoint_a_us: self.controller.pwm_endpoint_a_us,
                pwm_endpoint_b_us: self.controller.pwm_endpoint_b_us,
                direction: self.controller.direction,
                flags: 0,
                runtime_lease_ms: self.controller.runtime_lease_ms,
                command_lease_ms: self.controller.command_lease_ms,
                heartbeat_period_ms: self.controller.heartbeat_period_ms,
                acknowledgement_deadline_ms: self.controller.acknowledgement_deadline_ms,
                normal_slew_basis_points_per_second: self
                    .controller
                    .normal_slew_basis_points_per_second,
                protection_slew_basis_points_per_second: self
                    .controller
                    .protection_slew_basis_points_per_second,
                digest_prefix: self.controller.digest_prefix,
            },
        })?;
        self.configuration_sent_at = Some(Instant::now());
        Ok(())
    }

    pub(crate) fn ingest(&mut self, event: RuntimeEvent) -> Result<(), String> {
        self.event_sequence = self.event_sequence.saturating_add(1);
        if self.writer.as_ref().is_some_and(|writer| {
            RunRecord::runtime_event(self.event_sequence, &event).map_or(true, |record| {
                writer.enqueue(record) == EnqueueResult::Degraded
            })
        }) {
            self.storage_degraded = true;
        }
        let effects = self.session.ingest(event);
        self.execute_effects(effects)
    }

    pub(crate) fn execute_effects(&mut self, effects: Vec<RuntimeEffect>) -> Result<(), String> {
        for effect in effects {
            self.record("runtime_effect", &format!("{effect:?}"));
            match effect {
                RuntimeEffect::RenewRuntimeLease { sequence } => {
                    let boot_session = self
                        .boot_session
                        .ok_or_else(|| "lease effect without controller session".to_owned())?;
                    self.send_controller_frame(&Frame::RuntimeLease {
                        node: self.controller.address,
                        message: RuntimeLease {
                            boot_session,
                            configuration_generation: self.controller.configuration_generation,
                            epoch: self.epoch,
                            renewal_sequence: sequence,
                            validity_ms: self.controller.runtime_lease_ms,
                        },
                    })?;
                    let now = Instant::now();
                    if !self.lease_renewal_expected {
                        self.lease_renewal_expected_since = Some(now);
                    }
                    self.lease_renewal_expected = true;
                    self.last_sent_runtime_lease = Some(SentRuntimeLease {
                        sequence,
                        sent_at: now,
                    });
                }
                RuntimeEffect::SendCommand {
                    sequence,
                    radiator_split_basis_points,
                } => {
                    let source = self.session.outcome().command_source;
                    let now = Instant::now();
                    if self.last_sent_command.is_some_and(|command| {
                        now.saturating_duration_since(command.sent_at)
                            >= Duration::from_millis(u64::from(
                                self.controller.acknowledgement_deadline_ms,
                            ))
                    }) {
                        self.last_command_ack_miss_at = Some(now);
                    }
                    self.event_sequence = self.event_sequence.saturating_add(1);
                    self.enqueue_record(RunRecord::control(
                        self.event_sequence,
                        self.monotonic_ms().saturating_mul(1_000_000),
                        &serde_json::json!({
                            "schema_version": 1,
                            "state": "requested",
                            "epoch": self.epoch,
                            "sequence": sequence,
                            "radiator_split_basis_points": radiator_split_basis_points,
                            "source": format!("{source:?}"),
                        })
                        .to_string(),
                    ));
                    let boot_session = self
                        .boot_session
                        .ok_or_else(|| "command effect without controller session".to_owned())?;
                    self.send_controller_frame(&Frame::Command {
                        node: self.controller.address,
                        message: Command {
                            boot_session,
                            configuration_generation: self.controller.configuration_generation,
                            epoch: self.epoch,
                            command_sequence: sequence,
                            radiator_split_basis_points,
                            flags: 0,
                        },
                    })?;
                    self.last_sent_command = Some(SentCommand {
                        sequence,
                        basis_points: radiator_split_basis_points,
                        source,
                        sent_at: now,
                    });
                }
                RuntimeEffect::RequestFallback => {
                    self.clear_controller_diagnostics();
                    if let Some(boot_session) = self.boot_session {
                        self.fallback_sequence = self.fallback_sequence.wrapping_add(1);
                        self.send_controller_frame(&Frame::FallbackRequest {
                            node: self.controller.address,
                            message: FallbackRequest {
                                boot_session,
                                epoch: self.epoch,
                                request_sequence: self.fallback_sequence,
                            },
                        })?;
                    }
                }
                RuntimeEffect::RecordExperiment(event) => {
                    self.event_sequence = self.event_sequence.saturating_add(1);
                    self.enqueue_record(RunRecord::experiment(
                        self.event_sequence,
                        self.monotonic_ms().saturating_mul(1_000_000),
                        &event,
                    ));
                }
                RuntimeEffect::StopLeaseRenewal => {
                    self.lease_renewal_expected = false;
                    self.lease_renewal_expected_since = None;
                    self.last_sent_runtime_lease = None;
                    self.last_matching_runtime_lease_ack_at = None;
                }
                RuntimeEffect::RecordAuthority(_) | RuntimeEffect::RecordHardFault => {}
            }
        }
        Ok(())
    }

    pub(crate) fn send_controller_frame(&mut self, frame: &Frame) -> Result<(), String> {
        let mut bytes = [0_u8; 64];
        let encoded =
            control_protocol::encode(frame, &mut bytes).map_err(|error| format!("{error:?}"))?;
        self.actuator.send(frame)?;
        self.event_sequence = self.event_sequence.saturating_add(1);
        self.enqueue_bulk_record(RunRecord::raw_can(
            self.event_sequence,
            RawCanEvidence {
                monotonic_ns: self.monotonic_ms().saturating_mul(1_000_000),
                source: "actuator_can",
                channel: "actuator",
                direction: 2,
                id: u32::from(encoded.can_id),
                fd: true,
                bit_rate_switch: true,
                data: bytes[..encoded.len].to_vec(),
                hardware_timestamp_ns: None,
            },
        ));
        Ok(())
    }

    pub(crate) fn enqueue_record(&mut self, record: RunRecord) {
        if self
            .writer
            .as_ref()
            .is_some_and(|writer| writer.enqueue(record) == EnqueueResult::Degraded)
        {
            self.storage_degraded = true;
        }
    }

    pub(crate) fn enqueue_bulk_record(&mut self, record: RunRecord) {
        if self
            .writer
            .as_ref()
            .is_some_and(|writer| writer.enqueue_bulk(record) == EnqueueResult::Degraded)
        {
            self.storage_degraded = true;
        }
    }

    pub(crate) fn record(&mut self, source: &str, payload: &str) {
        self.event_sequence = self.event_sequence.saturating_add(1);
        if self.writer.as_ref().is_some_and(|writer| {
            writer.enqueue(RunRecord::evidence(
                self.event_sequence,
                self.monotonic_ms().saturating_mul(1_000_000),
                source,
                payload,
            )) == EnqueueResult::Degraded
        }) {
            self.storage_degraded = true;
        }
    }
}
