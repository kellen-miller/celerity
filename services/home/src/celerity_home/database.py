from __future__ import annotations

import json
import sqlite3
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from celerity.v1.celerity_pb2 import (
    COMPLETION_COMPLETE,
    COMPLETION_INCOMPLETE,
    WEBHOOK_EVENT_TYPE_CANDIDATE_REJECTION,
    WEBHOOK_EVENT_TYPE_DEMOTION,
    WEBHOOK_EVENT_TYPE_ROLLBACK,
    WEBHOOK_EVENT_TYPE_TRAINING_FAILURE,
    RunManifest,
    WebhookEvent,
)


def connect(database: Path) -> sqlite3.Connection:
    connection = sqlite3.connect(database, timeout=5)
    connection.row_factory = sqlite3.Row
    connection.execute("PRAGMA foreign_keys=ON")
    connection.execute("PRAGMA busy_timeout=5000")
    return connection


def _legacy_json_object(value: object, label: str) -> dict[str, object]:
    try:
        if isinstance(value, bytes | bytearray | memoryview):
            value = bytes(value).decode("utf-8")
        if not isinstance(value, str):
            raise RuntimeError(f"legacy {label} is not JSON text")
        decoded = json.loads(value)
    except (RuntimeError, UnicodeDecodeError, json.JSONDecodeError) as error:
        raise RuntimeError(f"legacy {label} is invalid JSON") from error
    if not isinstance(decoded, dict):
        raise RuntimeError(f"legacy {label} is not a JSON object")
    return decoded


def _legacy_run_manifest(value: object) -> bytes:
    legacy = _legacy_json_object(value, "Run manifest")
    completion = legacy.get("completion")
    completion_values = {
        "complete": COMPLETION_COMPLETE,
        "incomplete": COMPLETION_INCOMPLETE,
    }
    if not isinstance(completion, str) or completion not in completion_values:
        raise RuntimeError("legacy Run manifest has an invalid completion")
    fields = (
        "schema_version",
        "run_id",
        "incomplete_reason",
        "first_sequence",
        "last_sequence",
        "chunk_file",
        "chunk_sha256",
        "dropped_record_count",
        "configuration_generation",
        "configuration_sha256",
        "model_bundle_digest",
        "protocol_major",
        "firmware_generation",
        "decoder_generation",
        "model_abi",
        "model_input_signals",
        "model_history_length",
        "sample_period_ms",
        "command_lattice",
        "maximum_calibration_error",
    )
    message_fields = {
        field: legacy[field] for field in fields if field in legacy and legacy[field] is not None
    }
    message_fields["completion"] = completion_values[completion]
    try:
        manifest = RunManifest(**message_fields)
    except (TypeError, ValueError) as error:
        raise RuntimeError("legacy Run manifest has invalid fields") from error
    chunks = legacy.get("chunks", [])
    if chunks is None:
        chunks = []
    if not isinstance(chunks, list):
        raise RuntimeError("legacy Run manifest chunks are invalid")
    for chunk in chunks:
        if not isinstance(chunk, dict):
            raise RuntimeError("legacy Run manifest chunk is invalid")
        file = chunk.get("file")
        sha256 = chunk.get("sha256")
        if not isinstance(file, str) or not isinstance(sha256, str):
            raise RuntimeError("legacy Run manifest chunk is incomplete")
        manifest.chunks.add(file=file, sha256=sha256)
    return manifest.SerializeToString(deterministic=True)


