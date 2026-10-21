use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use control_core::{
    CommandSource, ControllerRuntimeConfiguration, PowertrainDecoder, RunContext, RunWriter,
    RuntimeEvent, RuntimeModel, RuntimeSession, StartupMode, TimingModel, ValidatedBundle,
};
use nix::errno::Errno;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use vehicle_diagnostics::{AcceptedRadiatorSplitCommand, DiagnosticsStore};

use crate::{ActuatorCanTransport, PowertrainCanReceiver};

const COMMAND_ACK_SUMMARY_WINDOW: Duration = Duration::from_secs(2);
const DISCOVERY_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const CONFIGURATION_RETRY_INTERVAL: Duration = Duration::from_millis(100);

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
    timing: TimingModel,
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
    last_discovery_sent_at: Option<Instant>,
    configuration_sent_at: Option<Instant>,
    fallback_sequence: u32,
    signal_values: BTreeMap<String, (f64, u64, u64)>,
    last_timestamp_source: Option<crate::TimestampSource>,
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
        let timing = bundle.timing_model();
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
            timing,
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
            discovery_sequence: 0,
            last_discovery_sent_at: None,
            configuration_sent_at: None,
            fallback_sequence: 0,
            signal_values: BTreeMap::new(),
            last_timestamp_source: None,
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
        runtime.send_discovery_probe()?;
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
        self.reconcile_controller()?;
        let cycle_started = Instant::now();
        let deadline = cycle_started + Duration::from_millis(self.timing.cycle_ms);
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
        self.reconcile_controller()?;
        self.update_diagnostics()
    }

    fn poll_once(&mut self, timeout: PollTimeout) -> Result<bool, String> {
        let (powertrain_ready, actuator_ready) = {
            let mut descriptors = [
                PollFd::new(self.powertrain.borrowed_fd(), PollFlags::POLLIN),
                PollFd::new(self.actuator.borrowed_fd(), PollFlags::POLLIN),
            ];
            match poll(&mut descriptors, timeout) {
                Ok(0) | Err(Errno::EINTR) => return Ok(false),
                Ok(_) => {}
                Err(error) => return Err(error.to_string()),
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
}

mod control;
mod diagnostics;
mod receive;

const fn map_command_source(source: CommandSource) -> vehicle_diagnostics::CommandSource {
    match source {
        CommandSource::ControllerLocalFallback => {
            vehicle_diagnostics::CommandSource::ControllerLocalFallback
        }
        CommandSource::Deterministic => vehicle_diagnostics::CommandSource::Deterministic,
        CommandSource::ModelOptimized => vehicle_diagnostics::CommandSource::ModelOptimized,
        CommandSource::Experiment => vehicle_diagnostics::CommandSource::Experiment,
    }
}
