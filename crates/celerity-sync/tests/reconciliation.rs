use std::{
    cell::RefCell,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use celerity_sync::{
    HomeApi, LinuxNetworkPresence, NetworkPresence, ReconcileOutcome, SyncConfiguration, SyncError,
    Synchronizer,
};
use serde_json::json;
use sha2::{Digest, Sha256};

struct Present;

impl NetworkPresence for Present {
    fn is_home(&self, _interface: &str, _gateway: &str) -> Result<bool, SyncError> {
        Ok(true)
    }
}

#[derive(Default)]
struct RecordingHome {
    missing: RefCell<Vec<String>>,
    uploaded: RefCell<Vec<String>>,
    model: RefCell<Option<Vec<u8>>>,
}

impl HomeApi for RecordingHome {
    fn reconcile(&self, request: &serde_json::Value) -> Result<serde_json::Value, SyncError> {
        *self.missing.borrow_mut() = request["completed_run_digests"]
            .as_array()
            .expect("run array")
            .iter()
            .map(|value| value.as_str().expect("digest").to_owned())
            .collect();
        Ok(json!({
            "missing_run_digests": *self.missing.borrow(),
            "desired_model_digest": self.model.borrow().as_ref().map(|bytes| digest(bytes)),
        }))
    }

    fn put_chunk(&self, run: &str, _chunk: &str, _bytes: &[u8]) -> Result<(), SyncError> {
        self.uploaded.borrow_mut().push(run.to_owned());
        Ok(())
    }

    fn complete(&self, run: &str, _manifest: &[u8]) -> Result<String, SyncError> {
        Ok(run.to_owned())
    }

    fn get_model(&self, _digest: &str) -> Result<Vec<u8>, SyncError> {
        Ok(self.model.borrow().clone().expect("model bytes"))
    }
}

fn root() -> PathBuf {
    static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "celerity-sync-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ))
}

fn digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(output, "{byte:02x}").expect("String write");
    }
    output
}

fn write_run(root: &Path, name: &str) {
    let directory = root.join("runs").join(name);
    fs::create_dir_all(&directory).expect("run directory");
    let bytes = format!("chunk-{name}").into_bytes();
    fs::write(directory.join("events.chunk"), &bytes).expect("chunk");
    fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "run_id": name,
            "completion": "complete",
            "chunk_file": "events.chunk",
            "chunk_sha256": digest(&bytes),
        }))
        .expect("manifest"),
    )
    .expect("write manifest");
}

fn configuration(root: &Path) -> SyncConfiguration {
    SyncConfiguration {
        spool_root: root.join("runs"),
        retention_count: 1,
        home_interface: "home0".to_owned(),
        expected_default_gateway: "192.0.2.1".to_owned(),
        home_api_url: "http://home.invalid".to_owned(),
        credential_path: root.join("token"),
        journal_path: root.join("sync.sqlite3"),
        model_root: root.join("models"),
    }
}

#[test]
fn linux_presence_requires_up_link_and_matching_default_route() {
    let root = root();
    fs::create_dir_all(root.join("sys/home0")).expect("link directory");
    fs::write(root.join("sys/home0/operstate"), "up\n").expect("operstate");
    fs::write(
        root.join("route"),
        "Iface Destination Gateway Flags RefCnt Use Metric Mask\nhome0 00000000 010200C0 0003 0 0 100 00000000\n",
    )
    .expect("route");
    let presence = LinuxNetworkPresence::from_paths(root.join("sys"), root.join("route"));
    assert_eq!(presence.is_home("home0", "192.0.2.1"), Ok(true));
    assert_eq!(presence.is_home("home0", "192.0.2.2"), Ok(false));
}

#[test]
fn reconcile_uploads_exact_artifacts_then_applies_acknowledged_retention() {
    let root = root();
    write_run(&root, "a");
    write_run(&root, "b");
    let home = RecordingHome {
        model: RefCell::new(Some(b"immutable-model-bundle".to_vec())),
        ..RecordingHome::default()
    };
    let expected_model_digest = digest(b"immutable-model-bundle");
    let mut sync = Synchronizer::new(configuration(&root)).expect("synchronizer");
    assert_eq!(
        sync.reconcile_once(&Present, &home, None, None, &[]),
        Ok(ReconcileOutcome::Synchronized {
            uploaded: 2,
            model_staged: true,
        })
    );
    assert_eq!(home.uploaded.borrow().len(), 2);
    assert!(!root.join("runs/a").exists());
    assert!(root.join("runs/b").exists());
    assert!(
        root.join("models")
            .join(format!("{expected_model_digest}.bundle"))
            .exists()
    );
}

#[test]
fn away_state_performs_no_inventory_or_http_work() {
    struct Away;
    impl NetworkPresence for Away {
        fn is_home(&self, _interface: &str, _gateway: &str) -> Result<bool, SyncError> {
            Ok(false)
        }
    }

    let root = root();
    let home = RecordingHome::default();
    let mut sync = Synchronizer::new(configuration(&root)).expect("synchronizer");
    assert_eq!(
        sync.reconcile_once(&Away, &home, None, None, &[]),
        Ok(ReconcileOutcome::Away)
    );
    assert!(home.missing.borrow().is_empty());
}
