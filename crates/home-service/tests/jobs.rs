#![cfg(unix)]

use std::{
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use celerity_proto::celerity::v1::home_server::Home;
use celerity_proto::celerity::v1::{
    JobRequest, ModelBundleManifest, ModelCompatibility, ModelInputRange, ModelNormalization,
    ReconcileRequest, RunManifest, TrainingOutcome, TrainingRequest, TrainingResult, WebhookEvent,
    WebhookEventType,
};
use home_service::{HomeConfig, HomeResult, HomeService, storage};
use prost::Message;
use rusqlite::params;
use sha2::{Digest, Sha256};
use tonic::{Code, Request};

struct WorkerFixture {
    directory: tempfile::TempDir,
    home: HomeService,
}

impl WorkerFixture {
    fn new(adapter_body: &str) -> HomeResult<Self> {
        let directory = tempfile::tempdir()?;
        let python = directory.path().join("python-fixture");
        let executable = Command::new("python3")
            .args(["-c", "import sys; print(sys.executable)"])
            .output()?;
        if !executable.status.success() {
            return Err("Python fixture interpreter is unavailable".into());
        }

        let interpreter = String::from_utf8(executable.stdout)?;
        std::fs::write(
            &python,
            format!(
                "#!{}\nimport pathlib, shutil, sys, time\nrequest_path = pathlib.Path(sys.argv[-1])\nassert request_path.name == 'request.pb'\nassert request_path.read_bytes() == (request_path.parents[2] / 'expected-request.pb').read_bytes()\noutput = request_path.parent\nwith (output / 'invocations').open('a') as stream: stream.write('trained\\n')\n(output / 'started').touch()\n{adapter_body}\n",
                interpreter.trim()
            ),
        )?;
        std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755))?;
        let home = HomeService::open(HomeConfig {
            storage_root: directory.path().to_path_buf(),
            vehicle_token: "secret".into(),
            webhook_url: "https://127.0.0.1:1/webhook".into(),
            python,
            worker_binary: PathBuf::from(env!("CARGO_BIN_EXE_celerity-home")),
        })?;
        let manifest = RunManifest {
            model_abi: "thermal-v1".into(),
            model_input_signals: vec!["coolant".into(), "air".into()],
            model_history_length: 4,
            sample_period_ms: 20,
            command_lattice: vec![1000, 5000, 9000],
            maximum_calibration_error: 100.0,
            ..RunManifest::default()
        };
        storage::connect(&home.database())?.execute(
            "INSERT INTO runs(digest, manifest_pb) VALUES ('run', ?)",
            [manifest.encode_to_vec()],
        )?;
        std::fs::write(
            directory.path().join("expected-request.pb"),
            TrainingRequest {
                schema_version: 1,
                storage_root: directory.path().to_string_lossy().into_owned(),
                run_digests: vec!["run".into()],
                manifests: vec![manifest],
                baseline_path: None,
            }
            .encode_to_vec(),
        )?;
        std::fs::write(
            directory.path().join("no-change.pb"),
            TrainingResult {
                schema_version: 1,
                outcome: TrainingOutcome::NoChange as i32,
                summary: "no eligible data".into(),
                ..TrainingResult::default()
            }
            .encode_to_vec(),
        )?;
        Ok(Self { directory, home })
    }

    fn enqueue(&self, job_id: &str) -> HomeResult<()> {
        let jobs = self.directory.path().join("jobs");
        std::fs::create_dir_all(&jobs)?;
        storage::connect(&self.home.database())?.execute("INSERT INTO jobs(id, state, recipe, input_digests_json, stdout_path, stderr_path) VALUES (?, 'queued', 'causal-tcn-v1', '[\"run\"]', ?, ?)", params![job_id, jobs.join(format!("{job_id}.stdout")).to_string_lossy(), jobs.join(format!("{job_id}.stderr")).to_string_lossy()])?;
        Ok(())
    }

    fn spawn(&self, job_id: &str) -> HomeResult<Child> {
        Ok(Command::new(&self.home.config.worker_binary)
            .arg("worker")
            .arg("--storage-root")
            .arg(self.directory.path())
            .arg("--job-id")
            .arg(job_id)
            .arg("--python")
            .arg(&self.home.config.python)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?)
    }

    fn output(&self, job_id: &str) -> PathBuf {
        self.directory.path().join("jobs").join(job_id)
    }

    fn state(&self, job_id: &str) -> HomeResult<String> {
        Ok(storage::connect(&self.home.database())?.query_row(
            "SELECT state FROM jobs WHERE id=?",
            [job_id],
            |row| row.get(0),
        )?)
    }

    fn candidate(&self, job_id: &str, parity: f64) -> HomeResult<String> {
        let output = self.output(job_id);
        std::fs::create_dir_all(&output)?;
        let model = b"fixture ONNX bytes; numeric validity is Python's responsibility";
        let manifest = ModelBundleManifest {
            schema_version: 1,
            onnx_sha256: hex::encode(Sha256::digest(model)),
            signal_order: vec!["coolant".into(), "air".into()],
            units: vec!["native".into(), "native".into()],
            sample_period_ms: 20,
            history_length: 4,
            horizons: vec![1],
            output_order: vec!["coolant".into(), "post_intercooler_iat".into()],
            command_lattice: vec![1000, 5000, 9000],
            compatibility: Some(ModelCompatibility {
                model_abi: "thermal-v1".into(),
                input_shape: vec![1, 9],
                derivation_digest: "a".repeat(64),
                input_runs: vec!["run".into()],
                maximum_parity_error: parity,
                held_out_mse: 1.0,
            }),
            input_ranges: vec![
                ModelInputRange {
                    minimum: 0.0,
                    maximum: 100.0
                };
                2
            ],
            normalization: vec![
                ModelNormalization {
                    mean: 50.0,
                    scale: 10.0
                };
                2
            ],
            calibration_error: 1.0,
        };
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        archive.start_file("manifest.pb", options)?;
        archive.write_all(&manifest.encode_to_vec())?;
        archive.start_file("model.onnx", options)?;
        archive.write_all(model)?;
        let bytes = archive.finish()?.into_inner();
        let digest = hex::encode(Sha256::digest(&bytes));
        std::fs::write(output.join(format!("{digest}.zip")), bytes)?;
        std::fs::write(
            output.join("candidate-result.pb"),
            TrainingResult {
                schema_version: 1,
                outcome: TrainingOutcome::Candidate as i32,
                summary: "candidate".into(),
                bundle_digest: Some(digest.clone()),
                maximum_parity_error: Some(parity),
                held_out_mse: Some(1.0),
                baseline_mse: None,
            }
            .encode_to_vec(),
        )?;
        Ok(digest)
    }
}

