use std::{
    cell::RefCell,
    fmt::Write as _,
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use celerity_proto::celerity::v1::{
    CompletedRun, Completion, ModelBundleManifest, ModelCompatibility, ModelInputRange,
    ModelNormalization, ReconcileRequest, ReconcileResponse, RunChunk, RunManifest,
};
use prost::Message;
use sha2::{Digest, Sha256};
use sync::{
    HomeApi, LinuxNetworkPresence, NetworkPresence, ReconcileOutcome, SyncConfiguration, SyncError,
    Synchronizer,
};

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
    fn reconcile(&self, request: &ReconcileRequest) -> Result<ReconcileResponse, SyncError> {
        self.missing
            .borrow_mut()
            .clone_from(&request.completed_run_digests);
        Ok(ReconcileResponse {
            missing_run_digests: self.missing.borrow().clone(),
            desired_model_digest: self.model.borrow().as_ref().map(|bytes| digest(bytes)),
        })
    }

    fn put_chunk(&self, run: &str, _chunk: &str, _path: &Path) -> Result<(), SyncError> {
        self.uploaded.borrow_mut().push(run.to_owned());
        Ok(())
    }

    fn complete(&self, run: &str, _manifest: &[u8]) -> Result<CompletedRun, SyncError> {
        Ok(CompletedRun {
            run_digest: run.to_owned(),
        })
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

fn model_bundle() -> Vec<u8> {
    let model =
        fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/models/identity.onnx"))
            .expect("model fixture");
    let manifest = ModelBundleManifest {
        schema_version: 1,
        onnx_sha256: digest(&model),
        signal_order: vec![
            "coolant_temperature_c".to_owned(),
            "air_temperature_c".to_owned(),
        ],
        units: vec!["native".to_owned(), "native".to_owned()],
        sample_period_ms: 20,
        history_length: 4,
        horizons: vec![1],
        output_order: vec!["coolant".to_owned(), "post_intercooler_iat".to_owned()],
        command_lattice: vec![1000, 5000, 9000],
        compatibility: Some(ModelCompatibility {
            model_abi: "thermal-v1".to_owned(),
            input_shape: vec![1, 9],
            ..ModelCompatibility::default()
        }),
        input_ranges: vec![
            ModelInputRange {
                minimum: 60.0,
                maximum: 120.0,
            },
            ModelInputRange {
                minimum: 0.0,
                maximum: 100.0,
            },
        ],
        normalization: vec![
            ModelNormalization {
                mean: 90.0,
                scale: 10.0,
            },
            ModelNormalization {
                mean: 40.0,
                scale: 10.0,
            },
        ],
        calibration_error: 0.01,
    }
    .encode_to_vec();
    let cursor = Cursor::new(Vec::new());
    let mut archive = zip::ZipWriter::new(cursor);
    let options = zip::write::SimpleFileOptions::default();
    archive
        .start_file("manifest.pb", options)
        .expect("manifest entry");
    archive.write_all(&manifest).expect("manifest bytes");
    archive
        .start_file("model.onnx", options)
        .expect("model entry");
    archive.write_all(&model).expect("model bytes");
    archive.finish().expect("finish bundle").into_inner()
}

fn write_run(root: &Path, name: &str) {
    let directory = root.join("runs").join(name);
    fs::create_dir_all(&directory).expect("run directory");
    let bytes = format!("chunk-{name}").into_bytes();
    fs::write(directory.join("events.chunk"), &bytes).expect("chunk");
    fs::write(
        directory.join("manifest.pb"),
        RunManifest {
            schema_version: 1,
            run_id: name.to_owned(),
            completion: Completion::Complete as i32,
            chunk_file: "events.chunk".to_owned(),
            chunk_sha256: digest(&bytes),
            chunks: vec![RunChunk {
                file: "events.chunk".to_owned(),
                sha256: digest(&bytes),
            }],
            ..RunManifest::default()
        }
        .encode_to_vec(),
    )
    .expect("write manifest");
}

fn configuration(root: &Path) -> SyncConfiguration {
    SyncConfiguration {
        spool_root: root.join("runs"),
        retention_count: 1,
        incomplete_retention_count: 1,
        home_interface: "home0".to_owned(),
        expected_default_gateway: "192.0.2.1".to_owned(),
        home_api_url: "http://home.invalid".to_owned(),
        credential_path: root.join("token"),
        journal_path: root.join("sync.sqlite3"),
        model_root: root.join("models"),
        model_abi: "thermal-v1".to_owned(),
        model_input_signals: vec![
            "coolant_temperature_c".to_owned(),
            "air_temperature_c".to_owned(),
        ],
        model_history_length: 4,
        model_command_lattice: vec![1000, 5000, 9000],
        model_maximum_calibration_error: 0.1,
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
    write_run(&root, "z-old");
    std::thread::sleep(Duration::from_millis(10));
    write_run(&root, "a-new");
    let bundle = model_bundle();
    let home = RecordingHome {
        model: RefCell::new(Some(bundle.clone())),
        ..RecordingHome::default()
    };
    let expected_model_digest = digest(&bundle);
    let mut sync = Synchronizer::new(configuration(&root)).expect("synchronizer");
    assert_eq!(
        sync.reconcile_once(&Present, &home, None, None, &[]),
        Ok(ReconcileOutcome::Synchronized {
            uploaded: 2,
            model_staged: true,
        })
    );
    assert_eq!(home.uploaded.borrow().len(), 2);
    assert!(!root.join("runs/z-old").exists());
    assert!(root.join("runs/a-new").exists());
    assert_eq!(
        fs::read_to_string(root.join("models/desired-digest")).expect("desired marker"),
        format!("{expected_model_digest}\n")
    );
    assert!(root.join("models/slot-a/model.onnx").exists());
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

#[test]
fn staging_preserves_the_only_known_good_rollback_slot() {
    let root = root();
    let models = root.join("models");
    fs::create_dir_all(models.join("slot-a")).expect("slot A");
    fs::create_dir_all(models.join("slot-b")).expect("slot B");
    fs::write(models.join("slot-a/bundle-digest"), "known-good\n").expect("known-good slot");
    fs::write(models.join("slot-b/bundle-digest"), "unproven-active\n").expect("active slot");
    fs::write(models.join("known-good-digest"), "known-good\n").expect("known-good marker");
    fs::write(models.join("active-digest"), "unproven-active\n").expect("active marker");
    let known_good_contents = fs::read(models.join("slot-a/bundle-digest")).expect("contents");
    let home = RecordingHome {
        model: RefCell::new(Some(model_bundle())),
        ..RecordingHome::default()
    };
    let mut sync = Synchronizer::new(configuration(&root)).expect("synchronizer");

    assert_eq!(
        sync.reconcile_once(
            &Present,
            &home,
            Some("unproven-active"),
            Some("unproven-active"),
            &[],
        ),
        Ok(ReconcileOutcome::Synchronized {
            uploaded: 0,
            model_staged: false,
        })
    );
    assert_eq!(
        fs::read(models.join("slot-a/bundle-digest")).expect("preserved slot"),
        known_good_contents
    );
}
