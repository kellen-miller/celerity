//! SQLite connections and atomic Home schema migration.

use std::{path::Path, time::Duration};

use rusqlite::{Connection, TransactionBehavior};

use crate::{HomeResult, migration};

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS runs (
  digest TEXT PRIMARY KEY,
  manifest_pb BLOB NOT NULL,
  completed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS chunks (
  run_digest TEXT NOT NULL,
  digest TEXT NOT NULL,
  path TEXT NOT NULL,
  PRIMARY KEY(run_digest, digest)
);
CREATE TABLE IF NOT EXISTS models (
  digest TEXT PRIMARY KEY,
  path TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('staged', 'rejected')),
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS jobs (
  id TEXT PRIMARY KEY,
  state TEXT NOT NULL CHECK(state IN (
    'queued', 'running', 'completed', 'no_change', 'rejected', 'failed'
  )),
  recipe TEXT NOT NULL,
  input_digests_json TEXT NOT NULL,
  pid INTEGER,
  pid_start_ticks TEXT,
  stdout_path TEXT,
  stderr_path TEXT,
  artifact_digest TEXT,
  terminal_summary TEXT,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS notifications (
  event_id TEXT PRIMARY KEY,
  job_id TEXT,
  payload_pb BLOB NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending', 'delivered', 'failed')),
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT,
  FOREIGN KEY(job_id) REFERENCES jobs(id)
);
CREATE UNIQUE INDEX IF NOT EXISTS one_active_job
  ON jobs ((1)) WHERE state IN ('queued', 'running');
";

/// Open a Home database with foreign keys and a five second busy timeout.
///
/// # Errors
/// Returns an error if SQLite cannot open or configure the database.
pub fn connect(database: &Path) -> HomeResult<Connection> {
    let connection = Connection::open(database)?;
    connection.pragma_update(None, "foreign_keys", true)?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(connection)
}

/// Create the schema and convert legacy JSON tables atomically.
///
/// # Errors
/// Returns an error for invalid legacy data or filesystem/SQLite failures.
pub fn migrate(database: &Path) -> HomeResult<()> {
    if let Some(parent) = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let mut connection = connect(database)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    migration::migrate(&transaction)?;
    transaction.commit()?;
    Ok(())
}
