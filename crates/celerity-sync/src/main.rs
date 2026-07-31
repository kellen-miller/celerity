#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        env, fs, io,
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
    let model_root = configuration.model_root.clone();
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
        let read_marker = |name: &str| {
            fs::read_to_string(model_root.join(name))
                .ok()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let active = read_marker("active-digest");
        let desired = read_marker("desired-digest");
        let rejected = fs::read_to_string(model_root.join("rejected-digests"))
            .map(|contents| {
                contents
                    .lines()
                    .map(str::trim)
                    .filter(|digest| !digest.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if let Err(error) = synchronizer.reconcile_once(
            &network,
            &home,
            active.as_deref(),
            desired.as_deref(),
            &rejected,
        ) {
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
