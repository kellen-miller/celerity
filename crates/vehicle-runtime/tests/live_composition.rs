#![cfg(target_os = "linux")]

use std::{
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use control_core::{
    CommandSource, Completion, ControllerEmulator, EmulatorProvisioning, RuntimeModel, StartupMode,
    ValidatedBundle, replay_events,
};
use control_protocol::{Frame, decode, encode};
use socketcan::{
    CanFdFrame, CanFdSocket, CanFrame, CanSocket, EmbeddedFrame, Frame as SocketCanFrame, Socket,
};
use vehicle_diagnostics::{
    CommandSource as DiagnosticCommandSource, DiagnosticStatus, DiagnosticUnknownReason,
    DiagnosticsSnapshot, DiagnosticsStore, RunStorageHealth,
};

#[test]
fn live_kernel_sockets_drive_model_command_and_seal_run() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let bundle_path = temporary.path().join("live.toml");
    let runs = temporary.path().join("runs");
    let slots = temporary.path().join("models");
    fs::write(
        &bundle_path,
        live_bundle(
            runs.to_str().expect("run path"),
            slots.to_str().expect("slot path"),
        ),
    )
    .expect("live bundle");
    let bundle = ValidatedBundle::load(&bundle_path, StartupMode::Live).expect("valid live bundle");
    let model_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/golden/model-v1/identity.onnx");
    let model = RuntimeModel::load(&model_path, &bundle).expect("production model");
    let diagnostics = DiagnosticsStore::new(
        DiagnosticsSnapshot::startup_fallback(),
        20,
        std::time::Instant::now(),
    );
    let stopping = Arc::new(AtomicBool::new(false));
    let diagnostics_socket = temporary.path().join("runtime-diagnostics.sock");
    let diagnostics_worker = vehicle_diagnostics::serve_diagnostics(
        &diagnostics_socket,
        Arc::clone(&stopping),
        diagnostics.clone(),
    )
    .expect("diagnostics server");
    let startup =
        vehicle_diagnostics::read_diagnostics(&diagnostics_socket).expect("startup diagnostics");
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
    let observed_commands = Arc::new(Mutex::new(Vec::new()));
    let controller = spawn_controller(Arc::clone(&stopping), Arc::clone(&observed_commands));

    let mut runtime = vehicle_runtime::LiveRuntime::open_virtual_hardware_free(
        bundle,
        Some(model),
        Some("fixture-model-bundle"),
        99,
        "live-kernel",
        diagnostics,
    )
    .expect("production loop must bind vcan");
    for _ in 0..4 {
        send_powertrain_temperature(100, 40);
        runtime.run_cycle().expect("bounded live cycle");
    }

    let outcome = runtime.outcome();
    assert_eq!(outcome.command_source, CommandSource::ModelOptimized);
    assert_eq!(outcome.accepted_basis_points, Some(9_000));
    assert!(observed_commands.lock().expect("commands").contains(&9_000));
    let snapshot =
        vehicle_diagnostics::read_diagnostics(&diagnostics_socket).expect("diagnostics snapshot");
    assert_eq!(
        snapshot.command_source,
        DiagnosticCommandSource::ModelOptimized
    );
    assert_eq!(
        snapshot
            .accepted_radiator_split_command
            .expect("accepted command")
            .basis_points,
        9_000
    );
    assert_eq!(
        snapshot.controller_runtime_lease_health,
        DiagnosticStatus::Healthy
    );
    assert_eq!(
        snapshot.controller_command_ack_health,
        DiagnosticStatus::Healthy
    );
    assert_eq!(snapshot.run_storage_health, RunStorageHealth::Healthy);
    let coolant = snapshot
        .coolant_temperature
        .expect("finite coolant temperature")
        .degrees_celsius;
    assert!((coolant - 100.0).abs() < f64::EPSILON);
    thread::sleep(Duration::from_millis(120));
    let delayed =
        vehicle_diagnostics::read_diagnostics(&diagnostics_socket).expect("delayed diagnostics");
    assert!(delayed.runtime_update_age_ms >= delayed.runtime_update_stale_after_ms);
    let manifest = runtime.shutdown().expect("Run must seal");
    let stopped =
        vehicle_diagnostics::read_diagnostics(&diagnostics_socket).expect("shutdown diagnostics");
    assert!(stopped.accepted_radiator_split_command.is_none());
    assert_eq!(
        stopped.controller_runtime_lease_health,
        DiagnosticStatus::Unknown(DiagnosticUnknownReason::NotExpectedInFallback)
    );
    assert_eq!(manifest.completion, Completion::Complete);
    assert!(runs.join("live-kernel/manifest.json").is_file());
    assert!(runs.join("live-kernel").join(manifest.chunk_file).is_file());
    let replayed = replay_events(&runs.join("live-kernel")).expect("canonical Run must replay");
    assert!(replayed.len() >= 4, "live Run must contain runtime events");

    stopping.store(true, Ordering::Relaxed);
    send_controller_wakeup();
    controller.join().expect("controller thread");
    diagnostics_worker
        .join()
        .expect("diagnostics thread")
        .expect("diagnostics result");
}

