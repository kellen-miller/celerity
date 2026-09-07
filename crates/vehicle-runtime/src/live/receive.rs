use std::time::Instant;

use control_core::{EnqueueResult, RawCanEvidence, RunRecord, RuntimeEvent};
use control_protocol::{DiscoveryProbe, Frame};
use vehicle_diagnostics::AcceptedRadiatorSplitCommand;

use super::{
    CONFIGURATION_RETRY_INTERVAL, DISCOVERY_RETRY_INTERVAL, LiveRuntime, map_command_source,
};

impl LiveRuntime {
    pub(crate) fn receive_powertrain(&mut self) {
        let received = match self.powertrain.receive() {
            Ok(received) => received,
            Err(error) => {
                self.record("powertrain_can_rejected", &error);
                return;
            }
        };
        if self.last_timestamp_source != Some(received.timestamp_source) {
            self.last_timestamp_source = Some(received.timestamp_source);
            self.record(
                "powertrain_timestamp_source",
                &format!("{:?}", received.timestamp_source),
            );
        }
        self.event_sequence = self.event_sequence.saturating_add(1);
        let raw_sequence = self.event_sequence;
        self.enqueue_bulk_record(RunRecord::raw_can(
            self.event_sequence,
            RawCanEvidence {
                monotonic_ns: self.monotonic_ms().saturating_mul(1_000_000),
                source: "powertrain_can",
                channel: "powertrain",
                direction: 1,
                id: received.id,
                fd: false,
                bit_rate_switch: false,
                data: received.data.clone(),
                hardware_timestamp_ns: (received.timestamp_source
                    == crate::TimestampSource::RawHardware)
                    .then_some(received.timestamp_ns),
            },
        ));
        let can_id = match u16::try_from(received.id) {
            Ok(can_id) => can_id,
            Err(error) => {
                self.record("powertrain_can_rejected", &error.to_string());
                return;
            }
        };
        let decoded = match self.powertrain_decoder.decode(can_id, &received.data) {
            Ok(decoded) => decoded,
            Err(error) => {
                self.record("powertrain_can_rejected", &format!("{error:?}"));
                return;
            }
        };
        let observed_ms = self.monotonic_ms();
        for signal in decoded.signals {
            self.signal_values.insert(
                signal.name.to_owned(),
                (signal.value, observed_ms, raw_sequence),
            );
        }
    }

    pub(crate) fn ingest_safety_snapshot(&mut self) -> Result<(), String> {
        let coolant = self.signal_values.get("coolant_temperature_c").copied();
        let iat = self.signal_values.get("air_temperature_c").copied();
        if let (Some((coolant_c, coolant_ms, _)), Some((iat_c, iat_ms, _))) = (coolant, iat) {
            let observed_ms = coolant_ms.min(iat_ms);
            if self
                .last_safety_observed_ms
                .is_some_and(|last| observed_ms <= last)
            {
                return Ok(());
            }
            self.last_safety_observed_ms = Some(observed_ms);
            self.ingest(RuntimeEvent::InputSnapshot {
                monotonic_ms: self.monotonic_ms(),
                observed_monotonic_ms: observed_ms,
                coolant_c,
                iat_c,
            })?;
        }
        Ok(())
    }

    pub(crate) fn ingest_model_snapshot(&mut self) -> Result<(), String> {
        let now_ms = self.monotonic_ms();
        let maximum_age_ms = self.timing.model_signals_stale_after_ms;
        let values = self
            .bundle
            .model_input_signals()
            .iter()
            .map(|signal| {
                self.signal_values
                    .get(signal)
                    .and_then(|(value, observed_ms, _)| {
                        (now_ms.saturating_sub(*observed_ms) <= maximum_age_ms).then_some(*value)
                    })
            })
            .collect::<Option<Vec<_>>>();
        match values {
            Some(values) => {
                let observations = self
                    .bundle
                    .model_input_signals()
                    .iter()
                    .cloned()
                    .zip(values.iter().copied())
                    .map(|(signal, value)| {
                        let (_, observed_ms, source_sequence) = self.signal_values[&signal];
                        (signal, value, observed_ms, source_sequence)
                    })
                    .collect::<Vec<_>>();
                for (signal, value, observed_ms, source_sequence) in observations {
                    self.event_sequence = self.event_sequence.saturating_add(1);
                    self.enqueue_record(RunRecord::signal(
                        self.event_sequence,
                        now_ms.saturating_mul(1_000_000),
                        &signal,
                        value,
                        self.bundle.decoder_generation(),
                        now_ms.saturating_sub(observed_ms).saturating_mul(1_000_000),
                        source_sequence,
                    ));
                }
                self.ingest(RuntimeEvent::ModelSignalSnapshot {
                    monotonic_ms: now_ms,
                    values,
                })
            }
            None => self.ingest(RuntimeEvent::ModelSignalsUnavailable {
                monotonic_ms: now_ms,
            }),
        }
    }

