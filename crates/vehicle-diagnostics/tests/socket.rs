#![cfg(unix)]

use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    os::unix::net::{UnixListener, UnixStream},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use vehicle_diagnostics::{
    CommandSource, DiagnosticStatus, DiagnosticUnknownReason, DiagnosticsSnapshot,
    DiagnosticsStore, FeatureAuthority, GlobalAuthority, RunStorageHealth, read_diagnostics,
    serve_diagnostics,
};

#[test]
fn startup_snapshot_is_typed_and_socket_is_group_readable() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let socket = temporary.path().join("diagnostics.sock");
    let stopping = Arc::new(AtomicBool::new(false));
    let published_at = Instant::now();
    let expected = DiagnosticsSnapshot {
        schema_version: 2,
        runtime_update_age_ms: 0,
        runtime_update_stale_after_ms: 100,
        global_authority: GlobalAuthority::Fallback,
        feature_authority: FeatureAuthority::Fallback,
        command_source: CommandSource::ControllerLocalFallback,
        accepted_radiator_split_command: None,
        coolant_temperature: None,
        intake_air_temperature: None,
        controller_runtime_lease_health: DiagnosticStatus::Unknown(
            DiagnosticUnknownReason::NotExpectedInFallback,
        ),
        controller_command_ack_health: DiagnosticStatus::Unknown(
            DiagnosticUnknownReason::NoOutstandingCommand,
        ),
        run_storage_health: RunStorageHealth::Unknown,
    };
    let store = DiagnosticsStore::new(expected.clone(), 20, published_at);
    let worker = serve_diagnostics(&socket, Arc::clone(&stopping), store).expect("server");

    let snapshot = read_diagnostics(&socket).expect("status");
    assert_eq!(snapshot.schema_version, 2);
    assert_eq!(snapshot.runtime_update_stale_after_ms, 100);
    assert_eq!(snapshot.global_authority, expected.global_authority);
    assert_eq!(snapshot.feature_authority, expected.feature_authority);
    assert_eq!(snapshot.command_source, expected.command_source);
    assert_eq!(
        fs::metadata(&socket)
            .expect("socket metadata")
            .permissions()
            .mode()
            & 0o777,
        0o660
    );

    stopping.store(true, Ordering::Relaxed);
    worker.join().expect("worker join").expect("worker result");
    assert!(!socket.exists());
    assert!(published_at.elapsed() < Duration::from_secs(2));
}

#[test]
fn bounded_transport_survives_silent_and_malformed_peers() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let socket = temporary.path().join("diagnostics.sock");
    let stopping = Arc::new(AtomicBool::new(false));
    let store = DiagnosticsStore::new(DiagnosticsSnapshot::startup_fallback(), 20, Instant::now());
    let worker = serve_diagnostics(&socket, Arc::clone(&stopping), store).expect("server");

    let silent = UnixStream::connect(&socket).expect("silent client");
    thread::sleep(Duration::from_millis(300));
    drop(silent);

    let mut malformed = UnixStream::connect(&socket).expect("malformed client");
    malformed
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("read timeout");
    malformed.write_all(b"mutate\n").expect("malformed request");
    let mut rejection = String::new();
    malformed
        .read_to_string(&mut rejection)
        .expect("bounded rejection");
    assert_eq!(
        rejection,
        "{\"error\":\"read-only status request required\"}\n"
    );

    let mut disconnected = UnixStream::connect(&socket).expect("disconnecting client");
    disconnected.write_all(b"status\n").expect("status request");
    drop(disconnected);
    assert_eq!(
        read_diagnostics(&socket)
            .expect("subsequent status")
            .schema_version,
        2
    );

    let stop_started = Instant::now();
    stopping.store(true, Ordering::Relaxed);
    worker.join().expect("worker join").expect("worker result");
    assert!(stop_started.elapsed() < Duration::from_secs(1));
}

#[test]
fn client_read_times_out_when_server_never_responds() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let socket = temporary.path().join("diagnostics.sock");
    let listener = UnixListener::bind(&socket).expect("fake listener");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("fake accept");
        let mut request = [0_u8; 7];
        stream.read_exact(&mut request).expect("status request");
        assert_eq!(&request, b"status\n");
        thread::sleep(Duration::from_millis(400));
    });

    let started = Instant::now();
    let error = read_diagnostics(&socket).expect_err("silent server must time out");
    assert!(started.elapsed() >= Duration::from_millis(200));
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(!error.is_empty());
    server.join().expect("fake server");
}
