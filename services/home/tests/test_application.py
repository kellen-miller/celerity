from __future__ import annotations

import hashlib
import json
import sqlite3
from collections.abc import Iterator
from pathlib import Path

import httpx
import pytest
from fastapi.testclient import TestClient

from celerity.v1 import celerity_pb2
from celerity.v1.celerity_pb2 import (
    COMPLETION_COMPLETE,
    WEBHOOK_EVENT_TYPE_DEMOTION,
    WEBHOOK_EVENT_TYPE_ROLLBACK,
    WEBHOOK_EVENT_TYPE_TRAINING_FAILURE,
    CompletedRun,
    ControlDecision,
    ReconcileRequest,
    ReconcileResponse,
    RunEvent,
    RunManifest,
    SignalObservation,
    WebhookEvent,
)
from celerity_home.application import create_app, deliver_notifications, migrate
from celerity_home.run import RunFormatError, decode_run_records

AUTHORIZATION = {"Authorization": "Bearer secret"}
PROTOBUF_HEADERS = {**AUTHORIZATION, "Content-Type": "application/x-protobuf"}
WEBHOOK_URL = "https://webhook.invalid/celerity"


@pytest.fixture
def home_client(tmp_path: Path) -> Iterator[TestClient]:
    with TestClient(create_app(tmp_path, "secret", WEBHOOK_URL)) as client:
        yield client


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def varint(value: int) -> bytes:
    encoded = bytearray()
    while value >= 0x80:
        encoded.append((value & 0x7F) | 0x80)
        value >>= 7
    encoded.append(value)
    return bytes(encoded)


def signal_record(sequence: int, signal: str, value: float) -> bytes:
    record = RunEvent(
        schema_major=1,
        sequence=sequence,
        monotonic_ns=sequence * 1_000_000,
        source="test",
    )
    record.signal_observation.CopyFrom(SignalObservation(signal=signal, value=value))
    encoded = record.SerializeToString()
    return varint(len(encoded)) + encoded


def control_record(sequence: int, split: int, state: str) -> bytes:
    encoded = json.dumps({"state": state, "radiator_split_basis_points": split}).encode()
    record = RunEvent(
        schema_major=1,
        sequence=sequence,
        monotonic_ns=sequence * 1_000_000,
        source="test",
    )
    record.control_decision.CopyFrom(ControlDecision(encoded=encoded.decode()))
    serialized = record.SerializeToString()
    return varint(len(serialized)) + serialized


def canonical_chunk(offset: float = 0.0) -> bytes:
    records = bytearray()
    sequence = 0
    for index in range(7):
        sequence += 1
        records.extend(signal_record(sequence, "coolant_temperature_c", 90.0 + offset - index))
        sequence += 1
        records.extend(signal_record(sequence, "air_temperature_c", 45.0 + offset / 2 - index / 2))
        if index < 6:
            sequence += 1
            records.extend(control_record(sequence, 2000, "requested"))
            sequence += 1
            records.extend(control_record(sequence, [1000, 5000, 9000][index % 3], "accepted"))
    return bytes(records)


def test_migrate_converts_legacy_json_contract_storage(tmp_path: Path) -> None:
    database = tmp_path / "home.sqlite3"
    legacy_manifest = json.dumps(
        {
            "schema_version": 1,
            "run_id": "legacy-run",
            "completion": "complete",
            "chunk_file": "events.chunk",
            "chunk_sha256": "a" * 64,
            "chunks": [{"file": "events.chunk", "sha256": "a" * 64}],
            "configuration_generation": 1,
            "configuration_sha256": "b" * 64,
            "model_bundle_digest": None,
            "protocol_major": 1,
            "firmware_generation": 1,
            "decoder_generation": 1,
            "model_abi": "thermal-v1",
            "model_input_signals": ["coolant_temperature_c", "air_temperature_c"],
            "model_history_length": 4,
            "sample_period_ms": 20,
            "command_lattice": [1000, 5000, 9000],
            "maximum_calibration_error": 100.0,
        },
        sort_keys=True,
    ).encode()
    legacy_event = json.dumps(
        {
            "schema_version": 1,
            "event_id": "legacy-event",
            "event_type": "training_failure",
            "occurred_at": "2026-01-01T00:00:00+00:00",
            "job_id": "legacy-job",
            "artifact_digest": None,
            "summary": "legacy failure",
        },
        sort_keys=True,
    )
    with sqlite3.connect(database) as connection:
        connection.executescript(
            """
            CREATE TABLE runs (
              digest TEXT PRIMARY KEY,
              manifest_json BLOB NOT NULL,
              completed_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE jobs (
              id TEXT PRIMARY KEY,
              state TEXT NOT NULL,
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
            CREATE TABLE notifications (
              event_id TEXT PRIMARY KEY,
              job_id TEXT,
              payload_json TEXT NOT NULL,
              state TEXT NOT NULL,
              attempts INTEGER NOT NULL DEFAULT 0,
              last_error TEXT,
              FOREIGN KEY(job_id) REFERENCES jobs(id)
            );
            INSERT INTO jobs(id, state, recipe, input_digests_json)
              VALUES ('legacy-job', 'completed', 'causal-tcn-v1', '[]');
            """
        )
        connection.execute(
            "INSERT INTO runs(digest, manifest_json) VALUES (?, ?)",
            (digest(legacy_manifest), legacy_manifest),
        )
        connection.execute(
            "INSERT INTO notifications(event_id, job_id, payload_json, state) "
            "VALUES ('legacy-event', 'legacy-job', ?, 'failed')",
            (legacy_event,),
        )

    migrate(database)
    migrate(database)

    with sqlite3.connect(database) as connection:
        run_columns = {row[1] for row in connection.execute("PRAGMA table_info(runs)")}
        notification_columns = {
            row[1] for row in connection.execute("PRAGMA table_info(notifications)")
        }
        manifest_bytes = connection.execute("SELECT manifest_pb FROM runs").fetchone()[0]
        notification_bytes = connection.execute("SELECT payload_pb FROM notifications").fetchone()[
            0
        ]
    assert run_columns == {"digest", "manifest_pb", "completed_at"}
    assert notification_columns == {
        "event_id",
        "job_id",
        "payload_pb",
        "state",
        "attempts",
        "last_error",
    }
    assert RunManifest.FromString(manifest_bytes).run_id == "legacy-run"
    assert WebhookEvent.FromString(notification_bytes).summary == "legacy failure"


