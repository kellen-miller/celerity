#![cfg(target_os = "linux")]

use std::{
    error::Error,
    fs, io,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use celerity_proto::celerity::v1::{
    ModelBundleManifest, ModelInputRange, ModelNormalization, RunEvent, run_event::Payload,
};
use control_core::{
    CommandSource, Completion, ControllerEmulator, EmulatorProvisioning, RuntimeModel, StartupMode, ValidatedBundle,
    replay_events,
};
use control_protocol::{Frame, decode, encode};
use prost::Message;
use socketcan::{CanFdFrame, CanFdSocket, CanFrame, CanSocket, EmbeddedFrame, Frame as SocketCanFrame, Socket};
use vehicle_diagnostics::{
    CommandSource as DiagnosticCommandSource, DiagnosticStatus, DiagnosticUnknownReason, DiagnosticsSnapshot,
    DiagnosticsStore, RunStorageHealth,
};

type LiveTestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[test]
fn live_kernel_sockets_drive_broadcast_and_nexus_and_seal_runs() -> LiveTestResult<()> {
    for nexus in [false, true] {
        run_live_case(nexus)?;
    }

    Ok(())
}

fn run_live_case(nexus: bool) -> LiveTestResult<()> {
    let temporary = tempfile::tempdir()?;
    let bundle_path = temporary.path().join("live.toml");
    let runs = temporary.path().join("runs");
    let slots = temporary.path().join("models");
    let runs_root = runs.to_str().ok_or_else(|| io::Error::other("run path is not UTF-8"))?;
    let slots_root = slots
        .to_str()
        .ok_or_else(|| io::Error::other("slot path is not UTF-8"))?;
    fs::write(&bundle_path, live_bundle(runs_root, slots_root, nexus))?;
    let bundle = ValidatedBundle::load(&bundle_path, StartupMode::Live)
        .map_err(|error| io::Error::other(format!("valid live bundle: {error:?}")))?;
    let model_directory = temporary.path().join("model");
    fs::create_dir_all(&model_directory)?;
    let model_path = model_directory.join("identity.onnx");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/models/identity.onnx"),
        &model_path,
    )?;
    fs::write(
        model_directory.join("manifest.pb"),
        ModelBundleManifest {
            input_ranges: vec![
                ModelInputRange {
                    minimum: 60.0,
                    maximum: 120.0,
                },
                ModelInputRange {
                    minimum: 0.0,
                    maximum: 100.0,
                },
            ],
            normalization: vec![
                ModelNormalization {
                    mean: 90.0,
                    scale: 10.0,
                },
                ModelNormalization {
                    mean: 40.0,
                    scale: 10.0,
                },
            ],
            calibration_error: 0.01,
            ..ModelBundleManifest::default()
        }
        .encode_to_vec(),
    )?;
    let model = RuntimeModel::load(&model_path, &bundle)
        .map_err(|error| io::Error::other(format!("production model: {error:?}")))?;
    let diagnostics = DiagnosticsStore::new(DiagnosticsSnapshot::startup_fallback(), 20, std::time::Instant::now());
    let diagnostics_socket = temporary.path().join("runtime-diagnostics.sock");
    let observed_commands = Arc::new(Mutex::new(Vec::new()));
    let mut workers = LiveWorkers::start(&diagnostics_socket, diagnostics.clone(), Arc::clone(&observed_commands))?;
    let startup = vehicle_diagnostics::read_diagnostics(&diagnostics_socket).map_err(io::Error::other)?;
    verify_startup_diagnostics(&startup);

    let mut runtime = match vehicle_runtime::LiveRuntime::open_virtual_hardware_free(
        bundle,
        Some(model),
        Some("fixture-model-bundle"),
        99,
        "live-kernel",
        diagnostics,
    ) {
        Ok(runtime) => runtime,
        Err(error) => {
            workers.stop()?;

            return Err(io::Error::other(error).into());
        }
    };
    let exercise_result = exercise_live_runtime(&mut runtime, nexus, &diagnostics_socket, &observed_commands);
    let manifest_result = runtime.shutdown().map_err(io::Error::other);
    let stopped_result = vehicle_diagnostics::read_diagnostics(&diagnostics_socket).map_err(io::Error::other);
    let workers_result = workers.stop();
    exercise_result?;
    let manifest = manifest_result?;
    let stopped = stopped_result?;
    workers_result?;
    verify_shutdown(&stopped, &manifest);
    verify_recorded_run(&runs, &manifest)?;
    if nexus {
        verify_nexus_recording(&runs, &manifest)?;
    }

    Ok(())
}

