use std::{
    fs, io,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use celerity_proto::celerity::v1::{JobRequest, JobStarted, JobState, JobStatus};
use control_core::{
    CommandSource, EnqueueResult, ExternalAdapters, ModelSlots, RunContext, RunRecord, RunWriter,
    Runtime, RuntimeEffect, RuntimeEvent, RuntimeModel, StartupMode, ValidatedBundle,
};
use prost::Message;
use reqwest::blocking::Client;
use serde_json::json;
use sha2::{Digest, Sha256};
use sync::{
    NetworkPresence, ReconcileOutcome, ReqwestHomeApi, SyncConfiguration, SyncError, Synchronizer,
};
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
        if let Err(error) = self.0.kill()
            && error.kind() != io::ErrorKind::InvalidInput
        {
            eprintln!("failed to stop home test process: {error}");
        }

        if let Err(error) = self.0.wait() {
            eprintln!("failed to reap home test process: {error}");
        }
    }
}

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn make_complete_run(spool: &Path, name: &str) -> io::Result<String> {
    let writer = RunWriter::start_with_context(
        spool,
        name,
        64,
        0,
        RunContext {
            configuration_generation: 1,
            configuration_sha256: "a".repeat(64),
            model_bundle_digest: None,
            protocol_major: 1,
            firmware_generation: 1,
            decoder_generation: 1,
            model_abi: "thermal-v1".to_owned(),
            model_input_signals: vec![
                "coolant_temperature_c".to_owned(),
                "air_temperature_c".to_owned(),
            ],
            model_history_length: 4,
            sample_period_ms: 20,
            command_lattice: vec![1000, 5000, 9000],
            maximum_calibration_error: 100.0,
        },
    )
    .map_err(io::Error::other)?;
    let mut sequence = 0;
    for index in 0_u32..7 {
        for (signal, value) in [
            ("coolant_temperature_c", 90.0 - f64::from(index)),
            ("air_temperature_c", 45.0 - f64::from(index) / 2.0),
        ] {
            sequence += 1;
            assert_eq!(
                writer.enqueue(RunRecord::signal(
                    sequence, sequence, signal, value, 1, 0, 1
                )),
                EnqueueResult::Accepted
            );
        }
        if index < 6 {
            sequence += 1;
            let command = match index % 3 {
                0 => 1000,
                1 => 5000,
                _ => 9000,
            };
            assert_eq!(
                writer.enqueue(RunRecord::control(
                    sequence,
                    sequence,
                    &json!({
                        "schema_version": 1,
                        "state": "accepted",
                        "radiator_split_basis_points": command
                    })
                    .to_string(),
                )),
                EnqueueResult::Accepted
            );
        }
    }
    writer.seal().map_err(io::Error::other)?;
    let manifest = fs::read(spool.join(name).join("manifest.pb"))?;
    Ok(digest(&manifest))
}

