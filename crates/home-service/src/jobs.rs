//! Durable admission, child self-claim, recovery and fenced terminal state.

use std::{collections::BTreeSet, fs::OpenOptions, path::Path, process::Command};

use celerity_proto::celerity::v1::{JobRequest, JobStarted, JobState, JobStatus, WebhookEventType};
use rusqlite::{Connection, Transaction, TransactionBehavior};

use celerity_sql::home as sql;

use crate::{
    HomeConfig, HomeResult, HomeService, notifications,
    rpc::{ApiError, ApiResult},
    storage, training,
};

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
        Ok(sql::CheckJobOwner::builder()
            .job_id(&self.job_id)
            .pid(Some(i64::from(self.pid)))
            .identity(Some(&self.identity))
            .build()
            .query_opt(connection)?
            .is_some())
    }

    pub(crate) fn fail(&self, database: &Path, summary: &str) -> HomeResult<()> {
        let mut connection = storage::connect(database)?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = sql::FailOwnedJob::builder()
            .summary(Some(summary))
            .job_id(&self.job_id)
            .pid(Some(i64::from(self.pid)))
            .identity(Some(&self.identity))
            .build()
            .execute(&transaction)?;
        if changed != 0 {
            let payload = notifications::webhook_event(
                &self.job_id,
                WebhookEventType::TrainingFailure,
                summary,
                Some(&self.job_id),
                None,
            );
            sql::InsertNotification::builder()
                .event_id(&self.job_id)
                .job_id(Some(&self.job_id))
                .payload(&payload)
                .build()
                .execute(&transaction)?;
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
    sql::EnqueueJob::builder()
        .job_id(&job_id)
        .recipe(recipe)
        .inputs(&serde_json::to_string(run_digests)?)
        .stdout(Some(&stdout.to_string_lossy()))
        .stderr(Some(&stderr.to_string_lossy()))
        .build()
        .execute(transaction)?;
    Ok(job_id)
}

pub(crate) fn start_job(home: &HomeService, mut request: JobRequest) -> ApiResult<JobStarted> {
    if request.run_digests.is_empty() {
        return Err(ApiError::invalid("job requires at least one Run"));
    }

    if request.recipe != "causal-tcn-v1" {
        return Err(ApiError::invalid("unsupported recipe"));
    }

    home.recover_jobs().map_err(ApiError::internal)?;
    let mut connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if sql::HasActiveJob.query_opt(&transaction)?.is_some() {
        return Err(ApiError::conflict("another training job is active"));
    }

    let admitted: BTreeSet<String> = sql::ListRunDigests
        .query_many(&transaction)?
        .into_iter()
        .map(|row| row.digest)
        .collect();
    let missing: Vec<String> = request
        .run_digests
        .iter()
        .filter(|digest| !admitted.contains(*digest))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !missing.is_empty() {
        return Err(ApiError::incomplete_runs(missing));
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
    Ok(JobStarted {
        job_id,
        state: JobState::Running as i32,
    })
}

pub(crate) fn get_job(home: &HomeService, job_id: String) -> ApiResult<JobStatus> {
    let connection = storage::connect(&home.database()).map_err(ApiError::internal)?;
    let row = sql::GetJob::builder()
        .job_id(&job_id)
        .build()
        .query_opt(&connection)?
        .ok_or_else(|| ApiError::new(tonic::Code::NotFound, "job not found"))?;
    let job = JobStatus {
        id: row.id,
        state: match row.state.as_str() {
            "queued" => JobState::Queued,
            "running" => JobState::Running,
            "completed" => JobState::Completed,
            "no_change" => JobState::NoChange,
            "rejected" => JobState::Rejected,
            _ => JobState::Failed,
        } as i32,
        recipe: row.recipe,
        run_digests: serde_json::from_str(&row.input_digests_json)?,
        pid: row.pid,
        stdout_path: row.stdout_path,
        stderr_path: row.stderr_path,
        artifact_digest: row.artifact_digest,
        terminal_summary: row.terminal_summary,
        created_at: row.created_at,
    };
    Ok(job)
}

/// Launch a durable worker; a reaper owns the child handle, never cancellation.
/// The worker claims its own row before invoking Python.
///
/// # Errors
/// Returns log opening, SQLite or executable launch failures. A failed launch
/// leaves the queued row available for explicit startup/request recovery.
pub fn launch(config: &HomeConfig, job_id: &str) -> HomeResult<()> {
    let connection = storage::connect(&config.storage_root.join("home.sqlite3"))?;
    let Some(paths) = sql::QueuedJobPaths::builder()
        .job_id(job_id)
        .build()
        .query_opt(&connection)?
    else {
        return Ok(());
    };
    let stdout = paths
        .stdout_path
        .ok_or("queued job stdout path is absent")?;
    let stderr = paths
        .stderr_path
        .ok_or("queued job stderr path is absent")?;
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
    let running = sql::ListRunningJobs
        .query_many(&transaction)?
        .into_iter()
        .map(|row| {
            Ok(ActiveJob {
                id: row.id,
                pid: row.pid.map(u32::try_from).transpose()?,
                identity: row.pid_start_ticks,
            })
        })
        .collect::<HomeResult<Vec<_>>>()?;
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

        // Python inherits this lock across exec. Keep admission occupied until
        // numerical work exits, even if its Rust supervisor was killed.
        let training_lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(
                config
                    .storage_root
                    .join("jobs")
                    .join(format!("{}.training.lock", job.id)),
            )?;
        match training_lock.try_lock() {
            Ok(()) => (),
            Err(std::fs::TryLockError::WouldBlock) => continue,
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }

        let summary = "managed training worker exited before recording a terminal state";
        let changed = sql::FailOrphanedJob::builder()
            .summary(Some(summary))
            .job_id(&job.id)
            .pid(job.pid.map(i64::from))
            .identity(job.identity.as_deref())
            .build()
            .execute(&transaction)?;
        if changed != 0 {
            let event_id = format!("orphan:{}", job.id);
            let payload = notifications::webhook_event(
                &event_id,
                WebhookEventType::TrainingFailure,
                summary,
                Some(&job.id),
                None,
            );
            sql::InsertNotification::builder()
                .event_id(&event_id)
                .job_id(Some(&job.id))
                .payload(&payload)
                .build()
                .execute(&transaction)?;
        }
    }

    let queued = sql::ListQueuedJobs.query_many(&transaction)?;
    transaction.commit()?;
    for job in &queued {
        launch(config, &job.id)?;
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
    let changed = sql::ClaimJob::builder()
        .pid(Some(i64::from(pid)))
        .identity(Some(&identity))
        .job_id(job_id)
        .build()
        .execute(&transaction)?;
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
pub fn run_worker(
    storage_root: &Path,
    job_id: &str,
    python: &Path,
    worker_binary: &Path,
) -> HomeResult<()> {
    let Some(owner) = claim(storage_root, job_id)? else {
        return Ok(());
    };
    if let Err(error) = training::execute(storage_root, python, worker_binary, &owner) {
        eprintln!("training job {job_id} failed: {error}");
        owner.fail(&storage_root.join("home.sqlite3"), &error.to_string())?;
        return Err(error);
    }

    Ok(())
}

/// Replace this process with Python while retaining the training admission lock.
///
/// # Errors
/// Returns lock, ownership, unsupported-platform or executable launch failures.
pub fn run_trainer(
    storage_root: &Path,
    job_id: &str,
    python: &Path,
    pid: u32,
    identity: String,
) -> HomeResult<()> {
    #[cfg(unix)]
    {
        use nix::fcntl::{FcntlArg, FdFlag, fcntl};
        use std::os::unix::process::CommandExt;

        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(
                storage_root
                    .join("jobs")
                    .join(format!("{job_id}.training.lock")),
            )?;
        lock.try_lock()?;
        let owner = WorkerOwnership {
            job_id: job_id.into(),
            pid,
            identity,
        };
        let connection = storage::connect(&storage_root.join("home.sqlite3"))?;
        if !owner.is_current(&connection)?
            || process_identity(pid).as_ref() != Some(&owner.identity)
        {
            return Err("training supervisor was superseded or exited".into());
        }

        drop(connection);
        // Clear CLOEXEC only for the admission lock. Python owns the same open
        // file description until exit, including supervisor SIGKILL windows.
        fcntl(&lock, FcntlArg::F_SETFD(FdFlag::empty()))?;
        Err(Command::new(python)
            .args(["-m", "celerity_home.worker", "--request"])
            .arg(storage_root.join("jobs").join(job_id).join("request.pb"))
            .exec()
            .into())
    }

    #[cfg(not(unix))]
    Err("home training requires Unix process ownership".into())
}
