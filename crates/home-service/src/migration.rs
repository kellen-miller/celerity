//! Conversion of the pre-protobuf Home database.

use celerity_proto::celerity::v1::{
    Completion, RunChunk, RunManifest, WebhookEvent, WebhookEventType,
};
use prost::Message;
use rusqlite::{Transaction, params, types::Value as SqlValue};
use serde_json::{Map, Value};

use crate::{HomeResult, storage::SCHEMA};

pub(crate) fn migrate(transaction: &Transaction<'_>) -> HomeResult<()> {
    let run_columns = columns(transaction, "PRAGMA table_info(runs)")?;
    let notification_columns = columns(transaction, "PRAGMA table_info(notifications)")?;
    let legacy_runs = run_columns.iter().any(|column| column == "manifest_json")
        && !run_columns.iter().any(|column| column == "manifest_pb");
    let legacy_notifications = notification_columns
        .iter()
        .any(|column| column == "payload_json")
        && !notification_columns
            .iter()
            .any(|column| column == "payload_pb");
    if legacy_runs {
        transaction.execute("ALTER TABLE runs RENAME TO runs_json_legacy", [])?;
    }

    if legacy_notifications {
        transaction.execute(
            "ALTER TABLE notifications RENAME TO notifications_json_legacy",
            [],
        )?;
    }

    transaction.execute_batch(SCHEMA)?;
    if legacy_runs {
        let mut statement = transaction
            .prepare("SELECT digest, manifest_json, completed_at FROM runs_json_legacy")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let manifest = run_manifest(row.get(1)?)?;
            transaction.execute(
                "INSERT INTO runs(digest, manifest_pb, completed_at) VALUES (?, ?, ?)",
                params![row.get::<_, String>(0)?, manifest, row.get::<_, String>(2)?],
            )?;
        }

        drop(rows);
        drop(statement);
        transaction.execute("DROP TABLE runs_json_legacy", [])?;
    }

    if legacy_notifications {
        let mut statement = transaction.prepare(
            "SELECT event_id, job_id, payload_json, state, attempts, last_error
             FROM notifications_json_legacy",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let event_id: String = row.get(0)?;
            let payload = webhook_event(row.get(2)?, &event_id)?;
            transaction.execute(
                "INSERT INTO notifications(event_id, job_id, payload_pb, state, attempts, last_error)
                 VALUES (?, ?, ?, ?, ?, ?)",
                params![
                    event_id,
                    row.get::<_, Option<String>>(1)?,
                    payload,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ],
            )?;
        }

        drop(rows);
        drop(statement);
        transaction.execute("DROP TABLE notifications_json_legacy", [])?;
    }

    Ok(())
}

fn columns(transaction: &Transaction<'_>, query: &str) -> HomeResult<Vec<String>> {
    let mut statement = transaction.prepare(query)?;
    Ok(statement
        .query_map([], |row| row.get(1))?
        .collect::<Result<_, _>>()?)
}

fn json_object(value: SqlValue, label: &str) -> HomeResult<Map<String, Value>> {
    let decoded: Value = match value {
        SqlValue::Text(text) => serde_json::from_str(&text),
        SqlValue::Blob(bytes) => serde_json::from_slice(&bytes),
        _ => return Err(format!("legacy {label} is invalid JSON").into()),
    }
    .map_err(|_| format!("legacy {label} is invalid JSON"))?;
    match decoded {
        Value::Object(object) => Ok(object),
        _ => Err(format!("legacy {label} is not a JSON object").into()),
    }
}

fn optional_string(object: &Map<String, Value>, field: &str) -> HomeResult<Option<String>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        _ => Err(format!("legacy {field} is not a string").into()),
    }
}

fn optional_integer(object: &Map<String, Value>, field: &str) -> HomeResult<Option<u64>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("legacy {field} is not an unsigned integer").into()),
    }
}

fn string_array(object: &Map<String, Value>, field: &str) -> HomeResult<Vec<String>> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| format!("legacy {field} contains a non-string").into())
            })
            .collect(),
        _ => Err(format!("legacy {field} is not an array").into()),
    }
}