def test_run_decoder_rejects_duplicate_and_empty_typed_payloads() -> None:
    signal = RunEvent(
        schema_major=1,
        sequence=1,
        monotonic_ns=1,
        source="test",
    )
    signal.signal_observation.CopyFrom(SignalObservation(signal="coolant", value=90.0))
    duplicate = signal.SerializeToString()
    control = ControlDecision(encoded='{"state":"accepted"}').SerializeToString()
    duplicate += varint((24 << 3) | 2) + varint(len(control)) + control
    duplicate = varint(len(duplicate)) + duplicate
    with pytest.raises(RunFormatError, match="exactly one typed payload"):
        decode_run_records(duplicate)

    empty = RunEvent(
        schema_major=1,
        sequence=1,
        monotonic_ns=1,
        source="test",
    )
    empty.control_decision.CopyFrom(ControlDecision())
    encoded = empty.SerializeToString()
    with pytest.raises(RunFormatError, match="encoded Run payload is absent"):
        decode_run_records(varint(len(encoded)) + encoded)


def upload_run(client: TestClient, run_name: str) -> str:
    chunk = canonical_chunk(float(len(run_name)))
    chunk_digest = digest(chunk)
    manifest = RunManifest(
        schema_version=1,
        run_id=run_name,
        completion=COMPLETION_COMPLETE,
        chunk_file="events.chunk",
        chunk_sha256=chunk_digest,
        configuration_generation=1,
        configuration_sha256="a" * 64,
        protocol_major=1,
        firmware_generation=1,
        decoder_generation=1,
        model_abi="thermal-v1",
        model_input_signals=["coolant_temperature_c", "air_temperature_c"],
        model_history_length=4,
        sample_period_ms=20,
        command_lattice=[1000, 5000, 9000],
        maximum_calibration_error=100.0,
    )
    manifest_bytes = manifest.SerializeToString(deterministic=True)
    run_digest = digest(manifest_bytes)
    response = client.put(
        f"/v1/runs/{run_digest}/chunks/{chunk_digest}", content=chunk, headers=AUTHORIZATION
    )
    assert response.status_code == 204
    response = client.post(
        f"/v1/runs/{run_digest}/complete", content=manifest_bytes, headers=PROTOBUF_HEADERS
    )
    assert response.status_code == 200
    assert CompletedRun.FromString(response.content).run_digest == run_digest
    return run_digest


def test_token_auth_and_idempotent_run_admission(home_client: TestClient) -> None:
    client = home_client
    assert client.post("/v1/reconcile", json={}).status_code == 401
    run_digest = upload_run(client, "run-a")
    assert upload_run(client, "run-a") == run_digest
    response = client.post(
        "/v1/reconcile",
        headers=PROTOBUF_HEADERS,
        content=ReconcileRequest(
            schema_version=1,
            completed_run_digests=[run_digest, "f" * 64],
        ).SerializeToString(deterministic=True),
    )
    assert response.status_code == 200
    assert ReconcileResponse.FromString(response.content).missing_run_digests == ["f" * 64]


