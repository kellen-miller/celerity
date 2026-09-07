from __future__ import annotations

import hashlib
import json
import sqlite3
import time
from collections.abc import Iterator
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from celerity.v1.celerity_pb2 import (
    COMPLETION_COMPLETE,
    JOB_STATE_COMPLETED,
    JOB_STATE_FAILED,
    JOB_STATE_NO_CHANGE,
    JOB_STATE_QUEUED,
    JOB_STATE_RUNNING,
    WEBHOOK_EVENT_TYPE_TRAINING_FAILURE,
    CompletedRun,
    ControlDecision,
    JobRequest,
    JobStarted,
    JobStatus,
    RunEvent,
    RunManifest,
    SignalObservation,
    WebhookEvent,
)
from celerity_home import worker
from celerity_home.application import create_app, migrate
from celerity_home.training import MAXIMUM_PARITY_ERROR, Recipe, load_training_examples
from celerity_home.worker import classify_evaluation

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


def test_job_admits_only_complete_runs_and_survives_as_durable_subprocess(
    tmp_path: Path, home_client: TestClient
) -> None:
    client = home_client
    assert client.post("/v1/jobs", json={}).status_code == 401
    rejected = client.post(
        "/v1/jobs",
        headers=PROTOBUF_HEADERS,
        content=JobRequest(run_digests=["0" * 64], recipe="causal-tcn-v1").SerializeToString(
            deterministic=True
        ),
    )
    assert rejected.status_code == 409
    run_digests = [upload_run(client, "run-training-a"), upload_run(client, "run-training-b")]
    admitted = client.post(
        "/v1/jobs",
        headers=PROTOBUF_HEADERS,
        content=JobRequest(run_digests=run_digests, recipe="causal-tcn-v1").SerializeToString(
            deterministic=True
        ),
    )
    assert admitted.status_code == 202
    job_id = JobStarted.FromString(admitted.content).job_id
    deadline = time.monotonic() + 10
    job = JobStatus.FromString(client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).content)
    while time.monotonic() < deadline and job.state != JOB_STATE_COMPLETED:
        time.sleep(0.05)
        job = JobStatus.FromString(client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).content)
    assert job.state == JOB_STATE_COMPLETED, (
        f"{job}\n{Path(job.stderr_path).read_text(encoding='utf-8')}"
    )
    assert sorted(job.run_digests) == sorted(run_digests)
    assert job.pid > 0
    assert Path(job.stdout_path).exists()
    assert Path(job.stderr_path).exists()
    assert len(job.artifact_digest) == 64

    repeated = client.post(
        "/v1/jobs",
        headers=PROTOBUF_HEADERS,
        content=JobRequest(run_digests=run_digests, recipe="causal-tcn-v1").SerializeToString(
            deterministic=True
        ),
    )
    assert repeated.status_code == 202
    repeated_id = JobStarted.FromString(repeated.content).job_id
    repeated_job = JobStatus.FromString(
        client.get(f"/v1/jobs/{repeated_id}", headers=AUTHORIZATION).content
    )
    while time.monotonic() < deadline + 10 and repeated_job.state == JOB_STATE_RUNNING:
        time.sleep(0.05)
        repeated_job = JobStatus.FromString(
            client.get(f"/v1/jobs/{repeated_id}", headers=AUTHORIZATION).content
        )
    assert repeated_job.state == JOB_STATE_NO_CHANGE
    assert repeated_job.artifact_digest == job.artifact_digest
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        notification_count = connection.execute(
            "SELECT count(*) FROM notifications WHERE job_id=?", (job_id,)
        ).fetchone()
        baseline_path = Path(
            connection.execute(
                "SELECT path FROM models WHERE digest=?", (job.artifact_digest,)
            ).fetchone()[0]
        )
    assert notification_count == (0,)

    baseline_path.write_bytes(b"not a model bundle")
    failed = client.post(
        "/v1/jobs",
        headers=PROTOBUF_HEADERS,
        content=JobRequest(run_digests=run_digests, recipe="causal-tcn-v1").SerializeToString(
            deterministic=True
        ),
    )
    assert failed.status_code == 202
    failed_id = JobStarted.FromString(failed.content).job_id
    deadline = time.monotonic() + 10
    failed_job = JobStatus.FromString(
        client.get(f"/v1/jobs/{failed_id}", headers=AUTHORIZATION).content
    )
    while time.monotonic() < deadline and failed_job.state in {
        JOB_STATE_QUEUED,
        JOB_STATE_RUNNING,
    }:
        time.sleep(0.05)
        failed_job = JobStatus.FromString(
            client.get(f"/v1/jobs/{failed_id}", headers=AUTHORIZATION).content
        )
    assert failed_job.state == JOB_STATE_FAILED
    assert "not a zip file" in failed_job.terminal_summary


def test_job_admission_reaps_a_dead_active_worker(tmp_path: Path, home_client: TestClient) -> None:
    run_digest = upload_run(home_client, "active-job-run")
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        connection.execute(
            "INSERT INTO jobs(id, state, recipe, input_digests_json) "
            "VALUES ('already-running', 'running', 'causal-tcn-v1', '[]')"
        )

    response = home_client.post(
        "/v1/jobs",
        headers=PROTOBUF_HEADERS,
        content=JobRequest(run_digests=[run_digest], recipe="causal-tcn-v1").SerializeToString(
            deterministic=True
        ),
    )

    assert response.status_code == 202
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        orphan = connection.execute(
            "SELECT state, terminal_summary FROM jobs WHERE id='already-running'"
        ).fetchone()
        notification = connection.execute(
            "SELECT payload_pb FROM notifications WHERE event_id='orphan:already-running'"
        ).fetchone()
    assert orphan == (
        "failed",
        "managed training worker exited before recording a terminal state",
    )
    assert (
        WebhookEvent.FromString(notification[0]).event_type == WEBHOOK_EVENT_TYPE_TRAINING_FAILURE
    )


