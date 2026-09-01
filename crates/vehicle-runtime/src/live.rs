use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use control_core::{
    CommandSource, Completion, ControllerRuntimeConfiguration, EnqueueResult, FeatureAuthority,
    PowertrainDecoder, RawCanEvidence, RunContext, RunManifest, RunRecord, RunWriter,
    RuntimeEffect, RuntimeEvent, RuntimeModel, RuntimeOutcome, RuntimeSession, StartupMode,
    ValidatedBundle,
};
use control_protocol::{
    Command, Configuration, DiscoveryProbe, FallbackRequest, Frame, RuntimeLease,
};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use vehicle_diagnostics::{
    AcceptedRadiatorSplitCommand, DiagnosticStatus, DiagnosticUnknownReason, DiagnosticsSnapshot,
    DiagnosticsStore, ObservedTemperature, RunStorageHealth,
};

use crate::{ActuatorCanTransport, PowertrainCanReceiver};

const COMMAND_ACK_SUMMARY_WINDOW: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct SentCommand {
    sequence: u32,
    basis_points: u16,
    source: CommandSource,
    sent_at: Instant,
}

#[derive(Clone, Copy)]
struct SentRuntimeLease {
    sequence: u32,
    sent_at: Instant,
}

pub struct LiveRuntime {
    bundle: ValidatedBundle,
    controller: ControllerRuntimeConfiguration,
    powertrain: PowertrainCanReceiver,
    powertrain_decoder: PowertrainDecoder,
    actuator: ActuatorCanTransport,
    session: RuntimeSession,
    writer: Option<RunWriter>,
    diagnostics: DiagnosticsStore,
    started: Instant,
    epoch: u64,
    boot_session: Option<u32>,
    announced_boot_session: Option<u32>,
    capability_boot_session: Option<u32>,
    configured: bool,
    event_sequence: u64,
    discovery_sequence: u32,
    fallback_sequence: u32,
    signal_values: BTreeMap<String, (f64, u64, u64)>,
    last_safety_observed_ms: Option<u64>,
    storage_degraded: bool,
    last_sent_command: Option<SentCommand>,
    last_accepted_radiator_split_command: Option<AcceptedRadiatorSplitCommand>,
    last_matching_command_ack_at: Option<Instant>,
    last_command_ack_miss_at: Option<Instant>,
    lease_renewal_expected: bool,
    lease_renewal_expected_since: Option<Instant>,
    last_sent_runtime_lease: Option<SentRuntimeLease>,
    last_matching_runtime_lease_ack_at: Option<Instant>,
}

struct LiveCanBoundaries {
    powertrain: PowertrainCanReceiver,
    actuator: ActuatorCanTransport,
}

impl LiveRuntime {
    /// Opens the strictly qualified physical live composition. This is the
    /// only constructor used by `celerityd` for authority.
    ///
    /// # Errors
    ///
    /// Returns an error for non-live configuration, netdevice qualification,
    /// socket binding, or Run writer startup.
    pub fn open(
        bundle: ValidatedBundle,
        model: Option<RuntimeModel>,
        model_bundle_digest: Option<&str>,
        epoch: u64,
        run_id: &str,
        diagnostics: DiagnosticsStore,
    ) -> Result<Self, String> {
        if bundle.mode() != StartupMode::Live {
            return Err("celerityd authority requires mode=live".to_owned());
        }
        let (powertrain_interface, actuator_interface) = bundle
            .live_interfaces()
            .ok_or_else(|| "live interfaces are absent".to_owned())?;
        let boundaries = LiveCanBoundaries {
            powertrain: PowertrainCanReceiver::open(powertrain_interface, actuator_interface)?,
            actuator: ActuatorCanTransport::open(actuator_interface, powertrain_interface)?,
        };
        Self::from_boundaries(
            bundle,
            model,
            model_bundle_digest,
            epoch,
            run_id,
            diagnostics,
            boundaries,
        )
    }

    /// Binds the same production loop to kernel vCAN devices for hardware-free
    /// Linux integration. Only names beginning with `vcan-` are accepted, so
    /// this path cannot bypass physical live netdevice qualification.
    ///
    /// # Errors
    ///
    /// Returns an error for non-live configuration, non-vCAN names, socket
    /// binding, or Run writer startup.
    pub fn open_virtual_hardware_free(
        bundle: ValidatedBundle,
        model: Option<RuntimeModel>,
        model_bundle_digest: Option<&str>,
        epoch: u64,
        run_id: &str,
        diagnostics: DiagnosticsStore,
    ) -> Result<Self, String> {
        if bundle.mode() != StartupMode::Live {
            return Err("virtual integration still requires a live bundle".to_owned());
        }
        let (powertrain_interface, actuator_interface) = bundle
            .live_interfaces()
            .ok_or_else(|| "live interfaces are absent".to_owned())?;
        if !powertrain_interface.starts_with("vcan-") || !actuator_interface.starts_with("vcan-") {
            return Err("hardware-free composition requires explicit vcan-* interfaces".to_owned());
        }
        let boundaries = LiveCanBoundaries {
            powertrain: PowertrainCanReceiver::bind_prequalified(powertrain_interface)?,
            actuator: ActuatorCanTransport::bind_prequalified(actuator_interface)?,
        };
        Self::from_boundaries(
            bundle,
            model,
            model_bundle_digest,
            epoch,
            run_id,
            diagnostics,
            boundaries,
        )
    }