fn wait_for(path: &Path) -> HomeResult<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(format!("timed out waiting for {}", path.display()).into());
        }

        std::thread::sleep(Duration::from_millis(10));
    }

    Ok(())
}

const NO_CHANGE: &str = "shutil.copyfile(request_path.parents[2] / 'no-change.pb', output / 'result.pb.staging'); (output / 'result.pb.staging').replace(output / 'result.pb')";
const WAIT: &str = "while not (output / 'release').exists(): time.sleep(0.01)";
const CANDIDATE: &str = "shutil.copyfile(output / 'candidate-result.pb', output / 'result.pb.staging'); (output / 'result.pb.staging').replace(output / 'result.pb')";

#[test]
fn duplicate_workers_self_claim_once_and_preserve_live_identity() -> HomeResult<()> {
    let fixture = WorkerFixture::new(&format!("{WAIT}\n{NO_CHANGE}"))?;
    fixture.enqueue("one-owner")?;
    assert_eq!(fixture.home.recover_jobs()?, 1);
    let mut duplicate = fixture.spawn("one-owner")?;
    let output = fixture.output("one-owner");
    wait_for(&output.join("started"))?;
    let connection = storage::connect(&fixture.home.database())?;
    let identity: String = connection.query_row(
        "SELECT pid_start_ticks FROM jobs WHERE id='one-owner'",
        [],
        |row| row.get(0),
    )?;
    assert_ne!(identity, "");
    fixture.home.recover_jobs()?;
    assert_eq!(fixture.state("one-owner")?, "running");
    std::fs::write(output.join("release"), [])?;
    assert!(duplicate.wait()?.success());
    let deadline = Instant::now() + Duration::from_secs(15);
    while fixture.state("one-owner")? == "running" && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }

    assert_eq!(fixture.state("one-owner")?, "no_change");
    assert_eq!(
        std::fs::read_to_string(output.join("invocations"))?,
        "trained\n"
    );
    assert_eq!(
        connection.query_row("SELECT count(*) FROM models", [], |row| row
            .get::<_, i64>(0))?,
        0
    );
    Ok(())
}

