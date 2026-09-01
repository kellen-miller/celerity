use std::{
    path::Path,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use vehicle_ui::StatusObserver;

const DIAGNOSTICS_SOCKET: &str = "/run/celerity/diagnostics.sock";

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let server = tiny_http::Server::http("127.0.0.1:8080")?;
    let observer = Arc::new(RwLock::new(StatusObserver::new()));
    let stopping = Arc::new(AtomicBool::new(false));
    let worker_observer = Arc::clone(&observer);
    let worker_stopping = Arc::clone(&stopping);
    let worker = thread::Builder::new()
        .name("celerity-status-observer".to_owned())
        .spawn(move || {
            let mut failure_active = false;
            let mut last_failure_log = None;
            while !worker_stopping.load(Ordering::Relaxed) {
                let observed_at = Instant::now();
                let result = vehicle_diagnostics::read_diagnostics(Path::new(DIAGNOSTICS_SOCKET));
                match &result {
                    Ok(_) if failure_active => {
                        eprintln!("diagnostics connection recovered");
                        failure_active = false;
                        last_failure_log = None;
                    }
                    Ok(_) => {}
                    Err(error)
                        if !failure_active
                            || last_failure_log.is_none_or(|last_log: Instant| {
                                observed_at.saturating_duration_since(last_log)
                                    >= Duration::from_secs(5)
                            }) =>
                    {
                        eprintln!("diagnostics read failed: {error}");
                        failure_active = true;
                        last_failure_log = Some(observed_at);
                    }
                    Err(_) => {
                        failure_active = true;
                    }
                }
                worker_observer
                    .write()
                    .expect("status observer lock poisoned")
                    .observe(result, observed_at);
                thread::sleep(Duration::from_millis(500));
            }
        })?;

    let result = vehicle_ui::run_http(server, observer, Arc::clone(&stopping));
    stopping.store(true, Ordering::Relaxed);
    worker
        .join()
        .map_err(|_| std::io::Error::other("status observer thread panicked"))?;
    result.map_err(std::io::Error::other)?;
    Ok(())
}
