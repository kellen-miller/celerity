use celerity_proto::celerity::v1::{
    Completion, RunChunk, RunManifest, WebhookEvent, WebhookEventType,
};
use home_service::{
    HomeResult,
    storage::{connect, migrate},
};
use prost::Message;
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use tempfile::tempdir;

const LEGACY_SCHEMA: &str = "
CREATE TABLE runs (
  digest TEXT PRIMARY KEY,
  manifest_json BLOB NOT NULL,
  completed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE notifications (
  event_id TEXT PRIMARY KEY,
  job_id TEXT,
  payload_json TEXT NOT NULL,
  state TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0,
  last_error TEXT
);
";

fn manifest() -> Value {
    json!({
        "schema_version": 1,
        "run_id": "legacy-run",
        "completion": "incomplete",
        "incomplete_reason": "disk full",
        "first_sequence": 1,
        "last_sequence": 9,
        "chunk_file": "events.chunk",
        "chunk_sha256": "a".repeat(64),
        "chunks": [{"file": "events.chunk", "sha256": "a".repeat(64)}],
        "dropped_record_count": 2,
        "configuration_generation": 3,
        "configuration_sha256": "b".repeat(64),
        "model_bundle_digest": null,
        "protocol_major": 1,
        "firmware_generation": 2,
        "decoder_generation": 3,
        "model_abi": "thermal-v1",
        "model_input_signals": ["coolant", "air"],
        "model_history_length": 4,
        "sample_period_ms": 20,
        "command_lattice": [1000, 5000, 9000],
        "maximum_calibration_error": 100.0
    })
}

#[test]
fn migrates_real_legacy_rows_once_preserving_metadata() -> HomeResult<()> {
    let directory = tempdir()?;
    let database = directory.path().join("home.sqlite3");
    let connection = Connection::open(&database)?;
    connection.execute_batch(LEGACY_SCHEMA)?;
    connection.execute_batch(
        "CREATE TABLE jobs (
          id TEXT PRIMARY KEY, state TEXT NOT NULL, recipe TEXT NOT NULL,
          input_digests_json TEXT NOT NULL, pid INTEGER, pid_start_ticks TEXT,
          stdout_path TEXT, stderr_path TEXT, artifact_digest TEXT,
          terminal_summary TEXT, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        INSERT INTO jobs(id, state, recipe, input_digests_json)
          VALUES ('legacy-job', 'completed', 'recipe', '[]');",
    )?;
    connection.execute(
        "INSERT INTO runs VALUES ('digest', ?, '2026-01-01')",
        [serde_json::to_vec(&manifest())?],
    )?;
    let event = json!({
        "schema_version": 3,
        "event_id": "event",
        "event_type": "training_failure",
        "occurred_at": "2026-01-01T00:00:00+00:00",
        "job_id": "legacy-job",
        "artifact_digest": "model",
        "summary": "legacy failure"
    });
    connection.execute(
        "INSERT INTO notifications VALUES ('event', 'legacy-job', ?, 'failed', 7, 'timeout')",
        [serde_json::to_string(&event)?],
    )?;
    migrate(&database)?;
    migrate(&database)?;
    let (bytes, completed_at): (Vec<u8>, String) = connection.query_row(
        "SELECT manifest_pb, completed_at FROM runs WHERE digest='digest'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let decoded = RunManifest::decode(bytes.as_slice())?;
    assert_eq!(
        decoded,
        RunManifest {
            schema_version: 1,
            run_id: "legacy-run".to_owned(),
            completion: Completion::Incomplete.into(),
            incomplete_reason: Some("disk full".to_owned()),
            first_sequence: Some(1),
            last_sequence: Some(9),
            chunks: vec![RunChunk {
                file: "events.chunk".to_owned(),
                sha256: "a".repeat(64)
            }],
            chunk_file: "events.chunk".to_owned(),
            chunk_sha256: "a".repeat(64),
            configuration_sha256: "b".repeat(64),
            dropped_record_count: 2,
            configuration_generation: 3,
            model_bundle_digest: None,
            protocol_major: 1,
            firmware_generation: 2,
            decoder_generation: 3,
            model_abi: "thermal-v1".to_owned(),
            model_input_signals: vec!["coolant".to_owned(), "air".to_owned()],
            model_history_length: 4,
            sample_period_ms: 20,
            command_lattice: vec![1000, 5000, 9000],
            maximum_calibration_error: 100.0,
        }
    );
    assert_eq!(completed_at, "2026-01-01");
    let (payload, attempts, last_error): (Vec<u8>, i64, String) = connection.query_row(
        "SELECT payload_pb, attempts, last_error FROM notifications",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let decoded = WebhookEvent::decode(payload.as_slice())?;
    assert_eq!(
        decoded,
        WebhookEvent {
            schema_version: 3,
            event_id: "event".to_owned(),
            event_type: WebhookEventType::TrainingFailure.into(),
            occurred_at: "2026-01-01T00:00:00+00:00".to_owned(),
            summary: "legacy failure".to_owned(),
            artifact_digest: Some("model".to_owned()),
            job_id: Some("legacy-job".to_owned()),
        }
    );
    assert_eq!(attempts, 7);
    assert_eq!(last_error, "timeout");
    assert_eq!(
        connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE '%json_legacy'",
            [],
            |row| row.get(0),
        )?,
        0
    );
    Ok(())
}

#[test]
fn empty_legacy_tables_are_removed() -> HomeResult<()> {
    let directory = tempdir()?;
    let database = directory.path().join("home.sqlite3");
    let connection = Connection::open(&database)?;
    connection.execute_batch(LEGACY_SCHEMA)?;
    migrate(&database)?;
    migrate(&database)?;
    assert_eq!(
        connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM sqlite_master WHERE name LIKE '%json_legacy'",
            [],
            |row| row.get(0),
        )?,
        0
    );
    connection.prepare("SELECT manifest_pb FROM runs")?;
    connection.prepare("SELECT payload_pb FROM notifications")?;
    Ok(())
}

#[test]
fn invalid_notification_rolls_back_renames_copies_and_schema() -> HomeResult<()> {
    let directory = tempdir()?;
    let database = directory.path().join("home.sqlite3");
    let connection = Connection::open(&database)?;
    connection.execute_batch(LEGACY_SCHEMA)?;
    let original = serde_json::to_string(&manifest())?;
    connection.execute("INSERT INTO runs VALUES ('digest', ?, 'date')", [&original])?;
    connection.execute(
        "INSERT INTO notifications VALUES ('event', NULL, 'not JSON', 'pending', 0, NULL)",
        [],
    )?;
    assert!(migrate(&database).is_err());
    assert_eq!(
        connection
            .query_row::<String, _, _>("SELECT manifest_json FROM runs", [], |row| row.get(0),)?,
        original
    );
    assert_eq!(
        connection.query_row::<String, _, _>(
            "SELECT payload_json FROM notifications",
            [],
            |row| row.get(0),
        )?,
        "not JSON"
    );
    assert_eq!(connection.query_row::<i64, _, _>(
        "SELECT count(*) FROM sqlite_master WHERE name IN ('jobs', 'runs_json_legacy', 'notifications_json_legacy')",
        [], |row| row.get(0),
    )?, 0);
    Ok(())
}

#[test]
fn rejects_invalid_legacy_manifest_fields_without_data_loss() -> HomeResult<()> {
    for (field, value) in [
        ("completion", json!("other")),
        ("schema_version", json!(true)),
        ("schema_version", json!(4_294_967_296_u64)),
        ("first_sequence", json!(-1)),
        ("run_id", json!(17)),
        ("model_input_signals", json!(["coolant", 7])),
        ("command_lattice", json!([true])),
        ("command_lattice", json!([4_294_967_296_u64])),
        ("maximum_calibration_error", json!("bad")),
        ("chunks", json!({})),
        ("chunks", json!([{}])),
        ("chunks", json!([null])),
    ] {
        let directory = tempdir()?;
        let database = directory.path().join("home.sqlite3");
        let connection = Connection::open(&database)?;
        connection.execute_batch(LEGACY_SCHEMA)?;
        let mut legacy = manifest();
        legacy[field] = value;
        let original = serde_json::to_string(&legacy)?;
        connection.execute("INSERT INTO runs VALUES ('digest', ?, 'date')", [&original])?;
        assert!(migrate(&database).is_err(), "accepted invalid {field}");
        assert_eq!(
            connection
                .query_row::<String, _, _>("SELECT manifest_json FROM runs", [], |row| row
                    .get(0),)?,
            original
        );
    }

    Ok(())
}

#[test]
fn migrates_event_types_defaults_and_optional_fields() -> HomeResult<()> {
    for (kind, expected) in [
        ("training_failure", WebhookEventType::TrainingFailure),
        ("candidate_rejection", WebhookEventType::CandidateRejection),
        ("demotion", WebhookEventType::Demotion),
        ("rollback", WebhookEventType::Rollback),
    ] {
        let directory = tempdir()?;
        let database = directory.path().join("home.sqlite3");
        let connection = Connection::open(&database)?;
        connection.execute_batch(LEGACY_SCHEMA)?;
        let legacy = json!({"event_type": kind, "occurred_at": "date", "summary": "summary"});
        connection.execute(
            "INSERT INTO notifications VALUES ('event', NULL, ?, 'pending', 0, NULL)",
            [serde_json::to_vec(&legacy)?],
        )?;
        migrate(&database)?;
        let payload: Vec<u8> =
            connection.query_row("SELECT payload_pb FROM notifications", [], |row| row.get(0))?;
        let decoded = WebhookEvent::decode(payload.as_slice())?;
        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.event_id, "event");
        assert_eq!(decoded.event_type, i32::from(expected));
    }

    Ok(())
}

