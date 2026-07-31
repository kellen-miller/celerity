use std::{
    fmt::Write as _,
    fs, io,
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use celerity_sync::{
    NetworkPresence, ReconcileOutcome, ReqwestHomeApi, SyncConfiguration, SyncError, Synchronizer,
};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

struct AlwaysHome;

impl NetworkPresence for AlwaysHome {
    fn is_home(&self, _interface: &str, _expected_gateway: &str) -> Result<bool, SyncError> {
        Ok(true)
    }
}

struct HomeProcess(Child);

impl Drop for HomeProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[derive(Deserialize)]
struct JobStarted {
    job_id: String,
}

#[derive(Deserialize)]
struct JobStatus {
    state: String,
    artifact_digest: Option<String>,
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("String write");
            output
        })
}

fn make_complete_run(spool: &Path) -> io::Result<String> {
    let run = spool.join("run-e2e");
    fs::create_dir_all(&run)?;
    let events = b"time_ms,coolant_c,iat_c,split_bp\n0,88,42,7500\n";
    fs::write(run.join("events.chunk"), events)?;
    let manifest = serde_json::to_vec(&json!({
        "chunk_file": "events.chunk",
        "chunk_sha256": digest(events),
        "completion": "complete",
        "run_id": "run-e2e",
        "schema_version": 1
    }))?;
    fs::write(run.join("manifest.json"), &manifest)?;
    Ok(digest(&manifest))
}

#[test]
#[ignore = "run by scripts/check-e2e with the home Python environment"]
fn production_http_upload_trains_and_stages_exact_model() {
    let temporary = TempDir::new().expect("temporary acceptance root");
    let spool = temporary.path().join("spool");
    let model_root = temporary.path().join("models");
    let home_root = temporary.path().join("home");
    let run_digest = make_complete_run(&spool).expect("complete Run fixture");
    let token_path = temporary.path().join("vehicle-token");
    fs::write(&token_path, "e2e-secret\n").expect("vehicle token");

    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve local HTTP port");
    let port = listener.local_addr().expect("reserved address").port();
    drop(listener);
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let child = Command::new("uv")
        .args([
            "run",
            "--project",
            "services/home",
            "uvicorn",
            "celerity_home.testing_server:app",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--log-level",
            "warning",
        ])
        .current_dir(repository)
        .env("CELERITY_HOME_STORAGE_ROOT", &home_root)
        .env("CELERITY_HOME_VEHICLE_TOKEN", "e2e-secret")
        .env("CELERITY_HOME_WEBHOOK_URL", "https://127.0.0.1:9/webhook")
        .stdout(Stdio::null())
        .spawn()
        .expect("start production ASGI server");
    let _home_process = HomeProcess(child);

    let base_url = format!("http://127.0.0.1:{port}");
    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("HTTP client");
    let ready_deadline = Instant::now() + Duration::from_secs(10);
    while client
        .get(format!("{base_url}/health/ready"))
        .send()
        .is_err()
    {
        assert!(
            Instant::now() < ready_deadline,
            "home API did not become ready"
        );
        thread::sleep(Duration::from_millis(50));
    }

    let configuration = SyncConfiguration {
        spool_root: spool,
        retention_count: 2,
        home_interface: "test-home".to_owned(),
        expected_default_gateway: "127.0.0.1".to_owned(),
        home_api_url: base_url.clone(),
        credential_path: token_path.clone(),
        journal_path: temporary.path().join("sync.sqlite3"),
        model_root: model_root.clone(),
    };
    let home = ReqwestHomeApi::new(&base_url, &token_path).expect("production home client");
    let mut synchronizer = Synchronizer::new(configuration).expect("synchronizer");
    assert_eq!(
        synchronizer
            .reconcile_once(&AlwaysHome, &home, None, None, &[])
            .expect("upload complete Run"),
        ReconcileOutcome::Synchronized {
            uploaded: 1,
            model_staged: false,
        }
    );

    let started: JobStarted = client
        .post(format!("{base_url}/v1/jobs"))
        .bearer_auth("e2e-secret")
        .json(&json!({"recipe": "causal-tcn-v1", "run_digests": [run_digest]}))
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .expect("start managed training job")
        .json()
        .expect("training job response");
    let job_deadline = Instant::now() + Duration::from_secs(30);
    let artifact_digest = loop {
        let job: JobStatus = client
            .get(format!("{base_url}/v1/jobs/{}", started.job_id))
            .bearer_auth("e2e-secret")
            .send()
            .and_then(reqwest::blocking::Response::error_for_status)
            .expect("read training job")
            .json()
            .expect("training job status");
        if job.state == "completed" {
            break job.artifact_digest.expect("completed model digest");
        }
        assert_ne!(job.state, "failed", "training job failed");
        assert!(Instant::now() < job_deadline, "training job timed out");
        thread::sleep(Duration::from_millis(50));
    };

    assert_eq!(
        synchronizer
            .reconcile_once(&AlwaysHome, &home, None, None, &[])
            .expect("download desired model"),
        ReconcileOutcome::Synchronized {
            uploaded: 0,
            model_staged: true,
        }
    );
    let staged = model_root.join(format!("{artifact_digest}.bundle"));
    let bytes = fs::read(staged).expect("atomically staged model bundle");
    assert_eq!(digest(&bytes), artifact_digest);
}
