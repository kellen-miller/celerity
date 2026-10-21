use home_service::{
    HomeResult,
    storage::{connect, initialize},
};
use rusqlite::params;
use tempfile::tempdir;

#[test]
fn initializes_idempotently_and_enforces_constraints() -> HomeResult<()> {
    let directory = tempdir()?;
    let database = directory.path().join("nested/home.sqlite3");
    initialize(&database)?;
    initialize(&database)?;
    let connection = connect(&database)?;
    for (query, expected) in [("PRAGMA foreign_keys", 1), ("PRAGMA busy_timeout", 5000)] {
        assert_eq!(
            connection.query_row::<i64, _, _>(query, [], |row| row.get(0))?,
            expected
        );
    }

    assert_eq!(
        connection.query_row::<String, _, _>("PRAGMA journal_mode", [], |row| row.get(0))?,
        "wal"
    );
    connection.execute(
        "INSERT INTO jobs(id, state, recipe, input_digests_json) VALUES (?, ?, 'recipe', '[]')",
        params!["job1", "queued"],
    )?;
    initialize(&database)?;
    assert_eq!(
        connection.query_row::<i64, _, _>("SELECT count(*) FROM jobs", [], |row| row.get(0))?,
        1
    );
    assert!(
        connection
            .execute(
                "INSERT INTO jobs(id, state, recipe, input_digests_json) VALUES (?, ?, 'recipe', '[]')",
                params!["job2", "running"],
            )
            .is_err()
    );
    assert!(connection.execute(
        "INSERT INTO notifications(event_id, job_id, payload_pb, state) VALUES ('event', 'missing', X'', 'pending')", [],
    ).is_err());
    Ok(())
}