fn spawn_controller(
    stopping: Arc<AtomicBool>,
    observed_commands: Arc<Mutex<Vec<u16>>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let socket = CanFdSocket::open("vcan-controller").expect("controller vcan");
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
            let frame = socket.read_frame().expect("controller receive");
            let decoded = decode(
                u16::try_from(frame.raw_id()).expect("standard id"),
                frame.data(),
            )
            .expect("typed controller frame");
            if let Frame::Command { message, .. } = &decoded {
                observed_commands
                    .lock()
                    .expect("commands")
                    .push(message.radiator_split_basis_points);
            }
            let now_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let mut response = emulator
                .ingest(
                    now_ms,
                    u16::try_from(frame.raw_id()).expect("id"),
                    frame.data(),
                )
                .expect("controller input");
            while let Some(current) = response {
                write_fd(&socket, current.can_id, current.payload());
                response = current.follow_up().cloned();
            }
            if let Some(heartbeat) = emulator.heartbeat(now_ms).expect("heartbeat") {
                write_fd(&socket, heartbeat.can_id, heartbeat.payload());
            }
        }
    })
}

fn write_fd(socket: &CanFdSocket, can_id: u16, payload: &[u8]) {
    let id = socketcan::StandardId::new(can_id).expect("standard id");
    let frame = CanFdFrame::new(id, payload).expect("CAN FD frame");
    socket.write_frame(&frame).expect("controller transmit");
}

fn send_powertrain_temperature(coolant_c: i16, iat_c: i16) {
    let socket = CanSocket::open("vcan-powertrain").expect("powertrain vcan");
    let raw = |temperature_c: i16| {
        u16::try_from(i32::from(temperature_c) * 10 + 2_731)
            .expect("test temperature must fit the Haltech encoding")
    };
    let coolant = raw(coolant_c).to_be_bytes();
    let iat = raw(iat_c).to_be_bytes();
    let payload = [coolant[0], coolant[1], iat[0], iat[1], 0, 0, 0, 0];
    let id = socketcan::StandardId::new(0x3e0).expect("standard id");
    socket
        .write_frame(&CanFrame::new(id, &payload).expect("powertrain frame"))
        .expect("powertrain transmit");
}

fn send_controller_wakeup() {
    let socket = CanFdSocket::open("vcan-controller").expect("controller vcan");
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
    .expect("wakeup frame");
    write_fd(&socket, encoded.can_id, &payload[..encoded.len]);
    thread::sleep(Duration::from_millis(10));
}

fn live_bundle(run_root: &str, slots_root: &str) -> String {
    format!(
        r#"schema_version = 1
generation = 1
mode = "live"

[runtime]
cycle_ms = 20

[powertrain]
decoder_generation = 1

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
acknowledgement_deadline_ms = 20
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