fn run_manifest(value: SqlValue) -> HomeResult<Vec<u8>> {
    let legacy = json_object(value, "Run manifest")?;
    let completion = match legacy.get("completion").and_then(Value::as_str) {
        Some("complete") => Completion::Complete,
        Some("incomplete") => Completion::Incomplete,
        _ => return Err("legacy Run manifest has an invalid completion".into()),
    };
    let mut manifest = RunManifest {
        schema_version: u32::try_from(optional_integer(&legacy, "schema_version")?.unwrap_or(0))?,
        run_id: optional_string(&legacy, "run_id")?.unwrap_or_default(),
        completion: completion.into(),
        incomplete_reason: optional_string(&legacy, "incomplete_reason")?,
        first_sequence: optional_integer(&legacy, "first_sequence")?,
        last_sequence: optional_integer(&legacy, "last_sequence")?,
        chunk_file: optional_string(&legacy, "chunk_file")?.unwrap_or_default(),
        chunk_sha256: optional_string(&legacy, "chunk_sha256")?.unwrap_or_default(),
        dropped_record_count: optional_integer(&legacy, "dropped_record_count")?.unwrap_or(0),
        configuration_generation: optional_integer(&legacy, "configuration_generation")?
            .unwrap_or(0),
        configuration_sha256: optional_string(&legacy, "configuration_sha256")?.unwrap_or_default(),
        model_bundle_digest: optional_string(&legacy, "model_bundle_digest")?,
        protocol_major: u32::try_from(optional_integer(&legacy, "protocol_major")?.unwrap_or(0))?,
        firmware_generation: u32::try_from(
            optional_integer(&legacy, "firmware_generation")?.unwrap_or(0),
        )?,
        decoder_generation: optional_integer(&legacy, "decoder_generation")?.unwrap_or(0),
        model_abi: optional_string(&legacy, "model_abi")?.unwrap_or_default(),
        model_input_signals: string_array(&legacy, "model_input_signals")?,
        model_history_length: optional_integer(&legacy, "model_history_length")?.unwrap_or(0),
        sample_period_ms: optional_integer(&legacy, "sample_period_ms")?.unwrap_or(0),
        maximum_calibration_error: match legacy.get("maximum_calibration_error") {
            None | Some(Value::Null) => 0.0,
            Some(value) => value
                .as_f64()
                .ok_or("legacy maximum_calibration_error is not numeric")?,
        },
        ..RunManifest::default()
    };
    match legacy.get("command_lattice") {
        None | Some(Value::Null) => {}
        Some(Value::Array(values)) => {
            for value in values {
                let value = value
                    .as_u64()
                    .ok_or("legacy command_lattice contains a non-integer")?;
                manifest.command_lattice.push(u32::try_from(value)?);
            }
        }
        _ => return Err("legacy command_lattice is not an array".into()),
    }

    match legacy.get("chunks") {
        None | Some(Value::Null) => {}
        Some(Value::Array(chunks)) => {
            for chunk in chunks {
                let chunk = chunk
                    .as_object()
                    .ok_or("legacy Run manifest chunk is invalid")?;
                manifest.chunks.push(RunChunk {
                    file: chunk
                        .get("file")
                        .and_then(Value::as_str)
                        .ok_or("legacy Run manifest chunk is incomplete")?
                        .to_owned(),
                    sha256: chunk
                        .get("sha256")
                        .and_then(Value::as_str)
                        .ok_or("legacy Run manifest chunk is incomplete")?
                        .to_owned(),
                });
            }
        }
        _ => return Err("legacy Run manifest chunks are invalid".into()),
    }

    Ok(manifest.encode_to_vec())
}

fn webhook_event(value: SqlValue, row_event_id: &str) -> HomeResult<Vec<u8>> {
    let legacy = json_object(value, "webhook event")?;
    let event_id = match legacy.get("event_id") {
        None => row_event_id,
        Some(value) => value
            .as_str()
            .ok_or("legacy webhook event id does not match its row")?,
    };
    if event_id != row_event_id {
        return Err("legacy webhook event id does not match its row".into());
    }

    let event_type = match legacy.get("event_type").and_then(Value::as_str) {
        Some("training_failure") => WebhookEventType::TrainingFailure,
        Some("candidate_rejection") => WebhookEventType::CandidateRejection,
        Some("demotion") => WebhookEventType::Demotion,
        Some("rollback") => WebhookEventType::Rollback,
        _ => return Err("legacy webhook event has an invalid type".into()),
    };
    let schema_version = match legacy.get("schema_version") {
        None => 1,
        Some(value) => value
            .as_u64()
            .ok_or("legacy webhook event schema version is invalid")?,
    };
    let event = WebhookEvent {
        schema_version: u32::try_from(schema_version)?,
        event_id: event_id.to_owned(),
        event_type: event_type.into(),
        occurred_at: legacy
            .get("occurred_at")
            .and_then(Value::as_str)
            .ok_or("legacy webhook event is incomplete")?
            .to_owned(),
        summary: legacy
            .get("summary")
            .and_then(Value::as_str)
            .ok_or("legacy webhook event is incomplete")?
            .to_owned(),
        job_id: optional_string(&legacy, "job_id")?,
        artifact_digest: optional_string(&legacy, "artifact_digest")?,
    };
    Ok(event.encode_to_vec())
}