    pub(crate) fn reconcile_controller(&mut self) -> Result<(), String> {
        if self.configured {
            return Ok(());
        }
        let now = Instant::now();
        if self.capability_boot_session.is_some() {
            if self.configuration_sent_at.is_none_or(|sent| {
                now.saturating_duration_since(sent) >= CONFIGURATION_RETRY_INTERVAL
            }) {
                self.configure_reconciled_controller()?;
            }
        } else if self
            .last_discovery_sent_at
            .is_none_or(|sent| now.saturating_duration_since(sent) >= DISCOVERY_RETRY_INTERVAL)
        {
            self.send_discovery_probe()?;
        }
        Ok(())
    }

    pub(crate) fn send_discovery_probe(&mut self) -> Result<(), String> {
        self.discovery_sequence = self.discovery_sequence.wrapping_add(1).max(1);
        self.send_controller_frame(&Frame::DiscoveryProbe(DiscoveryProbe {
            protocol_major: control_protocol::PROTOCOL_MAJOR,
            protocol_minor: 0,
            flags: 0,
            probe_sequence: self.discovery_sequence,
        }))?;
        self.last_discovery_sent_at = Some(Instant::now());
        Ok(())
    }

    pub(crate) const fn reset_controller_reconciliation(&mut self) {
        self.clear_controller_diagnostics();
        self.boot_session = None;
        self.announced_boot_session = None;
        self.capability_boot_session = None;
        self.configured = false;
        self.configuration_sent_at = None;
    }