#[test]
fn invalid_event_fields_roll_back_the_original_payload() -> HomeResult<()> {
    for (field, value) in [
        ("event_id", json!("different")),
        ("event_id", Value::Null),
        ("event_type", json!("invalid")),
        ("schema_version", json!(true)),
        ("schema_version", Value::Null),
        ("schema_version", json!(-1)),
        ("occurred_at", json!(5)),
        ("summary", Value::Null),
        ("job_id", json!(false)),
        ("artifact_digest", json!([])),
    ] {
        let directory = tempdir()?;
        let database = directory.path().join("home.sqlite3");
        let connection = Connection::open(&database)?;
        connection.execute_batch(LEGACY_SCHEMA)?;
        let mut legacy =
            json!({"event_type": "rollback", "occurred_at": "date", "summary": "summary"});
        legacy[field] = value;
        let original = serde_json::to_string(&legacy)?;
        connection.execute(
            "INSERT INTO notifications VALUES ('event', NULL, ?, 'pending', 0, NULL)",
            [&original],
        )?;
        assert!(migrate(&database).is_err(), "accepted invalid {field}");
        assert_eq!(
            connection.query_row::<String, _, _>(
                "SELECT payload_json FROM notifications",
                [],
                |row| row.get(0),
            )?,
            original
        );
    }

    Ok(())
}

#[test]
fn configures_connections_and_enforces_one_active_job() -> HomeResult<()> {
    let directory = tempdir()?;
    let database = directory.path().join("nested/home.sqlite3");
    migrate(&database)?;
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
