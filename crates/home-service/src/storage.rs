//! SQLite connections and atomic Home schema migration.

use std::{path::Path, time::Duration};

use rusqlite::{Connection, TransactionBehavior};

use crate::{HomeResult, migration};

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