fn verify_startup_diagnostics(startup: &DiagnosticsSnapshot) {
    assert!(startup.accepted_radiator_split_command.is_none());
    assert!(startup.coolant_temperature.is_none());
    assert_eq!(
        startup.controller_runtime_lease_health,
        DiagnosticStatus::Unknown(DiagnosticUnknownReason::NotExpectedInFallback)
    );
    assert_eq!(
        startup.controller_command_ack_health,
        DiagnosticStatus::Unknown(DiagnosticUnknownReason::NoOutstandingCommand)
    );
}

fn exercise_live_runtime(
    runtime: &mut vehicle_runtime::LiveRuntime,
    nexus: bool,
    diagnostics_socket: &Path,
    observed_commands: &Mutex<Vec<u16>>,
) -> LiveTestResult<()> {
    for _ in 0..4 {
        send_powertrain_temperature(100, 40, nexus)?;
        runtime.run_cycle().map_err(io::Error::other)?;
    }

    if nexus {
        send_powertrain_frame(0x601, &[0x04, 0xe2, 0xff, 0xce, 0x08, 0x98, 0x07, 0x08])?;
        send_powertrain_temperature(20, 0, false)?;
        runtime.run_cycle().map_err(io::Error::other)?;
    }

    let outcome = runtime.outcome();
    assert_eq!(outcome.command_source, CommandSource::ModelOptimized);
    assert_eq!(outcome.accepted_basis_points, Some(9_000));
    assert!(
        observed_commands
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?
            .contains(&9_000)
    );
    let snapshot = vehicle_diagnostics::read_diagnostics(diagnostics_socket).map_err(io::Error::other)?;
    assert_eq!(snapshot.command_source, DiagnosticCommandSource::ModelOptimized);
    assert_eq!(
        snapshot
            .accepted_radiator_split_command
            .ok_or_else(|| io::Error::other("accepted command missing"))?
            .basis_points,
        9_000
    );
    assert_eq!(snapshot.controller_runtime_lease_health, DiagnosticStatus::Healthy);
    assert_eq!(snapshot.controller_command_ack_health, DiagnosticStatus::Healthy);
    assert_eq!(snapshot.run_storage_health, RunStorageHealth::Healthy);
    let coolant = snapshot
        .coolant_temperature
        .ok_or_else(|| io::Error::other("finite coolant temperature missing"))?
        .degrees_celsius;
    assert!((coolant - 100.0).abs() < f64::EPSILON);
    thread::sleep(Duration::from_millis(120));
    let delayed = vehicle_diagnostics::read_diagnostics(diagnostics_socket).map_err(io::Error::other)?;
    assert!(delayed.runtime_update_age_ms >= delayed.runtime_update_stale_after_ms);
    if nexus {
        exercise_nexus_fallback(runtime, diagnostics_socket)?;
    }

    Ok(())
}

