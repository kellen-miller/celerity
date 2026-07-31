#![cfg(unix)]

use std::{
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

#[test]
fn diagnostics_exposes_read_only_startup_fallback() {
    static NEXT_SOCKET: AtomicU64 = AtomicU64::new(0);
    let socket = PathBuf::from(format!(
        "/tmp/celerity-diagnostics-{}-{}.sock",
        std::process::id(),
        NEXT_SOCKET.fetch_add(1, Ordering::Relaxed)
    ));
    let stopping = Arc::new(AtomicBool::new(false));
    let expected = celerity::DiagnosticsSnapshot::startup_fallback();
    let shared = Arc::new(RwLock::new(expected.clone()));
    let worker =
        celerity::serve_diagnostics(&socket, Arc::clone(&stopping), shared).expect("server");
    let snapshot = celerity::read_diagnostics(&socket).expect("status");
    assert_eq!(snapshot, expected);
    assert!(!snapshot.lease_renewal);
    stopping.store(true, Ordering::Relaxed);
    worker.join().expect("worker join").expect("worker result");
    assert!(!socket.exists());
}