def test_valid_but_ineligible_corpus_finishes_as_no_change(
    home_client: TestClient,
) -> None:
    client = home_client
    run_digest = upload_run(client, "single-valid-run")
    response = client.post(
        "/v1/jobs",
        headers=PROTOBUF_HEADERS,
        content=JobRequest(run_digests=[run_digest], recipe="causal-tcn-v1").SerializeToString(
            deterministic=True
        ),
    )
    assert response.status_code == 202
    job_id = JobStarted.FromString(response.content).job_id
    deadline = time.monotonic() + 10
    job = JobStatus.FromString(client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).content)
    while time.monotonic() < deadline and job.state in {JOB_STATE_QUEUED, JOB_STATE_RUNNING}:
        time.sleep(0.05)
        job = JobStatus.FromString(client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).content)
    assert job.state == JOB_STATE_NO_CHANGE
    assert job.artifact_digest == ""


def test_training_uses_only_acknowledged_control_decisions(
    tmp_path: Path, home_client: TestClient
) -> None:
    run_digests = [upload_run(home_client, "ack-run-a"), upload_run(home_client, "ack-run-b")]
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        manifests = [
            RunManifest.FromString(
                connection.execute(
                    "SELECT manifest_pb FROM runs WHERE digest=?", (run_digest,)
                ).fetchone()[0]
            )
            for run_digest in run_digests
        ]
    train_inputs, _, held_inputs, _, _ = load_training_examples(
        tmp_path, run_digests, manifests, Recipe()
    )
    command_values = {
        round(float(value), 1) for value in [*train_inputs[:, -1], *held_inputs[:, -1]]
    }
    assert command_values == {0.1, 0.5, 0.9}


def test_worker_records_candidate_rejection_and_training_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    database = tmp_path / "home.sqlite3"
    migrate(database)
    with sqlite3.connect(database) as connection:
        connection.execute(
            "INSERT INTO jobs(id, state, recipe, input_digests_json) "
            "VALUES ('fail-me', 'running', 'causal-tcn-v1', '[]')"
        )

    def fail_job(_root: Path, _job: str) -> None:
        raise RuntimeError("boom")

    monkeypatch.setattr(worker, "run", fail_job)
    monkeypatch.setattr(
        "sys.argv",
        ["celerity-home-worker", "--storage-root", str(tmp_path), "--job-id", "fail-me"],
    )
    with pytest.raises(RuntimeError, match="boom"):
        worker.main()

    with sqlite3.connect(database) as connection:
        jobs = dict(connection.execute("SELECT id, state FROM jobs").fetchall())
        events = {
            WebhookEvent.FromString(row[0]).event_type
            for row in connection.execute("SELECT payload_pb FROM notifications").fetchall()
        }
    assert jobs == {"fail-me": "failed"}
    assert events == {WEBHOOK_EVENT_TYPE_TRAINING_FAILURE}


def test_recipe_classification_covers_pass_no_change_and_reject() -> None:
    assert classify_evaluation(0.0, False) == "completed"
    assert classify_evaluation(0.0, False, improves_baseline=False) == "no_change"
    assert classify_evaluation(0.0, True) == "no_change"
    assert classify_evaluation(MAXIMUM_PARITY_ERROR, False) == "completed"
    assert classify_evaluation(MAXIMUM_PARITY_ERROR * 2, False) == "rejected"
    assert classify_evaluation(float("nan"), False) == "rejected"


def test_restart_recovers_an_orphaned_durable_job(tmp_path: Path, home_client: TestClient) -> None:
    client = home_client
    run_digests = [upload_run(client, "run-recovery-a"), upload_run(client, "run-recovery-b")]
    jobs = tmp_path / "jobs"
    jobs.mkdir(exist_ok=True)
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        connection.execute(
            "INSERT INTO jobs(id, state, recipe, input_digests_json, stdout_path, stderr_path) "
            "VALUES (?, 'queued', 'causal-tcn-v1', ?, ?, ?)",
            (
                "recover-me",
                json.dumps(run_digests),
                str(jobs / "recover-me.stdout"),
                str(jobs / "recover-me.stderr"),
            ),
        )

    with TestClient(create_app(tmp_path, "secret", WEBHOOK_URL)) as restarted:
        deadline = time.monotonic() + 10
        job = JobStatus.FromString(
            restarted.get("/v1/jobs/recover-me", headers=AUTHORIZATION).content
        )
        while time.monotonic() < deadline and job.state != JOB_STATE_COMPLETED:
            time.sleep(0.05)
            job = JobStatus.FromString(
                restarted.get("/v1/jobs/recover-me", headers=AUTHORIZATION).content
            )
        assert job.state == JOB_STATE_COMPLETED, (
            f"{job}\n{Path(job.stderr_path).read_text(encoding='utf-8')}"
        )
        assert job.pid > 0