def test_run_admission_rejects_nonmonotonic_records(
    tmp_path: Path, home_client: TestClient
) -> None:
    first = control_record(2, 5_000, "accepted")
    second = control_record(1, 5_000, "accepted")
    chunk = first + second
    chunk_digest = digest(chunk)
    manifest = RunManifest(
        schema_version=1,
        completion=COMPLETION_COMPLETE,
        chunk_sha256=chunk_digest,
    ).SerializeToString(deterministic=True)
    run_digest = digest(manifest)

    assert (
        home_client.put(
            f"/v1/runs/{run_digest}/chunks/{chunk_digest}",
            content=chunk,
            headers=AUTHORIZATION,
        ).status_code
        == 204
    )
    response = home_client.post(
        f"/v1/runs/{run_digest}/complete", content=manifest, headers=AUTHORIZATION
    )

    assert response.status_code == 422
    assert "sequence is not strictly increasing" in response.json()["detail"]
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        assert connection.execute("SELECT count(*) FROM runs").fetchone() == (0,)


def test_checked_contract_covers_application_routes_and_notification_events(
    tmp_path: Path,
) -> None:
    application_paths = {
        route.path
        for route in create_app(tmp_path, "secret", WEBHOOK_URL).routes
        if hasattr(route, "methods")
    }
    assert application_paths == {
        "/health/live",
        "/health/ready",
        "/v1/reconcile",
        "/v1/runs/{run_digest}/chunks/{chunk_digest}",
        "/v1/runs/{run_digest}/complete",
        "/v1/models/{digest}",
        "/v1/jobs",
        "/v1/jobs/{job_id}",
    }
    assert {
        "RunEvent",
        "RunManifest",
        "ReconcileRequest",
        "ReconcileResponse",
        "CompletedRun",
        "JobRequest",
        "JobStarted",
        "JobStatus",
        "ModelBundleManifest",
        "WebhookEvent",
    } <= set(celerity_pb2.DESCRIPTOR.message_types_by_name)
    assert {
        "WEBHOOK_EVENT_TYPE_TRAINING_FAILURE",
        "WEBHOOK_EVENT_TYPE_CANDIDATE_REJECTION",
        "WEBHOOK_EVENT_TYPE_DEMOTION",
        "WEBHOOK_EVENT_TYPE_ROLLBACK",
    } <= set(celerity_pb2.WebhookEventType.keys())


def test_webhook_failure_never_changes_completed_job(tmp_path: Path) -> None:
    database = tmp_path / "home.sqlite3"
    migrate(database)
    with sqlite3.connect(database) as connection:
        connection.execute(
            "INSERT INTO jobs(id, state, recipe, input_digests_json, terminal_summary) "
            "VALUES ('job-1', 'completed', 'causal-tcn-v1', '[]', 'model staged')"
        )
        connection.execute(
            "INSERT INTO notifications(event_id, job_id, payload_pb, state) "
            "VALUES ('event-1', 'job-1', ?, 'pending')",
            (
                WebhookEvent(
                    schema_version=1,
                    event_id="event-1",
                    event_type=WEBHOOK_EVENT_TYPE_TRAINING_FAILURE,
                    summary="test",
                ).SerializeToString(deterministic=True),
            ),
        )
    transport = httpx.MockTransport(lambda _request: httpx.Response(503))
    with httpx.Client(transport=transport) as client:
        assert deliver_notifications(database, "https://webhook.invalid", client) == 0
    with sqlite3.connect(database) as connection:
        job = connection.execute("SELECT state FROM jobs WHERE id='job-1'").fetchone()
        notification = connection.execute(
            "SELECT state, attempts FROM notifications WHERE event_id='event-1'"
        ).fetchone()
    assert job == ("completed",)
    assert notification == ("failed", 1)


def test_reconcile_demotes_rejected_model_and_selects_known_good(
    tmp_path: Path, home_client: TestClient
) -> None:
    client = home_client
    rejected_digest = "a" * 64
    known_good_digest = "b" * 64
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        connection.execute(
            "INSERT INTO models(digest, path, state) VALUES (?, 'rejected.onnx', 'staged')",
            (rejected_digest,),
        )
        connection.execute(
            "INSERT INTO models(digest, path, state) VALUES (?, 'known-good.onnx', 'staged')",
            (known_good_digest,),
        )
        connection.execute(
            "INSERT INTO settings(key, value) VALUES ('desired_model_digest', ?)",
            (rejected_digest,),
        )

    response = client.post(
        "/v1/reconcile",
        headers=PROTOBUF_HEADERS,
        content=ReconcileRequest(
            schema_version=1,
            active_model_digest=rejected_digest,
            rejected_model_digests=[rejected_digest],
        ).SerializeToString(deterministic=True),
    )
    assert response.status_code == 200
    assert ReconcileResponse.FromString(response.content).desired_model_digest == known_good_digest
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        state = connection.execute(
            "SELECT state FROM models WHERE digest=?", (rejected_digest,)
        ).fetchone()
        events = connection.execute(
            "SELECT payload_pb FROM notifications ORDER BY event_id"
        ).fetchall()
    assert state == ("rejected",)
    assert {WebhookEvent.FromString(event[0]).event_type for event in events} == {
        WEBHOOK_EVENT_TYPE_DEMOTION,
        WEBHOOK_EVENT_TYPE_ROLLBACK,
    }