    fn from_boundaries(
        bundle: ValidatedBundle,
        model: Option<RuntimeModel>,
        model_bundle_digest: Option<&str>,
        epoch: u64,
        run_id: &str,
        diagnostics: DiagnosticsStore,
        boundaries: LiveCanBoundaries,
    ) -> Result<Self, String> {
        if epoch == 0 {
            return Err("runtime epoch zero is reserved".to_owned());
        }
        let controller = bundle.controller_runtime_configuration();
        let writer = RunWriter::start_with_context(
            bundle.run_storage_root(),
            run_id,
            1024,
            bundle.minimum_run_free_bytes(),
            RunContext {
                configuration_generation: bundle.generation(),
                configuration_sha256: bundle.configuration_sha256().to_owned(),
                model_bundle_digest: model_bundle_digest.map(str::to_owned),
                protocol_major: control_protocol::PROTOCOL_MAJOR,
                firmware_generation: 0,
                decoder_generation: bundle.decoder_generation(),
                model_abi: bundle.model_abi().to_owned(),
                model_input_signals: bundle.model_input_signals().to_vec(),
                model_history_length: bundle.model_history_length(),
                sample_period_ms: bundle.cycle_ms(),
                command_lattice: bundle.model_command_lattice().to_vec(),
                maximum_calibration_error: bundle.model_maximum_calibration_error(),
            },
        )
        .map_err(|error| error.to_string())?;
        let session = RuntimeSession::start(bundle.clone(), model);
        let powertrain_decoder = bundle.powertrain_decoder();
        let mut runtime = Self {
            bundle,
            controller,
            powertrain: boundaries.powertrain,
            powertrain_decoder,
            actuator: boundaries.actuator,
            session,
            writer: Some(writer),
            diagnostics,
            started: Instant::now(),
            epoch,
            boot_session: None,
            announced_boot_session: None,
            capability_boot_session: None,
            configured: false,
            event_sequence: 0,
            discovery_sequence: 1,
            fallback_sequence: 0,
            signal_values: BTreeMap::new(),
            last_safety_observed_ms: None,
            storage_degraded: false,
            last_sent_command: None,
            last_accepted_radiator_split_command: None,
            last_matching_command_ack_at: None,
            last_command_ack_miss_at: None,
            lease_renewal_expected: false,
            lease_renewal_expected_since: None,
            last_sent_runtime_lease: None,
            last_matching_runtime_lease_ack_at: None,
        };
        runtime.send_controller_frame(&Frame::DiscoveryProbe(DiscoveryProbe {
            protocol_major: control_protocol::PROTOCOL_MAJOR,
            protocol_minor: 0,
            flags: 0,
            probe_sequence: runtime.discovery_sequence,
        }))?;
        runtime.record("runtime", "startup in controller-local fallback");
        Ok(runtime)
    }

