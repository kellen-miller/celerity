use std::{fs, path::Path};

use rusqlite::{Connection, params};

use super::inventory::SpoolRun;
use super::{SyncError, current_unix_seconds, sync_io, sync_sql};

pub struct TransferJournal {
    connection: Connection,
}

impl TransferJournal {
    /// Opens and migrates the sync-owned `SQLite` transfer journal.
    ///
    /// # Errors
    ///
    /// Returns an error when the database cannot be opened or migrated.
    pub fn open(path: &Path) -> Result<Self, SyncError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(sync_io)?;
        }
        let connection = Connection::open(path).map_err(sync_sql)?;
        connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 CREATE TABLE IF NOT EXISTS run_transfer (
                   run_digest TEXT PRIMARY KEY,
                   run_path TEXT NOT NULL,
                   acknowledged INTEGER NOT NULL DEFAULT 0,
                   attempts INTEGER NOT NULL DEFAULT 0,
                   last_error TEXT,
                   next_attempt_at INTEGER NOT NULL DEFAULT 0
                 );
                 CREATE TABLE IF NOT EXISTS model_transfer (
                   digest TEXT PRIMARY KEY,
                   state TEXT NOT NULL,
                   last_error TEXT
                 );",
            )
            .map_err(sync_sql)?;
        let has_backoff = connection
            .query_row(
                "SELECT count(*) FROM pragma_table_info('run_transfer') WHERE name='next_attempt_at'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(sync_sql)?
            > 0;
        if !has_backoff {
            connection
                .execute(
                    "ALTER TABLE run_transfer ADD COLUMN next_attempt_at INTEGER NOT NULL DEFAULT 0",
                    [],
                )
                .map_err(sync_sql)?;
        }
        Ok(Self { connection })
    }

    pub(super) fn observe_run(&self, run: &SpoolRun) -> Result<(), SyncError> {
        self.connection
            .execute(
                "INSERT INTO run_transfer(run_digest, run_path) VALUES (?1, ?2)
                 ON CONFLICT(run_digest) DO UPDATE SET run_path=excluded.run_path",
                params![run.digest, run.directory.to_string_lossy()],
            )
            .map_err(sync_sql)?;
        Ok(())
    }

    pub(super) fn record_attempt(
        &self,
        digest: &str,
        result: &Result<(), SyncError>,
    ) -> Result<(), SyncError> {
        let (acknowledged, error) = match result {
            Ok(()) => (1, None),
            Err(error) => (0, Some(error.to_string())),
        };
        let attempts: u32 = self
            .connection
            .query_row(
                "SELECT attempts FROM run_transfer WHERE run_digest=?1",
                [digest],
                |row| row.get(0),
            )
            .map_err(sync_sql)?;
        let next_attempt_at: i64 = if acknowledged == 1 {
            0
        } else {
            current_unix_seconds().saturating_add(
                i64::try_from(
                    30_u64
                        .saturating_mul(1_u64 << attempts.saturating_add(1).min(6))
                        .min(3_600),
                )
                .unwrap_or(i64::MAX),
            )
        };
        self.connection
            .execute(
                "UPDATE run_transfer SET attempts=attempts+1, acknowledged=?2, last_error=?3,
                 next_attempt_at=?4
                 WHERE run_digest=?1",
                params![digest, acknowledged, error, next_attempt_at],
            )
            .map_err(sync_sql)?;
        Ok(())
    }

    #[must_use]
    pub fn retry_allowed(&self, digest: &str) -> bool {
        self.connection
            .query_row(
                "SELECT next_attempt_at FROM run_transfer WHERE run_digest=?1",
                [digest],
                |row| row.get::<_, i64>(0),
            )
            .map_or(true, |next_attempt_at| {
                next_attempt_at <= current_unix_seconds()
            })
    }

    #[must_use]
    pub fn acknowledged(&self, digest: &str) -> bool {
        self.connection
            .query_row(
                "SELECT acknowledged FROM run_transfer WHERE run_digest=?1",
                [digest],
                |row| row.get::<_, bool>(0),
            )
            .unwrap_or(false)
    }
}