def _legacy_webhook_event(value: object, row_event_id: str) -> bytes:
    legacy = _legacy_json_object(value, "webhook event")
    event_id = legacy.get("event_id", row_event_id)
    event_type = legacy.get("event_type")
    event_types = {
        "training_failure": WEBHOOK_EVENT_TYPE_TRAINING_FAILURE,
        "candidate_rejection": WEBHOOK_EVENT_TYPE_CANDIDATE_REJECTION,
        "demotion": WEBHOOK_EVENT_TYPE_DEMOTION,
        "rollback": WEBHOOK_EVENT_TYPE_ROLLBACK,
    }
    if not isinstance(event_id, str) or event_id != row_event_id:
        raise RuntimeError("legacy webhook event id does not match its row")
    if not isinstance(event_type, str) or event_type not in event_types:
        raise RuntimeError("legacy webhook event has an invalid type")
    occurred_at = legacy.get("occurred_at")
    summary = legacy.get("summary")
    if not isinstance(occurred_at, str) or not isinstance(summary, str):
        raise RuntimeError("legacy webhook event is incomplete")
    try:
        event = WebhookEvent(
            schema_version=legacy.get("schema_version", 1),
            event_id=event_id,
            event_type=event_types[event_type],
            occurred_at=occurred_at,
            summary=summary,
        )
    except (TypeError, ValueError) as error:
        raise RuntimeError("legacy webhook event has invalid fields") from error
    job_id = legacy.get("job_id")
    artifact_digest = legacy.get("artifact_digest")
    if job_id is not None:
        if not isinstance(job_id, str):
            raise RuntimeError("legacy webhook event job id is invalid")
        event.job_id = job_id
    if artifact_digest is not None:
        if not isinstance(artifact_digest, str):
            raise RuntimeError("legacy webhook event artifact digest is invalid")
        event.artifact_digest = artifact_digest
    return event.SerializeToString(deterministic=True)


def migrate(database: Path) -> None:
    """Create the schema and migrate the pre-protobuf Home database once."""
    database.parent.mkdir(parents=True, exist_ok=True)
    with connect(database) as connection:
        connection.execute("PRAGMA journal_mode=WAL")
        legacy_runs: list[tuple[str, bytes, str]] = []
        legacy_notifications: list[tuple[str, str | None, bytes, str, int, str | None]] = []
        run_columns = {
            row["name"] for row in connection.execute("PRAGMA table_info(runs)").fetchall()
        }
        notification_columns = {
            row["name"] for row in connection.execute("PRAGMA table_info(notifications)").fetchall()
        }
        if "manifest_json" in run_columns and "manifest_pb" not in run_columns:
            legacy_run_rows = connection.execute(
                "SELECT digest, manifest_json, completed_at FROM runs"
            ).fetchall()
            legacy_runs = [
                (
                    row["digest"],
                    _legacy_run_manifest(row["manifest_json"]),
                    row["completed_at"],
                )
                for row in legacy_run_rows
            ]
            connection.execute("ALTER TABLE runs RENAME TO runs_json_legacy")
        if "payload_json" in notification_columns and "payload_pb" not in notification_columns:
            legacy_notification_rows = connection.execute(
                "SELECT event_id, job_id, payload_json, state, attempts, last_error "
                "FROM notifications"
            ).fetchall()
            legacy_notifications = [
                (
                    row["event_id"],
                    row["job_id"],
                    _legacy_webhook_event(row["payload_json"], row["event_id"]),
                    row["state"],
                    row["attempts"],
                    row["last_error"],
                )
                for row in legacy_notification_rows
            ]
            connection.execute("ALTER TABLE notifications RENAME TO notifications_json_legacy")
        connection.executescript(
            """
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
            """
        )
        for digest, manifest_bytes, completed_at in legacy_runs:
            connection.execute(
                "INSERT INTO runs(digest, manifest_pb, completed_at) VALUES (?, ?, ?)",
                (digest, manifest_bytes, completed_at),
            )
        if legacy_runs:
            connection.execute("DROP TABLE runs_json_legacy")
        for event_id, job_id, payload_bytes, state, attempts, last_error in legacy_notifications:
            connection.execute(
                "INSERT INTO notifications"
                "(event_id, job_id, payload_pb, state, attempts, last_error) "
                "VALUES (?, ?, ?, ?, ?, ?)",
                (
                    event_id,
                    job_id,
                    payload_bytes,
                    state,
                    attempts,
                    last_error,
                ),
            )
        if legacy_notifications:
            connection.execute("DROP TABLE notifications_json_legacy")


@contextmanager
def transaction(database: Path) -> Iterator[sqlite3.Connection]:
    connection = connect(database)
    try:
        connection.execute("BEGIN IMMEDIATE")
        yield connection
        connection.commit()
    except BaseException:
        connection.rollback()
        raise
    finally:
        connection.close()
