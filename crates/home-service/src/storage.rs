//! SQLite connections and atomic Home schema initialization.

use std::{path::Path, time::Duration};

use rusqlite::{Connection, TransactionBehavior};

use crate::HomeResult;

pub(crate) const SCHEMA: &str = include_str!("../../../sql/migrations/home/schema.sql");

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

/// Initialize the current schema atomically; retain data across restarts.
///
/// # Errors
/// Returns filesystem or SQLite initialization failures.
pub fn initialize(database: &Path) -> HomeResult<()> {
    if let Some(parent) = database
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let mut connection = connect(database)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute_batch(SCHEMA)?;
    transaction.commit()?;
    Ok(())
}