#[test]
fn worker_failure_is_fenced_and_orphan_recovery_uses_original_event_identity() -> HomeResult<()> {
    let fixture = WorkerFixture::new("raise RuntimeError('fixture training failed')")?;
    fixture.enqueue("failed")?;
    assert!(!fixture.spawn("failed")?.wait()?.success());
    assert_eq!(fixture.state("failed")?, "failed");
    let connection = storage::connect(&fixture.home.database())?;
    let payload: Vec<u8> = connection.query_row(
        "SELECT payload_pb FROM notifications WHERE event_id='failed'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(
        WebhookEvent::decode(payload.as_slice())?.event_type,
        WebhookEventType::TrainingFailure as i32
    );
    fixture.enqueue("orphan")?;
    connection.execute("UPDATE jobs SET state='running', pid=4294967295, pid_start_ticks='absent' WHERE id='orphan'", [])?;
    assert_eq!(fixture.home.recover_jobs()?, 0);
    fixture.home.recover_jobs()?;
    assert_eq!(fixture.state("orphan")?, "failed");
    assert_eq!(
        connection.query_row(
            "SELECT count(*) FROM notifications WHERE event_id='orphan:orphan'",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        1
    );
    let summary: String = connection.query_row(
        "SELECT terminal_summary FROM jobs WHERE id='orphan'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(
        summary,
        "managed training worker exited before recording a terminal state"
    );
    Ok(())
}

#[test]
fn superseded_worker_cannot_stage_models_or_emit_terminal_events() -> HomeResult<()> {
    for adapter in [CANDIDATE, "raise RuntimeError('stale failure')"] {
        let fixture = WorkerFixture::new(&format!("{WAIT}\n{adapter}"))?;
        fixture.enqueue("superseded")?;
        fixture.candidate("superseded", 0.0)?;
        let mut worker = fixture.spawn("superseded")?;
        let output = fixture.output("superseded");
        wait_for(&output.join("started"))?;
        let connection = storage::connect(&fixture.home.database())?;
        connection.execute("UPDATE jobs SET state='failed', terminal_summary='superseded', pid_start_ticks='different' WHERE id='superseded'", [])?;
        std::fs::write(output.join("release"), [])?;
        assert_eq!(worker.wait()?.success(), adapter == CANDIDATE);
        assert_eq!(fixture.state("superseded")?, "failed");
        assert_eq!(
            connection.query_row("SELECT count(*) FROM models", [], |row| row
                .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            connection.query_row("SELECT count(*) FROM notifications", [], |row| row
                .get::<_, i64>(0))?,
            0
        );
    }

    Ok(())
}

#[tokio::test]
async fn changed_baseline_cannot_be_undone_by_candidate_completion() -> HomeResult<()> {
    let fixture = WorkerFixture::new(&format!("{WAIT}\n{CANDIDATE}"))?;
    let connection = storage::connect(&fixture.home.database())?;
    for digest in ["old-baseline", "known-good"] {
        connection.execute(
            "INSERT INTO models(digest, path, state) VALUES (?, 'unused.zip', 'staged')",
            [digest],
        )?;
    }

    connection.execute(
        "INSERT INTO settings(key, value) VALUES ('desired_model_digest', 'old-baseline')",
        [],
    )?;
    fixture.enqueue("baseline-race")?;
    fixture.candidate("baseline-race", 0.0)?;
    let request_path = fixture.directory.path().join("expected-request.pb");
    let mut request = TrainingRequest::decode(std::fs::read(&request_path)?.as_slice())?;
    request.baseline_path = Some("unused.zip".into());
    std::fs::write(request_path, request.encode_to_vec())?;
    let result_path = fixture.output("baseline-race").join("candidate-result.pb");
    let mut result = TrainingResult::decode(std::fs::read(&result_path)?.as_slice())?;
    result.baseline_mse = Some(2.0);
    std::fs::write(result_path, result.encode_to_vec())?;
    let mut worker = fixture.spawn("baseline-race")?;
    let output = fixture.output("baseline-race");
    wait_for(&output.join("started"))?;
    let reconcile = ReconcileRequest {
        schema_version: 1,
        rejected_model_digests: vec!["old-baseline".into()],
        ..ReconcileRequest::default()
    };
    fixture.home.reconcile(Request::new(reconcile)).await?;
    std::fs::write(output.join("release"), [])?;
    assert!(worker.wait()?.success());
    assert_eq!(fixture.state("baseline-race")?, "no_change");
    let summary: String = connection.query_row(
        "SELECT terminal_summary FROM jobs WHERE id='baseline-race'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(summary, "baseline changed during training");
    assert_eq!(
        connection.query_row("SELECT value FROM settings", [], |row| row
            .get::<_, String>(0))?,
        "known-good"
    );
    assert_eq!(
        connection.query_row("SELECT count(*) FROM models", [], |row| row
            .get::<_, i64>(0))?,
        2
    );
    Ok(())
}

#[test]
fn parity_rejection_and_corrupt_bundle_do_not_promote() -> HomeResult<()> {
    let fixture = WorkerFixture::new(CANDIDATE)?;
    fixture.enqueue("rejected")?;
    fixture.candidate("rejected", 0.001)?;
    assert!(fixture.spawn("rejected")?.wait()?.success());
    assert_eq!(fixture.state("rejected")?, "rejected");
    let connection = storage::connect(&fixture.home.database())?;
    assert_eq!(
        connection.query_row("SELECT count(*) FROM settings", [], |row| row
            .get::<_, i64>(0))?,
        0
    );
    assert_eq!(
        connection.query_row("SELECT state FROM models", [], |row| row
            .get::<_, String>(0))?,
        "rejected"
    );
    let payload: Vec<u8> = connection.query_row(
        "SELECT payload_pb FROM notifications WHERE event_id='rejected'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(
        WebhookEvent::decode(payload.as_slice())?.event_type,
        WebhookEventType::CandidateRejection as i32
    );
    fixture.enqueue("corrupt")?;
    let digest = fixture.candidate("corrupt", 0.001)?;
    std::fs::write(
        fixture.output("corrupt").join(format!("{digest}.zip")),
        b"corrupt",
    )?;
    assert!(!fixture.spawn("corrupt")?.wait()?.success());
    assert_eq!(fixture.state("corrupt")?, "failed");
    assert_eq!(
        connection.query_row("SELECT count(*) FROM models", [], |row| row
            .get::<_, i64>(0))?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn admission_has_one_active_job_and_reconcile_suppresses_unchanged_corpus() -> HomeResult<()>
{
    let fixture = WorkerFixture::new(&format!("{WAIT}\n{NO_CHANGE}"))?;
    let request = JobRequest {
        recipe: "causal-tcn-v1".into(),
        run_digests: vec!["run".into()],
    };
    let started = fixture
        .home
        .start_job(Request::new(request.clone()))
        .await?
        .into_inner();
    let error = fixture
        .home
        .start_job(Request::new(request))
        .await
        .expect_err("active training job");
    assert_eq!(error.code(), Code::FailedPrecondition);
    let output = fixture.output(&started.job_id);
    wait_for(&output.join("started"))?;
    std::fs::write(output.join("release"), [])?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while fixture.state(&started.job_id)? == "running" && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(fixture.state(&started.job_id)?, "no_change");
    let reconcile = ReconcileRequest {
        schema_version: 1,
        completed_run_digests: vec!["run".into()],
        ..ReconcileRequest::default()
    };
    fixture.home.reconcile(Request::new(reconcile)).await?;
    assert_eq!(
        storage::connect(&fixture.home.database())?.query_row(
            "SELECT count(*) FROM jobs",
            [],
            |row| row.get::<_, i64>(0)
        )?,
        1
    );
    Ok(())
}
