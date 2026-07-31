#[cfg(unix)]
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

    use celerity_sync::{LinuxNetworkPresence, ReqwestHomeApi, SyncConfiguration, Synchronizer};
    use signal_hook::consts::{SIGINT, SIGTERM};

    let bundle_path = env::args()
        .nth(1)
        .ok_or_else(|| io::Error::other("usage: celerity-sync <bundle.toml>"))?;
    let journal_path = env::var_os("CELERITY_SYNC_JOURNAL").map_or_else(
        || PathBuf::from("/var/lib/celerity/sync.sqlite3"),
        PathBuf::from,
    );
    let model_root = env::var_os("CELERITY_MODEL_STAGING_ROOT")
        .map_or_else(|| PathBuf::from("/var/lib/celerity/models"), PathBuf::from);
    let mut configuration =
        SyncConfiguration::from_bundle(Path::new(&bundle_path), journal_path, model_root)?;
    if let Some(token_path) = env::var_os("CELERITY_HOME_TOKEN_PATH") {
        configuration.credential_path = PathBuf::from(token_path);
    }
    let home = ReqwestHomeApi::new(&configuration.home_api_url, &configuration.credential_path)?;
    let network = LinuxNetworkPresence::host();
    let mut synchronizer = Synchronizer::new(configuration)?;
    let stopping = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(SIGTERM, Arc::clone(&stopping))?;
    signal_hook::flag::register(SIGINT, Arc::clone(&stopping))?;

    while !stopping.load(Ordering::Relaxed) {
        if let Err(error) = synchronizer.reconcile_once(&network, &home, None, None, &[]) {
            eprintln!("sync reconciliation deferred: {error}");
        }
        for _ in 0..30 {
            if stopping.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    Err(std::io::Error::other("celerity-sync requires Unix").into())
}
