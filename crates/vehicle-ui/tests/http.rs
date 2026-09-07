use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use vehicle_diagnostics::DiagnosticsSnapshot;
use vehicle_ui::{StatusObserver, run_http};

#[test]
fn status_route_is_get_only_host_guarded_and_hardened() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral listener");
    let address = listener.local_addr().expect("listener address");
    let server = tiny_http::Server::from_listener(listener, None).expect("HTTP server");
    let mut observer = StatusObserver::new();
    let mut snapshot = DiagnosticsSnapshot::startup_fallback();
    snapshot.runtime_update_stale_after_ms = 100;
    observer.observe(Ok(snapshot), Instant::now());
    let observer = Arc::new(RwLock::new(observer));
    let stopping = Arc::new(AtomicBool::new(false));
    let worker_stopping = Arc::clone(&stopping);
    let worker = thread::spawn(move || run_http(server, observer, worker_stopping));

    let status = request(address, "GET", "/v1/status", "localhost:8080");
    assert!(status.starts_with("HTTP/1.1 200"), "{status}");
    assert!(
        status.contains("Content-Type: application/x-protobuf"),
        "{status}"
    );
    assert!(status.contains("Cache-Control: no-store"), "{status}");
    assert!(!status.to_ascii_lowercase().contains("access-control"));

    let forbidden = request(address, "GET", "/v1/status", "example.test");
    assert!(forbidden.starts_with("HTTP/1.1 403"), "{forbidden}");
    let missing = request(address, "GET", "/missing", "[::1]:4567");
    assert!(missing.starts_with("HTTP/1.1 404"), "{missing}");
    let method = request(address, "POST", "/v1/status", "127.0.0.1");
    assert!(method.starts_with("HTTP/1.1 405"), "{method}");
    assert!(method.contains("Allow: GET"), "{method}");

    let html = request(address, "GET", "/", "localhost");
    assert!(html.starts_with("HTTP/1.1 200"), "{html}");
    assert!(html.contains("X-Content-Type-Options: nosniff"), "{html}");
    assert!(
        html.contains("Content-Security-Policy: default-src 'none'"),
        "{html}"
    );

    stopping.store(true, Ordering::Relaxed);
    worker.join().expect("HTTP thread").expect("HTTP result");
}

#[test]
fn status_response_does_not_wait_for_diagnostics_io() {
    let temporary = tempfile::tempdir().expect("temporary directory");
    let diagnostics_socket = temporary.path().join("silent.sock");
    let diagnostics_listener = std::os::unix::net::UnixListener::bind(&diagnostics_socket)
        .expect("silent diagnostics listener");
    let diagnostics_server = thread::spawn(move || {
        let (_stream, _) = diagnostics_listener.accept().expect("diagnostics accept");
        thread::sleep(Duration::from_millis(400));
    });

    let listener = TcpListener::bind("127.0.0.1:0").expect("ephemeral listener");
    let address = listener.local_addr().expect("listener address");
    let server = tiny_http::Server::from_listener(listener, None).expect("HTTP server");
    let observer = Arc::new(RwLock::new(StatusObserver::new()));
    let observing = Arc::clone(&observer);
    let diagnostics = thread::spawn(move || {
        let result = vehicle_diagnostics::read_diagnostics(&diagnostics_socket);
        observing
            .write()
            .expect("observer lock")
            .observe(result, Instant::now());
    });
    thread::sleep(Duration::from_millis(25));

    let stopping = Arc::new(AtomicBool::new(false));
    let worker_stopping = Arc::clone(&stopping);
    let worker = thread::spawn(move || run_http(server, observer, worker_stopping));
    let started = Instant::now();
    let response = request(address, "GET", "/v1/status", "localhost");
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(started.elapsed() < Duration::from_millis(150));

    diagnostics.join().expect("diagnostics client");
    diagnostics_server.join().expect("diagnostics server");
    stopping.store(true, Ordering::Relaxed);
    worker.join().expect("HTTP thread").expect("HTTP result");
}

fn request(address: std::net::SocketAddr, method: &str, path: &str, host: &str) -> String {
    let mut stream = TcpStream::connect(address).expect("HTTP connect");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    )
    .expect("HTTP request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("HTTP response");
    response
}
