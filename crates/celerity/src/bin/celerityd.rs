#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        env, io,
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::Duration,
    };

    use celerity_runtime::ValidatedBundle;
    use sd_notify::NotifyState;
    use signal_hook::consts::{SIGINT, SIGTERM};

    let bundle_path = env::args()
        .nth(1)
        .ok_or_else(|| io::Error::other("usage: celerityd <bundle.toml>"))?;
    let bundle = ValidatedBundle::load_configured(Path::new(&bundle_path))
        .map_err(|error| io::Error::other(format!("{error:?}")))?;
    let (_powertrain_receiver, _actuator_transport) =
        if let Some((powertrain, actuator)) = bundle.live_interfaces() {
            let powertrain_receiver = celerity::PowertrainCanReceiver::open(powertrain, actuator)
                .map_err(io::Error::other)?;
            let actuator_transport = celerity::ActuatorCanTransport::open(actuator, powertrain)
                .map_err(io::Error::other)?;
            (Some(powertrain_receiver), Some(actuator_transport))
        } else {
            (None, None)
        };
    let socket = env::var_os("CELERITY_DIAGNOSTICS_SOCKET")
        .map_or_else(|| bundle.diagnostics_socket().to_path_buf(), PathBuf::from);

    let stopping = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGTERM, Arc::clone(&stopping))?;
    signal_hook::flag::register(SIGINT, Arc::clone(&stopping))?;
    let diagnostics = celerity::serve_diagnostics(&socket, Arc::clone(&stopping))?;

    sd_notify::notify(&[
        NotifyState::Ready,
        NotifyState::Status("controller-local fallback; awaiting fresh reconciliation"),
    ])?;
    let watchdog_interval =
        sd_notify::watchdog_enabled().map_or(Duration::from_millis(250), |duration| duration / 2);
    while !stopping.load(Ordering::Relaxed) {
        sd_notify::notify(&[NotifyState::Watchdog])?;
        thread::sleep(watchdog_interval);
    }

    sd_notify::notify(&[
        NotifyState::Stopping,
        NotifyState::Status("fallback requested; lease renewal stopped"),
    ])?;
    diagnostics
        .join()
        .map_err(|_| io::Error::other("diagnostics thread panicked"))?
        .map_err(io::Error::other)?;
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Err(std::io::Error::other("celerityd requires Linux and systemd").into())
}