    /// Services both CAN receive paths without blocking either one, executes
    /// exactly one control cycle, then services acknowledgements until the
    /// monotonic cycle deadline.
    ///
    /// # Errors
    ///
    /// Returns an error for poll, receive, protocol, transmit, or diagnostics
    /// state failures.
    pub fn run_cycle(&mut self) -> Result<(), String> {
        let cycle_started = Instant::now();
        let deadline = cycle_started + Duration::from_millis(self.bundle.cycle_ms());
        for _ in 0..64 {
            if Instant::now() >= deadline || !self.poll_once(PollTimeout::ZERO)? {
                break;
            }
        }

        let now = Instant::now();
        let remaining_cycle_ns =
            u64::try_from(deadline.saturating_duration_since(now).as_nanos()).unwrap_or(u64::MAX);
        self.ingest_safety_snapshot()?;
        self.ingest_model_snapshot()?;
        let event = RuntimeEvent::Cycle {
            monotonic_ms: self.monotonic_ms(),
            remaining_cycle_ns,
        };
        self.ingest(event)?;

        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let timeout = PollTimeout::try_from(remaining).unwrap_or(PollTimeout::MAX);
            if !self.poll_once(timeout)? {
                break;
            }
        }
        self.update_diagnostics()
    }

    fn poll_once(&mut self, timeout: PollTimeout) -> Result<bool, String> {
        let (powertrain_ready, actuator_ready) = {
            let mut descriptors = [
                PollFd::new(self.powertrain.borrowed_fd(), PollFlags::POLLIN),
                PollFd::new(self.actuator.borrowed_fd(), PollFlags::POLLIN),
            ];
            if poll(&mut descriptors, timeout).map_err(|error| error.to_string())? == 0 {
                return Ok(false);
            }
            (
                descriptors[0]
                    .revents()
                    .is_some_and(|events| events.contains(PollFlags::POLLIN)),
                descriptors[1]
                    .revents()
                    .is_some_and(|events| events.contains(PollFlags::POLLIN)),
            )
        };
        if powertrain_ready {
            self.receive_powertrain();
        }
        if actuator_ready {
            self.receive_controller()?;
        }
        Ok(powertrain_ready || actuator_ready)
    }

    fn receive_powertrain(&mut self) {
        let received = match self.powertrain.receive() {
            Ok(received) => received,
            Err(error) => {
                self.record("powertrain_can_rejected", &error);
                return;
            }
        };
        self.record(
            "powertrain_timestamp_source",
            &format!("{:?}", received.timestamp_source),
        );
        self.event_sequence = self.event_sequence.saturating_add(1);
        let raw_sequence = self.event_sequence;
        self.enqueue_record(RunRecord::raw_can(
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

    fn ingest_safety_snapshot(&mut self) -> Result<(), String> {
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

    fn ingest_model_snapshot(&mut self) -> Result<(), String> {
        let now_ms = self.monotonic_ms();
        let maximum_age_ms = self.bundle.cycle_ms().saturating_mul(2);
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

    fn receive_controller(&mut self) -> Result<(), String> {
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
        self.enqueue_record(RunRecord::raw_can(
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
        let frame = received.decoded;
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
                    if self.boot_session != Some(message.boot_session) {
                        self.clear_controller_diagnostics();
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
                } else {
                    self.record("controller_announce_rejected", &format!("{message:?}"));
                    self.clear_controller_diagnostics();
                    self.boot_session = None;
                    self.announced_boot_session = None;
                    self.capability_boot_session = None;
                    self.configured = false;
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
                    self.clear_controller_diagnostics();
                    self.ingest(RuntimeEvent::ControllerUnavailable {
                        monotonic_ms: self.monotonic_ms(),
                    })?;
                }
            }
            Frame::ConfigurationAck { node, message }
                if node == self.controller.address
                    && Some(message.boot_session) == self.boot_session
                    && message.configuration_generation
                        == self.controller.configuration_generation
                    && message.digest_prefix == self.controller.digest_prefix
                    && message.result == 1 =>
            {
                self.configured = true;
                self.ingest(RuntimeEvent::ControllerTruth {
                    monotonic_ms: self.monotonic_ms(),
                    boot_session: message.boot_session,
                    configuration_generation: message.configuration_generation,
                    identity_matches: true,
                })?;
            }
            Frame::Heartbeat { node, message }
                if node == self.controller.address && self.configured =>
            {
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
                    self.clear_controller_diagnostics();
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
            Frame::FaultReport { node, .. } if node == self.controller.address => {
                self.clear_controller_diagnostics();
                self.ingest(RuntimeEvent::SharedHardFault {
                    monotonic_ms: self.monotonic_ms(),
                })?;
            }
            _ => {}
        }
        Ok(())
    }

    fn configure_reconciled_controller(&mut self) -> Result<(), String> {
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
        })
    }

    fn ingest(&mut self, event: RuntimeEvent) -> Result<(), String> {
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

    fn execute_effects(&mut self, effects: Vec<RuntimeEffect>) -> Result<(), String> {
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

    fn send_controller_frame(&mut self, frame: &Frame) -> Result<(), String> {
        let mut bytes = [0_u8; 64];
        let encoded =
            control_protocol::encode(frame, &mut bytes).map_err(|error| format!("{error:?}"))?;
        self.actuator.send(frame)?;
        self.event_sequence = self.event_sequence.saturating_add(1);
        self.enqueue_record(RunRecord::raw_can(
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

    fn enqueue_record(&mut self, record: RunRecord) {
        if self
            .writer
            .as_ref()
            .is_some_and(|writer| writer.enqueue(record) == EnqueueResult::Degraded)
        {
            self.storage_degraded = true;
        }
    }

    fn record(&mut self, source: &str, payload: &str) {
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

    fn update_diagnostics(&self) -> Result<(), String> {
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

    fn clear_controller_diagnostics(&mut self) {
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

    fn monotonic_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

fn map_command_source(source: CommandSource) -> vehicle_diagnostics::CommandSource {
    match source {
        CommandSource::ControllerLocalFallback => {
            vehicle_diagnostics::CommandSource::ControllerLocalFallback
        }
        CommandSource::Deterministic => vehicle_diagnostics::CommandSource::Deterministic,
        CommandSource::ModelOptimized => vehicle_diagnostics::CommandSource::ModelOptimized,
        CommandSource::Experiment => vehicle_diagnostics::CommandSource::Experiment,
    }
}
