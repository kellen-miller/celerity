//! Durable admission, child self-claim, recovery and fenced terminal state.

use std::{collections::BTreeSet, fs::OpenOptions, path::Path, process::Command, sync::Arc};

use axum::{
    body::Bytes,
    extract::{Path as RoutePath, State},
    http::StatusCode,
    response::Response,
};
use celerity_proto::celerity::v1::{JobRequest, JobStarted, JobState, JobStatus, WebhookEventType};
use prost::Message;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use serde::Serialize;

use crate::{
    HomeConfig, HomeResult, HomeService,
    http::{ApiError, ApiResult, protobuf},
    notifications, storage, training,
};

#[derive(Serialize)]
struct MissingRuns {
    incomplete_runs: Vec<String>,
}

struct ActiveJob {
    id: String,
    pid: Option<u32>,
    identity: Option<String>,
}

/// A running row belongs to precisely this worker incarnation.
#[derive(Debug)]
pub(crate) struct WorkerOwnership {
    pub(crate) job_id: String,
    pub(crate) pid: u32,
    pub(crate) identity: String,
}

impl WorkerOwnership {
    pub(crate) fn is_current(&self, connection: &Connection) -> HomeResult<bool> {
        Ok(connection
            .query_row(
                "SELECT 1 FROM jobs WHERE id=? AND state='running' AND pid=? AND pid_start_ticks=?",
                params![self.job_id, self.pid, self.identity],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub(crate) fn fail(&self, database: &Path, summary: &str) -> HomeResult<()> {
        let mut connection = storage::connect(database)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = transaction.execute("UPDATE jobs SET state='failed', terminal_summary=? WHERE id=? AND state='running' AND pid=? AND pid_start_ticks=?", params![summary, self.job_id, self.pid, self.identity])?;
        if changed != 0 {
            let payload = notifications::webhook_event(
                &self.job_id,
                WebhookEventType::TrainingFailure,
                summary,
                Some(&self.job_id),
                None,
            );
            transaction.execute("INSERT OR IGNORE INTO notifications(event_id, job_id, payload_pb, state) VALUES (?, ?, ?, 'pending')", params![self.job_id, self.job_id, payload])?;
        }

        transaction.commit()?;
        Ok(())
    }
}

pub(crate) fn enqueue(
    transaction: &Transaction<'_>,
    storage_root: &Path,
    run_digests: &[String],
    recipe: &str,
) -> HomeResult<String> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let directory = storage_root.join("jobs");
    std::fs::create_dir_all(&directory)?;
    let stdout = directory.join(format!("{job_id}.stdout"));
    let stderr = directory.join(format!("{job_id}.stderr"));
    transaction.execute("INSERT INTO jobs(id, state, recipe, input_digests_json, stdout_path, stderr_path) VALUES (?, 'queued', ?, ?, ?, ?)", params![job_id, recipe, serde_json::to_string(run_digests)?, stdout.to_string_lossy(), stderr.to_string_lossy()])?;
    Ok(job_id)
}

pub(crate) async fn start_job(
    State(home): State<Arc<HomeService>>,
    data: Bytes,
) -> ApiResult<Response> {
    let mut request =
        JobRequest::decode(data.as_ref()).map_err(|_| ApiError::invalid("invalid job request"))?;
    if request.run_digests.is_empty() {
        return Err(ApiError::invalid("job requires at least one Run"));
    }

    if request.recipe != "causal-tcn-v1" {
        return Err(ApiError::invalid("unsupported recipe"));
    }

    home.recover_jobs().map_err(ApiError::internal)?;
    let mut connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if transaction
        .query_row(
            "SELECT 1 FROM jobs WHERE state IN ('queued', 'running') LIMIT 1",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        return Err(ApiError::conflict("another training job is active"));
    }

    let admitted: BTreeSet<String> = transaction
        .prepare("SELECT digest FROM runs")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    let missing: Vec<String> = request
        .run_digests
        .iter()
        .filter(|digest| !admitted.contains(*digest))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !missing.is_empty() {
        return Err(ApiError::conflict(serde_json::to_value(MissingRuns {
            incomplete_runs: missing,
        })?));
    }

    request.run_digests.sort();
    let job_id = enqueue(
        &transaction,
        &home.config.storage_root,
        &request.run_digests,
        &request.recipe,
    )
    .map_err(ApiError::internal)?;
    transaction.commit()?;
    launch(&home.config, &job_id).map_err(ApiError::internal)?;
    Ok(protobuf(
        &JobStarted {
            job_id,
            state: JobState::Running as i32,
        },
        StatusCode::ACCEPTED,
    ))
}

pub(crate) async fn get_job(
    State(home): State<Arc<HomeService>>,
    RoutePath(job_id): RoutePath<String>,
) -> ApiResult<Response> {
    let connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let job = connection
        .query_row("SELECT * FROM jobs WHERE id=?", [job_id], |row| {
            let state: String = row.get("state")?;
            let inputs: String = row.get("input_digests_json")?;
            let run_digests = serde_json::from_str(&inputs).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(JobStatus {
                id: row.get("id")?,
                state: match state.as_str() {
                    "queued" => JobState::Queued,
                    "running" => JobState::Running,
                    "completed" => JobState::Completed,
                    "no_change" => JobState::NoChange,
                    "rejected" => JobState::Rejected,
                    _ => JobState::Failed,
                } as i32,
                recipe: row.get("recipe")?,
                run_digests,
                pid: row.get("pid")?,
                stdout_path: row.get("stdout_path")?,
                stderr_path: row.get("stderr_path")?,
                artifact_digest: row.get("artifact_digest")?,
                terminal_summary: row.get("terminal_summary")?,
                created_at: row.get("created_at")?,
            })
        })
        .optional()?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "job not found"))?;
    Ok(protobuf(&job, StatusCode::OK))
}

