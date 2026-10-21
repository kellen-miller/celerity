use std::{
    path::{Path, PathBuf},
    process::Command,
};

use celerity_proto::celerity::v1::{RunManifest, WebhookEventType};
use prost::Message;
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use crate::{HomeResult, artifact::Candidate, jobs::WorkerOwnership, notifications, storage};

#[derive(Debug, PartialEq)]
pub(crate) struct ModelContract {
    pub(crate) model_abi: String,
    pub(crate) input_signals: Vec<String>,
    pub(crate) history_length: u64,
    pub(crate) sample_period_ms: u64,
    pub(crate) command_lattice: Vec<u32>,
    pub(crate) maximum_calibration_error: f64,
}

impl From<&RunManifest> for ModelContract {
    fn from(manifest: &RunManifest) -> Self {
        Self {
            model_abi: manifest.model_abi.clone(),
            input_signals: manifest.model_input_signals.clone(),
            history_length: manifest.model_history_length,
            sample_period_ms: manifest.sample_period_ms,
            command_lattice: manifest.command_lattice.clone(),
            maximum_calibration_error: manifest.maximum_calibration_error,
        }
    }
}

impl ModelContract {
    fn validate(&self) -> HomeResult<()> {
        if self.model_abi.is_empty()
            || self.input_signals.is_empty()
            || self.history_length == 0
            || self.sample_period_ms == 0
            || self.command_lattice.is_empty()
            || !self.maximum_calibration_error.is_finite()
            || self.maximum_calibration_error <= 0.0
        {
            return Err("Run model contracts are absent or incompatible".into());
        }

        Ok(())
    }
}

#[derive(Serialize)]
struct TrainingRequest {
    schema_version: u32,
    storage_root: PathBuf,
    run_digests: Vec<String>,
    manifests_hex: Vec<String>,
    baseline_path: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum TrainingOutcome {
    Candidate,
    NoChange,
}

#[derive(Deserialize)]
pub(crate) struct TrainingResult {
    schema_version: u32,
    outcome: TrainingOutcome,
    summary: String,
    pub(crate) bundle_digest: Option<String>,
    pub(crate) maximum_parity_error: Option<f64>,
    pub(crate) held_out_mse: Option<f64>,
    baseline_mse: Option<f64>,
}

struct TrainingSnapshot {
    request: TrainingRequest,
    baseline_digest: Option<String>,
    contract: ModelContract,
}

/// Preserve rejection-before-no-change ordering of the recipe policy.
fn classify_evaluation(
    parity: f64,
    mse: f64,
    baseline: Option<f64>,
    maximum_calibration: f64,
    already_staged: bool,
) -> &'static str {
    if !parity.is_finite()
        || !(0.0..=1e-4).contains(&parity)
        || !mse.is_finite()
        || mse < 0.0
        || mse.sqrt() > maximum_calibration
        || baseline.is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return "rejected";
    }

    if already_staged
        || baseline.is_some_and(|value| mse >= value - 1e-9_f64.max(value.abs() * 1e-6))
    {
        return "no_change";
    }

    "completed"
}

fn snapshot(storage_root: &Path, owner: &WorkerOwnership) -> HomeResult<TrainingSnapshot> {
    let mut connection = storage::connect(&storage_root.join("home.sqlite3"))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !owner.is_current(&transaction)? {
        return Err("worker ownership was superseded".into());
    }

    let inputs: String = transaction.query_row(
        "SELECT input_digests_json FROM jobs WHERE id=?",
        [&owner.job_id],
        |row| row.get(0),
    )?;
    let run_digests: Vec<String> = serde_json::from_str(&inputs)?;
    let mut manifests = Vec::new();
    let mut manifests_hex = Vec::new();
    for digest in &run_digests {
        let bytes: Vec<u8> = transaction.query_row(
            "SELECT manifest_pb FROM runs WHERE digest=?",
            [digest],
            |row| row.get(0),
        )?;
        manifests.push(RunManifest::decode(bytes.as_slice())?);
        manifests_hex.push(hex::encode(bytes));
    }

    let contract = ModelContract::from(manifests.first().ok_or("job requires at least one Run")?);
    contract.validate()?;
    if manifests
        .iter()
        .any(|manifest| ModelContract::from(manifest) != contract)
    {
        return Err("Run model contracts are absent or incompatible".into());
    }

    let baseline_digest: Option<String> = transaction
        .query_row(
            "SELECT value FROM settings WHERE key='desired_model_digest'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let baseline_path = if let Some(digest) = &baseline_digest {
        let path: String = transaction
            .query_row(
                "SELECT path FROM models WHERE digest=? AND state='staged'",
                [digest],
                |row| row.get(0),
            )
            .optional()?
            .ok_or("desired baseline model is absent or not staged")?;
        Some(PathBuf::from(path))
    } else {
        None
    };
    transaction.commit()?;
    Ok(TrainingSnapshot {
        request: TrainingRequest {
            schema_version: 1,
            storage_root: storage_root.to_path_buf(),
            run_digests,
            manifests_hex,
            baseline_path,
        },
        baseline_digest,
        contract,
    })
}

