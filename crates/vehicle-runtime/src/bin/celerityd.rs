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
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use control_core::{Completion, ModelSlots, RuntimeModel, StartupMode, ValidatedBundle};
    use sd_notify::NotifyState;
    use signal_hook::consts::{SIGINT, SIGTERM};

    let mut arguments = env::args().skip(1);
    let argument = arguments.next().ok_or_else(|| {
        io::Error::other(
            "usage: celerityd <live-bundle.toml> | celerityd --systemd-vcan-check <live-bundle.toml>",
        )
    })?;
    if argument == "--systemd-supervisor-check" {
        let stopping = Arc::new(AtomicBool::new(false));
        signal_hook::flag::register(SIGTERM, Arc::clone(&stopping))?;
        signal_hook::flag::register(SIGINT, Arc::clone(&stopping))?;
        sd_notify::notify(&[
            NotifyState::Ready,
            NotifyState::Status("test-only supervisor check; no vehicle authority"),
        ])?;
        let watchdog_interval = sd_notify::watchdog_enabled()
            .map_or(Duration::from_millis(250), |duration| duration / 2);
        while !stopping.load(Ordering::Relaxed) {
            sd_notify::notify(&[NotifyState::Watchdog])?;
            thread::sleep(watchdog_interval);
        }
        sd_notify::notify(&[
            NotifyState::Stopping,
            NotifyState::Status("test-only supervisor check stopped"),
        ])?;
        return Ok(());
    }

    let (argument, systemd_vcan_check) = if argument == "--systemd-vcan-check" {
        let bundle = arguments.next().ok_or_else(|| {
            io::Error::other("usage: celerityd --systemd-vcan-check <live-bundle.toml>")
        })?;
        (bundle, true)
    } else {
        (argument, false)
    };
    if arguments.next().is_some() {
        return Err(io::Error::other("celerityd accepts exactly one live bundle").into());
    }

    let bundle = ValidatedBundle::load(Path::new(&argument), StartupMode::Live)
        .map_err(|error| io::Error::other(format!("{error:?}")))?;
    let slots = ModelSlots::open(bundle.model_slots_root())
        .map_err(|error| io::Error::other(format!("{error:?}")))?;
    let mut selected = slots
        .select_configured_startup(bundle.model_abi(), bundle.model_input_length())
        .map_err(|error| io::Error::other(format!("{error:?}")))?;
    let mut model = None;
    if let Some(candidate) = &selected {
        match RuntimeModel::load(&candidate.onnx_path, &bundle) {
            Ok(loaded) => model = Some(loaded),
            Err(error) => {
                let rejected = candidate.digest.clone();
                slots
                    .mark_rejected(&rejected)
                    .map_err(|slot_error| io::Error::other(format!("{slot_error:?}")))?;
                eprintln!("rejected startup model {rejected}: {error:?}");
                selected = slots
                    .select_known_good_startup(bundle.model_abi(), bundle.model_input_length())
                    .map_err(|slot_error| io::Error::other(format!("{slot_error:?}")))?
                    .filter(|known_good| known_good.digest != rejected);
                if let Some(known_good) = &selected {
                    match RuntimeModel::load(&known_good.onnx_path, &bundle) {
                        Ok(loaded) => model = Some(loaded),
                        Err(error) => {
                            slots
                                .mark_rejected(&known_good.digest)
                                .map_err(|slot_error| {
                                    io::Error::other(format!("{slot_error:?}"))
                                })?;
                            eprintln!(
                                "rejected known-good startup model {}: {error:?}",
                                known_good.digest
                            );
                            selected = None;
                        }
                    }
                }
            }
        }
    }
    if bundle.model_required() && selected.is_none() {
        return Err(io::Error::other("required startup model is absent or invalid").into());
    }
    if selected.is_none() {
        slots
            .clear_active()
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
    }
    let socket = env::var_os("CELERITY_DIAGNOSTICS_SOCKET")
        .map_or_else(|| bundle.diagnostics_socket().to_path_buf(), PathBuf::from);
    let stopping = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGTERM, Arc::clone(&stopping))?;
    signal_hook::flag::register(SIGINT, Arc::clone(&stopping))?;
    let diagnostics_store = vehicle_diagnostics::DiagnosticsStore::new(
        vehicle_diagnostics::DiagnosticsSnapshot::startup_fallback(),
        bundle.cycle_ms(),
        Instant::now(),
    );
    let diagnostics = vehicle_diagnostics::serve_diagnostics(
        &socket,
        Arc::clone(&stopping),
        diagnostics_store.clone(),
    )?;

    let epoch = u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos())
        .unwrap_or(u64::MAX)
        ^ u64::from(std::process::id());
    let epoch = epoch.max(1);
    let run_id = format!("live-{epoch:016x}");
    let mut runtime = if systemd_vcan_check {
        vehicle_runtime::LiveRuntime::open_virtual_hardware_free(
            bundle,
            model,
            selected.as_ref().map(|selected| selected.digest.as_str()),
            epoch,
            &run_id,
            diagnostics_store,
        )
    } else {
        vehicle_runtime::LiveRuntime::open(
            bundle,
            model,
            selected.as_ref().map(|selected| selected.digest.as_str()),
            epoch,
            &run_id,
            diagnostics_store,
        )
    }
    .map_err(io::Error::other)?;
    if let Some(selected) = &selected {
        slots
            .mark_active(selected)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
    }

    sd_notify::notify(&[
        NotifyState::Ready,
        NotifyState::Status("controller-local fallback; reconciling live controller"),
    ])?;
    while !stopping.load(Ordering::Relaxed) {
        runtime.run_cycle().map_err(io::Error::other)?;
        sd_notify::notify(&[NotifyState::Watchdog])?;
    }

    sd_notify::notify(&[
        NotifyState::Stopping,
        NotifyState::Status("fallback requested; lease renewal stopped; sealing Run"),
    ])?;
    let model_acceptance_observed = runtime.model_acceptance_observed();
    let manifest = runtime.shutdown().map_err(io::Error::other)?;
    if model_acceptance_observed
        && manifest.completion == Completion::Complete
        && let Some(selected) = &selected
    {
        slots
            .promote_known_good(selected)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
    }
    stopping.store(true, Ordering::Relaxed);
    diagnostics
        .join()
        .map_err(|_| io::Error::other("diagnostics thread panicked"))?
        .map_err(io::Error::other)?;
    eprintln!(
        "sealed Run {} completion={:?} chunk_sha256={}",
        manifest.run_id, manifest.completion, manifest.chunk_sha256
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Err(std::io::Error::other("celerityd requires Linux and systemd").into())
}