/// Launch a durable worker; a reaper owns the child handle, never cancellation.
/// The worker claims its own row before invoking Python.
///
/// # Errors
/// Returns log opening, SQLite or executable launch failures. A failed launch
/// leaves the queued row available for explicit startup/request recovery.
pub fn launch(config: &HomeConfig, job_id: &str) -> HomeResult<()> {
    let connection = storage::connect(&config.storage_root.join("home.sqlite3"))?;
    let paths: Option<(String, String)> = connection
        .query_row(
            "SELECT stdout_path, stderr_path FROM jobs WHERE id=? AND state='queued'",
            [job_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((stdout, stderr)) = paths else {
        return Ok(());
    };
    let stdout = OpenOptions::new().append(true).create(true).open(stdout)?;
    let stderr = OpenOptions::new().append(true).create(true).open(stderr)?;
    let mut child = Command::new(&config.worker_binary)
        .arg("worker")
        .arg("--storage-root")
        .arg(&config.storage_root)
        .arg("--job-id")
        .arg(job_id)
        .arg("--python")
        .arg(&config.python)
        .stdin(std::process::Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()?;
    std::thread::spawn(move || {
        if let Err(error) = child.wait() {
            eprintln!("worker reap failed: {error}");
        }
    });
    Ok(())
}

/// Recover only dead owned workers, then launch queued work.
///
/// # Errors
/// Returns SQLite and worker launch failures.
pub fn recover(config: &HomeConfig) -> HomeResult<usize> {
    let database = config.storage_root.join("home.sqlite3");
    let mut connection = storage::connect(&database)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let running = transaction
        .prepare("SELECT id, pid, pid_start_ticks FROM jobs WHERE state='running'")?
        .query_map([], |row| {
            Ok(ActiveJob {
                id: row.get(0)?,
                pid: row.get(1)?,
                identity: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for job in running {
        if job
            .pid
            .and_then(process_identity)
            .as_ref()
            .zip(job.identity.as_ref())
            .is_some_and(|(actual, expected)| actual == expected)
        {
            continue;
        }

        let summary = "managed training worker exited before recording a terminal state";
        let changed = transaction.execute("UPDATE jobs SET state='failed', terminal_summary=? WHERE id=? AND state='running' AND pid IS ? AND pid_start_ticks IS ?", params![summary, job.id, job.pid, job.identity])?;
        if changed != 0 {
            let event_id = format!("orphan:{}", job.id);
            let payload = notifications::webhook_event(
                &event_id,
                WebhookEventType::TrainingFailure,
                summary,
                Some(&job.id),
                None,
            );
            transaction.execute("INSERT OR IGNORE INTO notifications(event_id, job_id, payload_pb, state) VALUES (?, ?, ?, 'pending')", params![event_id, job.id, payload])?;
        }
    }

    let queued = transaction
        .prepare("SELECT id FROM jobs WHERE state='queued' ORDER BY created_at, id")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    transaction.commit()?;
    for job_id in &queued {
        launch(config, job_id)?;
    }

    Ok(queued.len())
}

pub(crate) fn process_identity(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
        if fields.first().is_some_and(|state| *state == "Z") {
            return None;
        }

        fields.get(19).map(|ticks| (*ticks).to_owned())
    }

    #[cfg(not(target_os = "linux"))]
    {
        let output = Command::new("ps")
            .arg("-o")
            .arg("lstart=")
            .arg("-p")
            .arg(pid.to_string())
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }

        let identity = String::from_utf8(output.stdout).ok()?.trim().to_owned();
        (!identity.is_empty()).then_some(identity)
    }
}

pub(crate) fn claim(storage_root: &Path, job_id: &str) -> HomeResult<Option<WorkerOwnership>> {
    let pid = std::process::id();
    let identity = process_identity(pid).ok_or("cannot establish worker process identity")?;
    let mut connection = storage::connect(&storage_root.join("home.sqlite3"))?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = transaction.execute(
        "UPDATE jobs SET state='running', pid=?, pid_start_ticks=? WHERE id=? AND state='queued'",
        params![pid, identity, job_id],
    )?;
    transaction.commit()?;
    Ok((changed != 0).then(|| WorkerOwnership {
        job_id: job_id.into(),
        pid,
        identity,
    }))
}

/// Supervise one Python adapter as the sole owner of a queued job.
///
/// # Errors
/// Returns training/validation failures after persisting a fenced failed state.
/// Duplicate launches return successfully without training.
pub fn run_worker(storage_root: &Path, job_id: &str, python: &Path) -> HomeResult<()> {
    let Some(owner) = claim(storage_root, job_id)? else {
        return Ok(());
    };
    if let Err(error) = training::execute(storage_root, python, &owner) {
        eprintln!("training job {job_id} failed: {error}");
        owner.fail(&storage_root.join("home.sqlite3"), &error.to_string())?;
        return Err(error);
    }

    Ok(())
}