pub(crate) fn execute(
    storage_root: &Path,
    python: &Path,
    worker_binary: &Path,
    owner: &WorkerOwnership,
) -> HomeResult<()> {
    // The snapshot precedes Python execution. Completion compares the same
    // desired digest inside its write transaction, preserving concurrent rollback.
    let snapshot = snapshot(storage_root, owner)?;
    if owner.job_id.is_empty()
        || Path::new(&owner.job_id).components().count() != 1
        || owner.job_id == "."
        || owner.job_id == ".."
    {
        return Err("invalid durable job id".into());
    }

    let output = storage_root.join("jobs").join(&owner.job_id);
    std::fs::create_dir_all(&output)?;
    let request_path = output.join("request.json");
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&request_path)?;
    serde_json::to_writer(&mut file, &snapshot.request)?;
    file.sync_all()?;
    drop(file);
    let status = Command::new(worker_binary)
        .arg("trainer")
        .arg(storage_root)
        .arg(&owner.job_id)
        .arg(python)
        .arg(owner.pid.to_string())
        .arg(&owner.identity)
        .status()?;
    if !status.success() {
        return Err(format!("Python training adapter exited with {status}").into());
    }

    let result: TrainingResult =
        serde_json::from_slice(&std::fs::read(output.join("result.json"))?)?;
    if result.schema_version != 1 {
        return Err("unsupported training result schema_version".into());
    }

    let candidate = match result.outcome {
        TrainingOutcome::NoChange => {
            if result.bundle_digest.is_some() {
                return Err("no_change result unexpectedly includes an artifact".into());
            }

            None
        }
        TrainingOutcome::Candidate => {
            if result.baseline_mse.is_some() != snapshot.baseline_digest.is_some() {
                return Err("candidate baseline evaluation is absent or unexpected".into());
            }

            Some(Candidate::read(
                &output,
                &result,
                &snapshot.contract,
                &snapshot.request.run_digests,
            )?)
        }
    };
    let mut connection = storage::connect(&storage_root.join("home.sqlite3"))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if !owner.is_current(&transaction)? {
        return Ok(());
    }

    let desired: Option<String> = transaction
        .query_row(
            "SELECT value FROM settings WHERE key='desired_model_digest'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    let mut state = "no_change";
    let mut summary = result.summary;
    let mut artifact_digest = None;
    if desired != snapshot.baseline_digest {
        summary = "baseline changed during training".into();
    } else if let Some(candidate) = candidate {
        let existing = transaction
            .query_row(
                "SELECT 1 FROM models WHERE digest=?",
                [&candidate.digest],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        state = classify_evaluation(
            result
                .maximum_parity_error
                .ok_or("candidate parity is absent")?,
            result.held_out_mse.ok_or("candidate MSE is absent")?,
            result.baseline_mse,
            snapshot.contract.maximum_calibration_error,
            existing,
        );
        if state == "completed" {
            transaction.execute(
                "INSERT INTO models(digest, path, state) VALUES (?, ?, 'staged')",
                params![candidate.digest, candidate.path.to_string_lossy()],
            )?;
            transaction.execute("INSERT INTO settings(key, value) VALUES ('desired_model_digest', ?) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [&candidate.digest])?;
        } else if state == "rejected" {
            transaction.execute(
                "INSERT OR REPLACE INTO models(digest, path, state) VALUES (?, ?, 'rejected')",
                params![candidate.digest, candidate.path.to_string_lossy()],
            )?;
            let payload = notifications::webhook_event(
                &owner.job_id,
                WebhookEventType::CandidateRejection,
                "recipe causal-tcn-v1 rejected",
                Some(&owner.job_id),
                Some(&candidate.digest),
            );
            transaction.execute("INSERT OR IGNORE INTO notifications(event_id, job_id, payload_pb, state) VALUES (?, ?, ?, 'pending')", params![owner.job_id, owner.job_id, payload])?;
        }

        artifact_digest = Some(candidate.digest);
        summary = format!("model {state}");
    }

    // The transaction's ownership check fences every model/settings/outbox write;
    // the update repeats it so terminal state remains an explicit ownership gate.
    transaction.execute("UPDATE jobs SET state=?, artifact_digest=?, terminal_summary=? WHERE id=? AND state='running' AND pid=? AND pid_start_ticks=?", params![state, artifact_digest, summary, owner.job_id, owner.pid, owner.identity])?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::classify_evaluation;

    #[test]
    fn policy_rejects_invalid_metrics_before_duplicate_check() {
        assert_eq!(
            classify_evaluation(f64::NAN, 0.0, None, 1.0, true),
            "rejected"
        );
        assert_eq!(classify_evaluation(0.0, -1.0, None, 1.0, false), "rejected");
        assert_eq!(
            classify_evaluation(0.0, 0.0, Some(f64::INFINITY), 1.0, false),
            "rejected"
        );
        assert_eq!(classify_evaluation(2e-4, 0.0, None, 1.0, false), "rejected");
        assert_eq!(classify_evaluation(0.0, 4.0, None, 1.0, false), "rejected");
    }

    #[test]
    fn policy_requires_material_improvement() {
        assert_eq!(
            classify_evaluation(1e-4, 0.0, None, 1.0, false),
            "completed"
        );
        assert_eq!(classify_evaluation(0.0, 0.0, None, 1.0, true), "no_change");
        assert_eq!(
            classify_evaluation(0.0, 1.0 - 1e-7, Some(1.0), 1.0, false),
            "no_change"
        );
        assert_eq!(
            classify_evaluation(0.0, 1.0 - 1e-5, Some(1.0), 1.0, false),
            "completed"
        );
    }
}