fn exercise_nexus_fallback(
    runtime: &mut vehicle_runtime::LiveRuntime,
    diagnostics_socket: &Path,
) -> LiveTestResult<()> {
    for _ in 0..15 {
        send_powertrain_frame(0x600, &[0x80, 0, 0, 0, 0, 0, 0, 0])?;
        runtime.run_cycle().map_err(io::Error::other)?;
    }

    assert_eq!(runtime.outcome().command_source, CommandSource::ControllerLocalFallback);
    let expired = vehicle_diagnostics::read_diagnostics(diagnostics_socket).map_err(io::Error::other)?;
    let temperature = expired
        .coolant_temperature
        .ok_or_else(|| io::Error::other("last valid coolant temperature missing"))?;
    assert!((temperature.degrees_celsius - 100.0).abs() < f64::EPSILON);
    assert!(temperature.observation_age_ms >= 250);
    assert!(
        expired
            .intake_air_temperature
            .ok_or_else(|| io::Error::other("last valid outlet temperature missing"))?
            .observation_age_ms
            >= 250
    );

    Ok(())
}

fn verify_shutdown(stopped: &DiagnosticsSnapshot, manifest: &control_core::RunManifest) {
    assert!(stopped.accepted_radiator_split_command.is_none());
    assert_eq!(
        stopped.controller_runtime_lease_health,
        DiagnosticStatus::Unknown(DiagnosticUnknownReason::NotExpectedInFallback)
    );
    assert_eq!(manifest.completion, Completion::Complete);
}

fn verify_recorded_run(runs: &Path, manifest: &control_core::RunManifest) -> LiveTestResult<()> {
    let run_directory = runs.join("live-kernel");
    assert!(run_directory.join("manifest.pb").is_file());
    assert!(run_directory.join(&manifest.chunk_file).is_file());
    let replayed = replay_events(&run_directory).map_err(|error| io::Error::other(format!("{error:?}")))?;
    assert!(replayed.len() >= 4, "live Run must contain runtime events");

    Ok(())
}

fn verify_nexus_recording(runs: &Path, manifest: &control_core::RunManifest) -> LiveTestResult<()> {
    let bytes = fs::read(runs.join("live-kernel").join(&manifest.chunk_file))?;
    let mut input = bytes.as_slice();
    let mut raw_sequences = Vec::new();
    let mut supplementary = None;
    while !input.is_empty() {
        let event = RunEvent::decode_length_delimited(&mut input)?;
        match event.payload {
            Some(Payload::RawCan(raw)) if raw.id == 0x601 => {
                raw_sequences.push(event.sequence);
            }
            Some(Payload::SignalObservation(signal)) if signal.signal == "ambient_air_temperature_c" => {
                assert_eq!(event.source, "decoded_signal");
                supplementary = Some(signal);
            }
            _ => {}
        }
    }

    let signal = supplementary.ok_or_else(|| io::Error::other("supplementary channel missing from Run"))?;
    assert!((signal.value + 5.0).abs() < f64::EPSILON);
    assert_eq!(signal.age_ns, 0);
    assert!(raw_sequences.contains(&signal.source_event_sequence));

    Ok(())
}

struct LiveWorkers {
    stopping: Arc<AtomicBool>,
    controller: Option<JoinHandle<LiveTestResult<()>>>,
    diagnostics: Option<JoinHandle<Result<(), String>>>,
}

impl LiveWorkers {
    fn start(
        diagnostics_socket: &Path,
        diagnostics_store: DiagnosticsStore,
        observed_commands: Arc<Mutex<Vec<u16>>>,
    ) -> LiveTestResult<Self> {
        let stopping = Arc::new(AtomicBool::new(false));
        let diagnostics =
            vehicle_diagnostics::serve_diagnostics(diagnostics_socket, Arc::clone(&stopping), diagnostics_store)?;
        let mut workers = Self {
            stopping: Arc::clone(&stopping),
            controller: None,
            diagnostics: Some(diagnostics),
        };
        workers.controller = Some(spawn_controller(stopping, observed_commands)?);

        Ok(workers)
    }

