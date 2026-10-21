#![cfg(unix)]

use std::{
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use celerity_proto::celerity::v1::home_server::Home;
use celerity_proto::celerity::v1::{JobRequest, RunManifest, TrainingOutcome, TrainingResult};
use home_service::{HomeConfig, HomeResult, HomeService, storage};
use prost::Message;
use tonic::{Code, Request};

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

#[tokio::test]
async fn numerical_child_retains_admission_after_supervisor_sigkill() -> HomeResult<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let python = root.join("blocking-adapter");
    // The fixture uses only the shell: its process retains the exact inherited
    // descriptor that the production Python interpreter receives.
    std::fs::write(
        &python,
        "#!/bin/sh\nfor request; do :; done\noutput=${request%/*}\necho $$ > \"$output/pid\"\nwhile [ ! -f \"$output/release\" ]; do sleep 0.01; done\ncp \"$output/../../fixture-result.pb\" \"$output/result.pb.staging\"\nmv \"$output/result.pb.staging\" \"$output/result.pb\"\n",
    )?;
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755))?;
    std::fs::write(
        root.join("fixture-result.pb"),
        TrainingResult {
            schema_version: 1,
            outcome: TrainingOutcome::NoChange as i32,
            summary: "fixture".into(),
            ..TrainingResult::default()
        }
        .encode_to_vec(),
    )?;
    let home = HomeService::open(HomeConfig {
        storage_root: root.to_path_buf(),
        vehicle_token: "secret".into(),
        webhook_url: "https://127.0.0.1:1/webhook".into(),
        python,
        worker_binary: env!("CARGO_BIN_EXE_celerity-home").into(),
    })?;
    let manifest = RunManifest {
        model_abi: "thermal-v1".into(),
        model_input_signals: vec!["coolant".into()],
        model_history_length: 4,
        sample_period_ms: 20,
        command_lattice: vec![1000],
        maximum_calibration_error: 100.0,
        ..RunManifest::default()
    };
    let connection = storage::connect(&home.database())?;
    connection.execute(
        "INSERT INTO runs(digest, manifest_pb) VALUES ('run', ?)",
        [manifest.encode_to_vec()],
    )?;
    std::fs::create_dir_all(root.join("jobs"))?;
    connection.execute("INSERT INTO jobs(id, state, recipe, input_digests_json) VALUES ('killed', 'queued', 'causal-tcn-v1', '[\"run\"]')", [])?;
    let mut supervisor = Command::new(&home.config.worker_binary)
        .args(["worker", "--storage-root"])
        .arg(root)
        .args(["--job-id", "killed", "--python"])
        .arg(&home.config.python)
        .spawn()?;
    let output = root.join("jobs/killed");
    wait_for(&output.join("pid"))?;
    supervisor.kill()?;
    supervisor.wait()?;
    home.recover_jobs()?;
    let state: String =
        connection.query_row("SELECT state FROM jobs WHERE id='killed'", [], |row| {
            row.get(0)
        })?;
    assert_eq!(
        state, "running",
        "live numerical work must retain admission"
    );
    let request = JobRequest {
        recipe: "causal-tcn-v1".into(),
        run_digests: vec!["run".into()],
    };
    let error = home
        .start_job(Request::new(request.clone()))
        .await
        .expect_err("live trainer owns admission");
    assert_eq!(error.code(), Code::FailedPrecondition);
    std::fs::write(output.join("release"), [])?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        home.recover_jobs()?;
        let state: String =
            connection.query_row("SELECT state FROM jobs WHERE id='killed'", [], |row| {
                row.get(0)
            })?;
        if state == "failed" {
            break;
        }

        assert!(Instant::now() < deadline, "numerical child did not exit");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    home.recover_jobs()?;
    let events: i64 = connection.query_row(
        "SELECT count(*) FROM notifications WHERE job_id='killed'",
        [],
        |row| row.get(0),
    )?;
    assert_eq!(events, 1);
    let job = home.start_job(Request::new(request)).await?.into_inner();
    let replacement = root.join("jobs").join(&job.job_id);
    wait_for(&replacement.join("pid"))?;
    assert!(output.join("result.pb").exists());
    std::fs::write(replacement.join("release"), [])?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state: String =
            connection.query_row("SELECT state FROM jobs WHERE id=?", [&job.job_id], |row| {
                row.get(0)
            })?;
        if state == "no_change" {
            break;
        }

        assert!(
            Instant::now() < deadline,
            "replacement trainer did not finish"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // A trainer delayed until after recovery cannot start numerical work.
    let status = Command::new(&home.config.worker_binary)
        .arg("trainer")
        .arg(root)
        .arg("killed")
        .arg(&home.config.python)
        .args(["4294967295", "absent"])
        .status()?;
    assert!(!status.success());
    let models: i64 = connection.query_row("SELECT count(*) FROM models", [], |row| row.get(0))?;
    assert_eq!(models, 0);
    Ok(())
}

#[test]
fn result_rejects_unspecified_unknown_and_unexpected_baseline() -> HomeResult<()> {
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let python = root.join("result-adapter");
    std::fs::write(
        &python,
        "#!/bin/sh\nfor request; do :; done\noutput=${request%/*}\ncp \"$output/../../fixture-result.pb\" \"$output/result.pb.staging\"\nmv \"$output/result.pb.staging\" \"$output/result.pb\"\n",
    )?;
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o755))?;
    let home = HomeService::open(HomeConfig {
        storage_root: root.to_path_buf(),
        vehicle_token: "secret".into(),
        webhook_url: "https://127.0.0.1:1/webhook".into(),
        python,
        worker_binary: env!("CARGO_BIN_EXE_celerity-home").into(),
    })?;
    let manifest = RunManifest {
        model_abi: "thermal-v1".into(),
        model_input_signals: vec!["coolant".into()],
        model_history_length: 4,
        sample_period_ms: 20,
        command_lattice: vec![1000],
        maximum_calibration_error: 100.0,
        ..RunManifest::default()
    };
    let connection = storage::connect(&home.database())?;
    connection.execute(
        "INSERT INTO runs(digest, manifest_pb) VALUES ('run', ?)",
        [manifest.encode_to_vec()],
    )?;
    for outcome in [
        TrainingOutcome::Unspecified as i32,
        99,
        TrainingOutcome::Candidate as i32,
    ] {
        let job_id = format!("invalid-{outcome}");
        let result = TrainingResult {
            schema_version: 1,
            outcome,
            baseline_mse: Some(0.0),
            ..TrainingResult::default()
        };
        std::fs::write(root.join("fixture-result.pb"), result.encode_to_vec())?;
        connection.execute("INSERT INTO jobs(id, state, recipe, input_digests_json) VALUES (?, 'queued', 'causal-tcn-v1', '[\"run\"]')", [&job_id])?;
        let output = Command::new(&home.config.worker_binary)
            .args(["worker", "--storage-root"])
            .arg(root)
            .arg("--job-id")
            .arg(&job_id)
            .arg("--python")
            .arg(&home.config.python)
            .output()?;
        assert!(!output.status.success());
        let summary: String = connection.query_row(
            "SELECT terminal_summary FROM jobs WHERE id=?",
            [&job_id],
            |row| row.get(0),
        )?;
        let expected = match outcome {
            0 => "training result outcome is unspecified",
            99 => "unknown training result outcome",
            _ => "candidate baseline evaluation is absent or unexpected",
        };
        assert!(summary.contains(expected), "{summary}");
    }

    let models: i64 = connection.query_row("SELECT count(*) FROM models", [], |row| row.get(0))?;
    assert_eq!(models, 0);
    Ok(())
}
