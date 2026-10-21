use std::{fs, path::Path};

use rusqlite::Connection;

use super::inventory::SpoolRun;
use super::{SyncError, current_unix_seconds, sync_io, sync_sql};
use celerity_sql::sync::{
    GetRunAcknowledged, GetRunAttempt, GetRunRetry, HasRunBackoff, ObserveRun, RecordRunAttempt,
};

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
            .execute_batch(include_str!("../../../sql/migrations/sync/schema.sql"))
            .map_err(sync_sql)?;
        let has_backoff = HasRunBackoff
            .query_one(&connection)
            .map_err(sync_sql)?
            .matching_columns
            > 0;
        if !has_backoff {
            connection
                .execute_batch(include_str!(
                    "../../../sql/migrations/sync/add-run-backoff.sql"
                ))
                .map_err(sync_sql)?;
        }

        Ok(Self { connection })
    }

    pub(super) fn observe_run(&self, run: &SpoolRun) -> Result<(), SyncError> {
        ObserveRun::builder()
            .run_digest(&run.digest)
            .run_path(&run.directory.to_string_lossy())
            .build()
            .execute(&self.connection)
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
        let attempts = GetRunAttempt::builder()
            .run_digest(digest)
            .build()
            .query_one(&self.connection)
            .map_err(sync_sql)?
            .attempts;
        let attempts = u32::try_from(attempts)
            .map_err(|_| sync_sql(rusqlite::Error::IntegralValueOutOfRange(0, attempts)))?;
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
        RecordRunAttempt::builder()
            .run_digest(digest)
            .acknowledged(acknowledged)
            .last_error(error.as_deref())
            .next_attempt_at(next_attempt_at)
            .build()
            .execute(&self.connection)
            .map_err(sync_sql)?;
        Ok(())
    }

    #[must_use]
    pub fn retry_allowed(&self, digest: &str) -> bool {
        GetRunRetry::builder()
            .run_digest(digest)
            .build()
            .query_one(&self.connection)
            .map_or(true, |row| row.next_attempt_at <= current_unix_seconds())
    }

    #[must_use]
    pub fn acknowledged(&self, digest: &str) -> bool {
        GetRunAcknowledged::builder()
            .run_digest(digest)
            .build()
            .query_one(&self.connection)
            .is_ok_and(|row| row.acknowledged != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_journal_retains_transfers_and_upgrades_once() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("sync.sqlite3");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(include_str!("../../../sql/tests/sync/legacy-schema.sql"))
            .unwrap();
        ObserveRun::builder()
            .run_digest("legacy-run")
            .run_path("/spool/original")
            .build()
            .execute(&connection)
            .unwrap();
        drop(connection);

        let journal = TransferJournal::open(&path).unwrap();
        assert!(journal.retry_allowed("legacy-run"));
        assert!(!journal.acknowledged("legacy-run"));
        journal.record_attempt("legacy-run", &Ok(())).unwrap();
        drop(journal);

        let journal = TransferJournal::open(&path).unwrap();
        assert!(journal.acknowledged("legacy-run"));
        assert!(journal.retry_allowed("legacy-run"));
    }

    #[test]
    fn retry_backoff_and_acknowledgement_survive_reobservation() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("nested/sync.sqlite3");
        let journal = TransferJournal::open(&path).unwrap();
        let run = SpoolRun {
            digest: "run-digest".to_owned(),
            directory: temporary.path().join("spool"),
            manifest: Vec::new(),
            chunks: Vec::new(),
        };
        assert!(journal.retry_allowed("missing"));
        assert!(!journal.acknowledged("missing"));
        journal.observe_run(&run).unwrap();

        for delay in [60, 120, 240, 480, 960, 1_920, 1_920] {
            let before = current_unix_seconds();
            journal
                .record_attempt(&run.digest, &Err(SyncError("offline".to_owned())))
                .unwrap();
            let after = current_unix_seconds();
            let retry = GetRunRetry::builder()
                .run_digest(&run.digest)
                .build()
                .query_one(&journal.connection)
                .unwrap();
            assert!((before + delay..=after + delay).contains(&retry.next_attempt_at));
            assert!(!journal.retry_allowed(&run.digest));
            assert!(!journal.acknowledged(&run.digest));
        }

        journal.observe_run(&run).unwrap();
        assert!(!journal.retry_allowed(&run.digest));
        journal.record_attempt(&run.digest, &Ok(())).unwrap();
        journal.observe_run(&run).unwrap();
        assert!(journal.acknowledged(&run.digest));
        assert!(journal.retry_allowed(&run.digest));
        drop(journal);

        let journal = TransferJournal::open(&path).unwrap();
        assert!(journal.acknowledged(&run.digest));
        assert!(journal.retry_allowed(&run.digest));
    }
}
