use std::{io::Write, path::Path, process::Command};

use celerity_proto::celerity::v1::{
    RunManifest, TrainingOutcome, TrainingRequest, TrainingResult, WebhookEventType,
};
use prost::Message;
use rusqlite::TransactionBehavior;

use celerity_sql::home as sql;

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

    let inputs = sql::GetJobInputs::builder()
        .job_id(&owner.job_id)
        .build()
        .query_one(&transaction)?
        .input_digests_json;
    let run_digests: Vec<String> = serde_json::from_str(&inputs)?;
    let mut manifests = Vec::new();
    for digest in &run_digests {
        let bytes = sql::GetRun::builder()
            .digest(digest)
            .build()
            .query_one(&transaction)?
            .manifest_pb;
        manifests.push(RunManifest::decode(bytes.as_slice())?);
    }

    let contract = ModelContract::from(manifests.first().ok_or("job requires at least one Run")?);
    contract.validate()?;
    if manifests
        .iter()
        .any(|manifest| ModelContract::from(manifest) != contract)
    {
        return Err("Run model contracts are absent or incompatible".into());
    }

    let baseline_digest = sql::GetDesiredModel
        .query_opt(&transaction)?
        .map(|row| row.value);
    let baseline_path = if let Some(digest) = &baseline_digest {
        let path = sql::GetStagedModel::builder()
            .digest(digest)
            .build()
            .query_opt(&transaction)?
            .map(|row| row.path)
            .ok_or("desired baseline model is absent or not staged")?;
        Some(path)
    } else {
        None
    };
    transaction.commit()?;
    Ok(TrainingSnapshot {
        request: TrainingRequest {
            schema_version: 1,
            storage_root: storage_root
                .to_str()
                .ok_or("training storage root is not UTF-8")?
                .into(),
            run_digests,
            manifests,
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
    let request_path = output.join("request.pb");
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&request_path)?;
    file.write_all(&snapshot.request.encode_to_vec())?;
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

    let result = TrainingResult::decode(std::fs::read(output.join("result.pb"))?.as_slice())?;
    if result.schema_version != 1 {
        return Err("unsupported training result schema_version".into());
    }

    let candidate = match TrainingOutcome::try_from(result.outcome)
        .map_err(|_| "unknown training result outcome")?
    {
        TrainingOutcome::Unspecified => {
            return Err("training result outcome is unspecified".into());
        }

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

    let desired = sql::GetDesiredModel
        .query_opt(&transaction)?
        .map(|row| row.value);
    let mut state = "no_change";
    let mut summary = result.summary;
    let mut artifact_digest = None;
    if desired != snapshot.baseline_digest {
        summary = "baseline changed during training".into();
    } else if let Some(candidate) = candidate {
        let existing = sql::HasModel::builder()
            .digest(&candidate.digest)
            .build()
            .query_opt(&transaction)?
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
            sql::InsertStagedModel::builder()
                .digest(&candidate.digest)
                .path(&candidate.path.to_string_lossy())
                .build()
                .execute(&transaction)?;
            sql::SetDesiredModel::builder()
                .digest(&candidate.digest)
                .build()
                .execute(&transaction)?;
        } else if state == "rejected" {
            sql::InsertRejectedModel::builder()
                .digest(&candidate.digest)
                .path(&candidate.path.to_string_lossy())
                .build()
                .execute(&transaction)?;
            let payload = notifications::webhook_event(
                &owner.job_id,
                WebhookEventType::CandidateRejection,
                "recipe causal-tcn-v1 rejected",
                Some(&owner.job_id),
                Some(&candidate.digest),
            );
            sql::InsertNotification::builder()
                .event_id(&owner.job_id)
                .job_id(Some(&owner.job_id))
                .payload(&payload)
                .build()
                .execute(&transaction)?;
        }

        artifact_digest = Some(candidate.digest);
        summary = format!("model {state}");
    }

    // The transaction's ownership check fences every model/settings/outbox write;
    // the update repeats it so terminal state remains an explicit ownership gate.
    sql::CompleteOwnedJob::builder()
        .state(state)
        .artifact_digest(artifact_digest.as_deref())
        .summary(Some(&summary))
        .job_id(&owner.job_id)
        .pid(Some(i64::from(owner.pid)))
        .identity(Some(&owner.identity))
        .build()
        .execute(&transaction)?;
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