    fn stop(&mut self) -> LiveTestResult<()> {
        self.stopping.store(true, Ordering::Relaxed);
        let wakeup_result = if self.controller.is_some() {
            send_controller_wakeup()
        } else {
            Ok(())
        };
        let controller_result = self
            .controller
            .take()
            .map(JoinHandle::join)
            .transpose()
            .map_err(|_| io::Error::other("controller thread panicked"));
        let diagnostics_result = self
            .diagnostics
            .take()
            .map(JoinHandle::join)
            .transpose()
            .map_err(|_| io::Error::other("diagnostics thread panicked"));
        wakeup_result?;
        if let Some(result) = controller_result? {
            result?;
        }
        if let Some(result) = diagnostics_result? {
            result.map_err(io::Error::other)?;
        }

        Ok(())
    }
}

impl Drop for LiveWorkers {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("live composition worker cleanup failed: {error}");
        }
    }
}

fn spawn_controller(
    stopping: Arc<AtomicBool>,
    observed_commands: Arc<Mutex<Vec<u16>>>,
) -> io::Result<JoinHandle<LiveTestResult<()>>> {
    thread::Builder::new()
        .name("celerity-test-controller".to_owned())
        .spawn(move || run_controller(stopping, observed_commands))
}

fn run_controller(stopping: Arc<AtomicBool>, observed_commands: Arc<Mutex<Vec<u16>>>) -> LiveTestResult<()> {
    let socket = CanFdSocket::open("vcan-controller")?;
    socket.set_read_timeout(Duration::from_millis(100))?;
    let mut emulator = ControllerEmulator::new(EmulatorProvisioning {
        node: 1,
        identity: 1,
        boot_session: 7,
        firmware_generation: 1,
        capability_generation: 1,
        fallback_basis_points: 9_000,
    });
    let started = std::time::Instant::now();
    while !stopping.load(Ordering::Relaxed) {
        let frame = match socket.read_frame() {
            Ok(frame) => frame,
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let can_id = u16::try_from(frame.raw_id())?;
        let decoded = decode(can_id, frame.data()).map_err(|error| io::Error::other(format!("{error:?}")))?;
        if let Frame::Command { message, .. } = &decoded {
            observed_commands
                .lock()
                .map_err(|error| io::Error::other(error.to_string()))?
                .push(message.radiator_split_basis_points);
        }

        let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let mut response = emulator
            .ingest(now_ms, can_id, frame.data())
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        while let Some(current) = response {
            write_fd(&socket, current.can_id, current.payload())?;
            response = current.follow_up().cloned();
        }
        if let Some(heartbeat) = emulator
            .heartbeat(now_ms)
            .map_err(|error| io::Error::other(format!("{error:?}")))?
        {
            write_fd(&socket, heartbeat.can_id, heartbeat.payload())?;
        }
    }

    Ok(())
}

fn write_fd(socket: &CanFdSocket, can_id: u16, payload: &[u8]) -> LiveTestResult<()> {
    let id = socketcan::StandardId::new(can_id).ok_or_else(|| io::Error::other("invalid standard CAN id"))?;
    let frame = CanFdFrame::new(id, payload).ok_or_else(|| io::Error::other("invalid CAN FD frame"))?;
    socket.write_frame(&frame)?;

    Ok(())
}

fn send_powertrain_temperature(coolant_c: i16, iat_c: i16, nexus: bool) -> LiveTestResult<()> {
    let (id, coolant, iat) = if nexus {
        (0x600, (coolant_c * 10).to_be_bytes(), (iat_c * 10).to_be_bytes())
    } else {
        let coolant = u16::try_from(i32::from(coolant_c) * 10 + 2_731)?.to_be_bytes();
        let iat = u16::try_from(i32::from(iat_c) * 10 + 2_731)?.to_be_bytes();
        (0x3e0, coolant, iat)
    };
    send_powertrain_frame(id, &[coolant[0], coolant[1], iat[0], iat[1], 0, 0, 0, 0])
}

fn send_powertrain_frame(can_id: u16, payload: &[u8]) -> LiveTestResult<()> {
    let socket = CanSocket::open("vcan-powertrain")?;
    let id = socketcan::StandardId::new(can_id).ok_or_else(|| io::Error::other("invalid standard CAN id"))?;
    let frame = CanFrame::new(id, payload).ok_or_else(|| io::Error::other("invalid Classical CAN frame"))?;
    socket.write_frame(&frame)?;

    Ok(())
}

fn send_controller_wakeup() -> LiveTestResult<()> {
    let socket = CanFdSocket::open("vcan-controller")?;
    let mut payload = [0_u8; 64];
    let encoded = encode(
        &Frame::DiscoveryProbe(control_protocol::DiscoveryProbe {
            protocol_major: 1,
            protocol_minor: 0,
            flags: 0,
            probe_sequence: 0,
        }),
        &mut payload,
    )
    .map_err(|error| io::Error::other(format!("{error:?}")))?;
    write_fd(&socket, encoded.can_id, &payload[..encoded.len])?;
    thread::sleep(Duration::from_millis(10));

    Ok(())
}

fn live_bundle(run_root: &str, slots_root: &str, nexus: bool) -> String {
    let haltech = if nexus {
        "mode = \"nexus-gcan-v1\"\nbase_id = 1536\nmaximum_period_ms = 100"
    } else {
        "mode = \"broadcast-v2\""
    };
    format!(
        r#"schema_version = 1
generation = 1
mode = "live"

[runtime]
cycle_ms = 20
input_stale_after_ms = 250
model_signals_stale_after_ms = 250
controller_truth_stale_after_ms = 80

[powertrain]
decoder_generation = 1

[powertrain.haltech]
{haltech}

[powertrain.cantcu]
mode = "disabled"

[controllers.duct]
address = 1
identity = 1
configuration_generation = 1
capability_generation = 1
resource_id = 1
minimum_basis_points = 0
maximum_basis_points = 10000
maximum_command_rate_hz = 50
fallback_basis_points = 9000
pwm_endpoint_a_us = 1000
pwm_endpoint_b_us = 2000
direction = 1
runtime_lease_ms = 100
command_lease_ms = 50
heartbeat_period_ms = 20
acknowledgement_deadline_ms = 25
normal_slew_basis_points_per_second = 1000
protection_slew_basis_points_per_second = 2000
digest_prefix = 1

[duct]
radiator_split_minimum = 0.1
radiator_split_maximum = 0.9
coolant_breakpoints_c = [80.0, 95.0, 105.0]
iat_breakpoints_c = [30.0, 50.0, 70.0]
policy_basis_points = [[1000, 2000, 3000], [4000, 5000, 6000], [7000, 8000, 9000]]

[model]
required = true
slots_root = "{slots_root}"
abi = "thermal-v1"
input_signals = ["coolant_temperature_c", "air_temperature_c"]
history_length = 1
warmup_iterations = 1
maximum_uncertainty = 0.1
maximum_ood_score = 0.1
required_post_work_ns = 1
hard_coolant_ceiling_c = 110.0
coolant_target_c = 80.0
coolant_input_minimum_c = 60.0
coolant_input_maximum_c = 120.0
iat_input_minimum_c = 0.0
iat_input_maximum_c = 100.0
command_lattice = [1000, 5000, 9000]

[run_storage]
root = "{run_root}"
minimum_free_bytes = 1

[diagnostics]
socket = "{run_root}/diagnostics.sock"

[sync]
spool_root = "{run_root}"
retention_count = 2
incomplete_retention_count = 2
home_interface = "home0"
expected_default_gateway = "192.0.2.1"
home_api_url = "https://home.invalid"
credential_path = "{run_root}/home-token"

[composition]
kind = "live"
powertrain_interface = "vcan-powertrain"
actuator_interface = "vcan-controller"
watchdog = true
"#
    )
}