    pub(crate) fn receive_controller(&mut self) -> Result<(), String> {
        let received = match self.actuator.receive() {
            Ok(frame) => frame,
            Err(error) => {
                self.record("actuator_can_rejected", &error);
                self.clear_controller_diagnostics();
                self.ingest(RuntimeEvent::ControllerUnavailable {
                    monotonic_ms: self.monotonic_ms(),
                })?;
                return Ok(());
            }
        };
        self.event_sequence = self.event_sequence.saturating_add(1);
        self.enqueue_bulk_record(RunRecord::raw_can(
            self.event_sequence,
            RawCanEvidence {
                monotonic_ns: self.monotonic_ms().saturating_mul(1_000_000),
                source: "actuator_can",
                channel: "actuator",
                direction: 1,
                id: received.id,
                fd: received.fd,
                bit_rate_switch: received.bit_rate_switch,
                data: received.data,
                hardware_timestamp_ns: None,
            },
        ));
        let Some(frame) = received.decoded else {
            if let Some(reason) = received.ignored_reason {
                self.record("actuator_can_ignored", &reason);
            }
            return Ok(());
        };
        match frame {
            Frame::NodeAnnounce { node, message } => {
                let valid = node == self.controller.address
                    && message.protocol_major == control_protocol::PROTOCOL_MAJOR
                    && message.protocol_minor == 0
                    && matches!(message.lifecycle, 1 | 2)
                    && message.provisioned_identity == self.controller.identity
                    && message.capability_generation == self.controller.capability_generation
                    && (message.configuration_generation == 0
                        || message.configuration_generation
                            == self.controller.configuration_generation);
                if valid {
                    let session_changed = self
                        .boot_session
                        .is_some_and(|session| session != message.boot_session);
                    let was_configured = self.configured;
                    if session_changed {
                        self.clear_controller_diagnostics();
                        self.configuration_sent_at = None;
                    }
                    if self.writer.as_ref().is_some_and(|writer| {
                        writer.record_firmware_generation(message.firmware_generation)
                            == EnqueueResult::Degraded
                    }) {
                        self.storage_degraded = true;
                    }
                    self.boot_session = Some(message.boot_session);
                    self.announced_boot_session = Some(message.boot_session);
                    self.capability_boot_session = None;
                    self.configured = false;
                    self.configuration_sent_at = None;
                    if session_changed || was_configured {
                        self.ingest(RuntimeEvent::ControllerUnavailable {
                            monotonic_ms: self.monotonic_ms(),
                        })?;
                    }
                } else {
                    self.record("controller_announce_rejected", &format!("{message:?}"));
                    self.reset_controller_reconciliation();
                    self.ingest(RuntimeEvent::ControllerUnavailable {
                        monotonic_ms: self.monotonic_ms(),
                    })?;
                }
            }
            Frame::CapabilityReport { node, message } => {
                let valid = node == self.controller.address
                    && Some(message.boot_session) == self.announced_boot_session
                    && message.capability_generation == self.controller.capability_generation
                    && message.resource_id == self.controller.resource_id
                    && message.minimum_basis_points == self.controller.minimum_basis_points
                    && message.maximum_basis_points == self.controller.maximum_basis_points
                    && message.maximum_command_rate_hz == self.controller.maximum_command_rate_hz;
                if valid {
                    self.capability_boot_session = Some(message.boot_session);
                    self.configure_reconciled_controller()?;
                } else {
                    self.record("controller_capability_rejected", &format!("{message:?}"));
                    self.reset_controller_reconciliation();
                    self.ingest(RuntimeEvent::ControllerUnavailable {
                        monotonic_ms: self.monotonic_ms(),
                    })?;
                }
            }
            Frame::ConfigurationAck { node, message } if node == self.controller.address => {
                let accepted = Some(message.boot_session) == self.boot_session
                    && message.configuration_generation == self.controller.configuration_generation
                    && message.digest_prefix == self.controller.digest_prefix
                    && message.result == 1;
                if accepted {
                    self.configured = true;
                    self.configuration_sent_at = None;
                    self.ingest(RuntimeEvent::ControllerTruth {
                        monotonic_ms: self.monotonic_ms(),
                        boot_session: message.boot_session,
                        configuration_generation: message.configuration_generation,
                        identity_matches: true,
                    })?;
                } else {
                    self.record("controller_configuration_rejected", &format!("{message:?}"));
                    self.configured = false;
                    self.configuration_sent_at = None;
                    self.clear_controller_diagnostics();
                    self.ingest(RuntimeEvent::ControllerUnavailable {
                        monotonic_ms: self.monotonic_ms(),
                    })?;
                }
            }
            Frame::Heartbeat { node, message } if node == self.controller.address => {
                if !self.configured {
                    self.record("controller_heartbeat_ignored", &format!("{message:?}"));
                    return Ok(());
                }
                let state_consistent = match message.state_flags {
                    1 => message.current_epoch == 0 || message.current_epoch == self.epoch,
                    2 => message.current_epoch == self.epoch,
                    _ => false,
                };
                let truth_matches = Some(message.boot_session) == self.boot_session
                    && message.configuration_generation == self.controller.configuration_generation
                    && message.capability_generation == self.controller.capability_generation
                    && state_consistent;
                if !truth_matches {
                    self.reset_controller_reconciliation();
                    self.ingest(RuntimeEvent::ControllerUnavailable {
                        monotonic_ms: self.monotonic_ms(),
                    })?;
                    return Ok(());
                }
                self.ingest(RuntimeEvent::ControllerTruth {
                    monotonic_ms: self.monotonic_ms(),
                    boot_session: message.boot_session,
                    configuration_generation: message.configuration_generation,
                    identity_matches: truth_matches,
                })?;
            }
            Frame::CommandAck { node, message } if node == self.controller.address => {
                let accepted = self.last_sent_command.is_some_and(|command| {
                    Some(message.boot_session) == self.boot_session
                        && message.configuration_generation
                            == self.controller.configuration_generation
                        && message.epoch == self.epoch
                        && message.command_sequence == command.sequence
                        && message.accepted_basis_points == command.basis_points
                        && message.mode == 2
                        && message.fault_latch == 1
                        && message.result == 1
                        && message.output_state == 1
                });
                self.ingest(RuntimeEvent::CommandAcknowledged {
                    monotonic_ms: self.monotonic_ms(),
                    boot_session: message.boot_session,
                    command_sequence: message.command_sequence,
                    accepted,
                })?;
                if accepted && let Some(command) = self.last_sent_command.take() {
                    self.last_matching_command_ack_at = Some(Instant::now());
                    self.last_accepted_radiator_split_command =
                        Some(AcceptedRadiatorSplitCommand {
                            basis_points: command.basis_points,
                            source: map_command_source(command.source),
                        });
                    self.event_sequence = self.event_sequence.saturating_add(1);
                    self.enqueue_record(RunRecord::control(
                        self.event_sequence,
                        self.monotonic_ms().saturating_mul(1_000_000),
                        &serde_json::json!({
                            "schema_version": 1,
                            "state": "accepted",
                            "epoch": self.epoch,
                            "sequence": command.sequence,
                            "radiator_split_basis_points": command.basis_points,
                            "source": format!("{:?}", command.source),
                        })
                        .to_string(),
                    ));
                } else if !accepted {
                    self.last_command_ack_miss_at = Some(Instant::now());
                    self.last_sent_command = None;
                }
            }
            Frame::RuntimeLeaseAck { node, message } if node == self.controller.address => {
                if self.last_sent_runtime_lease.is_some_and(|lease| {
                    Some(message.boot_session) == self.boot_session
                        && message.configuration_generation
                            == self.controller.configuration_generation
                        && message.epoch == self.epoch
                        && message.renewal_sequence == lease.sequence
                        && message.result == 1
                }) {
                    self.last_matching_runtime_lease_ack_at = Some(Instant::now());
                }
            }
            Frame::FaultReport { node, message } if node == self.controller.address => {
                self.record("controller_fault", &format!("{message:?}"));
                if message.severity >= 3 {
                    self.clear_controller_diagnostics();
                    self.ingest(RuntimeEvent::SharedHardFault {
                        monotonic_ms: self.monotonic_ms(),
                    })?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