fn start_and_poll_training_job(
    client: &Client,
    base_url: &str,
    run_digests: &[String],
) -> Result<JobStatus, Box<dyn std::error::Error>> {
    let response = client
        .post(format!("{base_url}/v1/jobs"))
        .bearer_auth("e2e-secret")
        .header("Content-Type", "application/x-protobuf")
        .body(
            JobRequest {
                run_digests: run_digests.to_vec(),
                recipe: "causal-tcn-v1".to_owned(),
            }
            .encode_to_vec(),
        )
        .send()?
        .error_for_status()?
        .bytes()?;
    let started = JobStarted::decode(response.as_ref())?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let response = client
            .get(format!("{base_url}/v1/jobs/{}", started.job_id))
            .bearer_auth("e2e-secret")
            .send()?
            .error_for_status()?
            .bytes()?;
        let job = JobStatus::decode(response.as_ref())?;
        if job.state != JobState::Queued as i32 && job.state != JobState::Running as i32 {
            return Ok(job);
        }

        if Instant::now() >= deadline {
            return Err(format!("training job timed out: {job:?}").into());
        }

        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
#[ignore = "run by scripts/check-e2e with the home Python environment"]
fn production_http_upload_trains_and_stages_exact_model() {
    let temporary = TempDir::new().expect("temporary acceptance root");
    let spool = temporary.path().join("spool");
    let model_root = temporary.path().join("models");
    let home_root = temporary.path().join("home");
    let first_run = make_complete_run(&spool, "run-e2e-a").expect("first complete Run fixture");
    let second_run = make_complete_run(&spool, "run-e2e-b").expect("second complete Run fixture");
    let input_runs = [first_run, second_run];
    let token_path = temporary.path().join("vehicle-token");
    fs::write(&token_path, "e2e-secret\n").expect("vehicle token");

    let listener = TcpListener::bind("127.0.0.1:0").expect("reserve local HTTP port");
    let port = listener.local_addr().expect("reserved address").port();
    drop(listener);
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root");
    let home_binary = std::env::var_os("CELERITY_HOME_BINARY")
        .map(PathBuf::from)
        .expect("scripts/check-e2e supplies the Rust home binary");
    // Preserve the virtualenv symlink: resolving it selects the base interpreter
    // and loses the installed numerical dependencies.
    let python = repository.join("services/home/.venv/bin/python");
    assert!(python.is_file(), "prepared Python training environment");
    let child = Command::new(home_binary)
        .args(["--bind", &format!("127.0.0.1:{port}")])
        .current_dir(repository)
        .env("CELERITY_HOME_STORAGE_ROOT", &home_root)
        .env("CELERITY_HOME_TOKEN_PATH", &token_path)
        .env("CELERITY_HOME_PYTHON", python)
        .env("CELERITY_HOME_WEBHOOK_URL", "https://127.0.0.1:9/webhook")
        .stdout(Stdio::null())
        .spawn()
        .expect("start production Rust home server");
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
        incomplete_retention_count: 2,
        home_interface: "test-home".to_owned(),
        expected_default_gateway: "127.0.0.1".to_owned(),
        home_api_url: base_url.clone(),
        credential_path: token_path.clone(),
        journal_path: temporary.path().join("sync.sqlite3"),
        model_root: model_root.clone(),
        model_abi: "thermal-v1".to_owned(),
        model_input_signals: vec![
            "coolant_temperature_c".to_owned(),
            "air_temperature_c".to_owned(),
        ],
        model_history_length: 4,
        model_command_lattice: vec![1000, 5000, 9000],
        model_maximum_calibration_error: 100.0,
    };
    let home = ReqwestHomeApi::new(&base_url, &token_path).expect("production home client");
    let mut synchronizer = Synchronizer::new(configuration).expect("synchronizer");
    assert_eq!(
        synchronizer
            .reconcile_once(&AlwaysHome, &home, None, None, &[])
            .expect("upload complete Run"),
        ReconcileOutcome::Synchronized {
            uploaded: 2,
            model_staged: false,
        }
    );

    let job_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let outcome = synchronizer
            .reconcile_once(&AlwaysHome, &home, None, None, &[])
            .expect("automatic training reconciliation");
        if outcome
            == (ReconcileOutcome::Synchronized {
                uploaded: 0,
                model_staged: true,
            })
        {
            break;
        }
        assert!(Instant::now() < job_deadline, "training job timed out");
        thread::sleep(Duration::from_millis(50));
    }
    assert!(model_root.join("desired-digest").is_file());
    assert!(model_root.join("slot-a/model.onnx").is_file());
    let desired_digest = fs::read_to_string(model_root.join("desired-digest"))
        .expect("desired digest")
        .trim()
        .to_owned();
    let slots = ModelSlots::open(&model_root).expect("startup model slots");
    let selected = slots
        .select_configured_startup("thermal-v1", 9)
        .expect("startup selection")
        .expect("staged startup model");
    assert_eq!(selected.digest, desired_digest);

    let runtime_bundle_path = temporary.path().join("runtime.toml");
    let runtime_bundle = fs::read_to_string(repository.join("config/examples/simulation.toml"))
        .expect("runtime bundle")
        .replacen("history_length = 1", "history_length = 4", 1)
        .replacen(
            "maximum_uncertainty = 0.1",
            "maximum_uncertainty = 100.0",
            1,
        );
    fs::write(&runtime_bundle_path, runtime_bundle).expect("runtime bundle fixture");
    let runtime_bundle = ValidatedBundle::load(&runtime_bundle_path, StartupMode::Simulation)
        .expect("validated runtime bundle");
    let model = RuntimeModel::load(&selected.onnx_path, &runtime_bundle)
        .expect("load exact staged graph with tract");
    let mut events = Vec::new();
    for index in 0_u32..4 {
        events.push(RuntimeEvent::ModelSignalSnapshot {
            monotonic_ms: u64::from(index),
            values: vec![88.0 - f64::from(index), 44.0 - f64::from(index) / 2.0],
        });
    }
    events.extend([
        RuntimeEvent::InputSnapshot {
            monotonic_ms: 4,
            observed_monotonic_ms: 4,
            coolant_c: 85.0,
            iat_c: 42.5,
        },
        RuntimeEvent::ControllerTruth {
            monotonic_ms: 4,
            boot_session: 9,
            configuration_generation: 1,
            identity_matches: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 5,
            remaining_cycle_ns: 1_000_000_000,
        },
    ]);
    let outcome =
        Runtime::new_with_model(runtime_bundle, ExternalAdapters::simulation(events), model)
            .expect("runtime composition")
            .run();
    assert_eq!(outcome.command_source, CommandSource::ModelOptimized);
    assert!(outcome.effects.iter().any(|effect| matches!(
        effect,
        RuntimeEffect::SendCommand {
            radiator_split_basis_points: 1000 | 5000 | 9000,
            ..
        }
    )));

    let repeated = start_and_poll_training_job(&client, &base_url, &input_runs)
        .expect("repeat training reaches terminal state");
    assert_eq!(repeated.state, JobState::NoChange as i32, "{repeated:?}");
    assert_eq!(
        repeated.artifact_digest.as_deref(),
        Some(desired_digest.as_str())
    );

    let database = rusqlite::Connection::open(home_root.join("home.sqlite3"))
        .expect("home database for failure acceptance");
    let baseline_path: String = database
        .query_row(
            "SELECT path FROM models WHERE digest=?",
            [&desired_digest],
            |row| row.get(0),
        )
        .expect("staged baseline path");
    fs::write(baseline_path, b"not a model bundle").expect("corrupt baseline fixture");
    let failed = start_and_poll_training_job(&client, &base_url, &input_runs)
        .expect("corrupt baseline training reaches terminal state");
    assert_eq!(failed.state, JobState::Failed as i32, "{failed:?}");
    assert!(
        failed
            .terminal_summary
            .as_deref()
            .is_some_and(|summary| !summary.is_empty())
    );
    let still_desired: String = database
        .query_row(
            "SELECT value FROM settings WHERE key='desired_model_digest'",
            [],
            |row| row.get(0),
        )
        .expect("failed training preserves desired model");
    assert_eq!(still_desired, desired_digest);
}
