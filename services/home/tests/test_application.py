from __future__ import annotations

import hashlib
import json
import sqlite3
import struct
import time
from collections.abc import Iterator
from pathlib import Path

import httpx
import pytest
import yaml
from fastapi.testclient import TestClient

from celerity_home import worker
from celerity_home.application import create_app, deliver_notifications, migrate
from celerity_home.training import MAXIMUM_PARITY_ERROR, Recipe, load_training_examples
from celerity_home.worker import classify_evaluation

AUTHORIZATION = {"Authorization": "Bearer secret"}
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


def typed_record(sequence: int, payload_field: int, payload: bytes) -> bytes:
    source = b"test"
    message = (
        varint(1 << 3)
        + varint(1)
        + varint(4 << 3)
        + varint(sequence)
        + varint(5 << 3)
        + varint(sequence * 1_000_000)
        + varint((8 << 3) | 2)
        + varint(len(source))
        + source
        + varint((payload_field << 3) | 2)
        + varint(len(payload))
        + payload
    )
    return varint(len(message)) + message


def signal_record(sequence: int, signal: str, value: float) -> bytes:
    name = signal.encode()
    payload = (
        varint((1 << 3) | 2)
        + varint(len(name))
        + name
        + varint((2 << 3) | 1)
        + struct.pack("<d", value)
    )
    return typed_record(sequence, 21, payload)


def control_record(sequence: int, split: int, state: str) -> bytes:
    encoded = json.dumps({"state": state, "radiator_split_basis_points": split}).encode()
    payload = varint((1 << 3) | 2) + varint(len(encoded)) + encoded
    return typed_record(sequence, 24, payload)


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
    manifest = json.dumps(
        {
            "schema_version": 1,
            "run_id": run_name,
            "completion": "complete",
            "chunk_file": "events.chunk",
            "chunk_sha256": chunk_digest,
            "configuration_generation": 1,
            "configuration_sha256": "a" * 64,
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
    run_digest = digest(manifest)
    response = client.put(
        f"/v1/runs/{run_digest}/chunks/{chunk_digest}", content=chunk, headers=AUTHORIZATION
    )
    assert response.status_code == 204
    response = client.post(
        f"/v1/runs/{run_digest}/complete", content=manifest, headers=AUTHORIZATION
    )
    assert response.status_code == 200
    assert response.json() == {"run_digest": run_digest}
    return run_digest


def test_token_auth_and_idempotent_run_admission(home_client: TestClient) -> None:
    client = home_client
    assert client.post("/v1/reconcile", json={}).status_code == 401
    run_digest = upload_run(client, "run-a")
    assert upload_run(client, "run-a") == run_digest
    response = client.post(
        "/v1/reconcile",
        headers=AUTHORIZATION,
        json={
            "schema_version": 1,
            "completed_run_digests": [run_digest, "f" * 64],
            "active_model_digest": None,
            "staged_model_digest": None,
            "rejected_model_digests": [],
        },
    )
    assert response.status_code == 200
    assert response.json()["missing_run_digests"] == ["f" * 64]


def test_run_admission_rejects_nonmonotonic_records(
    tmp_path: Path, home_client: TestClient
) -> None:
    first = control_record(2, 5_000, "accepted")
    second = control_record(1, 5_000, "accepted")
    chunk = first + second
    chunk_digest = digest(chunk)
    manifest = json.dumps(
        {
            "schema_version": 1,
            "completion": "complete",
            "chunk_sha256": chunk_digest,
        },
        sort_keys=True,
    ).encode()
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
    repository = Path(__file__).resolve().parents[3]
    contract = yaml.safe_load((repository / "contracts/home/v1/openapi.yaml").read_text())
    application_paths = set(create_app(tmp_path, "secret", WEBHOOK_URL).openapi()["paths"])
    assert set(contract["paths"]) == application_paths
    assert contract["paths"]["/health/live"]["get"]["security"] == []
    assert contract["paths"]["/health/ready"]["get"]["security"] == []

    webhook = json.loads((repository / "contracts/home/v1/webhook.schema.json").read_text())
    assert webhook["properties"]["event_type"]["enum"] == [
        "training_failure",
        "candidate_rejection",
        "demotion",
        "rollback",
    ]


def test_job_admits_only_complete_runs_and_survives_as_durable_subprocess(
    tmp_path: Path, home_client: TestClient
) -> None:
    client = home_client
    assert client.post("/v1/jobs", json={}).status_code == 401
    rejected = client.post(
        "/v1/jobs",
        headers=AUTHORIZATION,
        json={"run_digests": ["0" * 64], "recipe": "causal-tcn-v1"},
    )
    assert rejected.status_code == 409
    run_digests = [upload_run(client, "run-training-a"), upload_run(client, "run-training-b")]
    admitted = client.post(
        "/v1/jobs",
        headers=AUTHORIZATION,
        json={"run_digests": run_digests, "recipe": "causal-tcn-v1"},
    )
    assert admitted.status_code == 202
    job_id = admitted.json()["job_id"]
    deadline = time.monotonic() + 10
    job = client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).json()
    while time.monotonic() < deadline and job["state"] != "completed":
        time.sleep(0.05)
        job = client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).json()
    assert job["state"] == "completed", (
        f"{job}\n{Path(job['stderr_path']).read_text(encoding='utf-8')}"
    )
    assert job["pid"] > 0
    assert Path(job["stdout_path"]).exists()
    assert Path(job["stderr_path"]).exists()
    assert len(job["artifact_digest"]) == 64

    repeated = client.post(
        "/v1/jobs",
        headers=AUTHORIZATION,
        json={"run_digests": run_digests, "recipe": "causal-tcn-v1"},
    )
    assert repeated.status_code == 202
    repeated_id = repeated.json()["job_id"]
    repeated_job = client.get(f"/v1/jobs/{repeated_id}", headers=AUTHORIZATION).json()
    while time.monotonic() < deadline + 10 and repeated_job["state"] == "running":
        time.sleep(0.05)
        repeated_job = client.get(f"/v1/jobs/{repeated_id}", headers=AUTHORIZATION).json()
    assert repeated_job["state"] == "no_change"
    assert repeated_job["artifact_digest"] == job["artifact_digest"]
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        notification_count = connection.execute(
            "SELECT count(*) FROM notifications WHERE job_id=?", (job_id,)
        ).fetchone()
        baseline_path = Path(
            connection.execute(
                "SELECT path FROM models WHERE digest=?", (job["artifact_digest"],)
            ).fetchone()[0]
        )
    assert notification_count == (0,)

    baseline_path.write_bytes(b"not a model bundle")
    failed = client.post(
        "/v1/jobs",
        headers=AUTHORIZATION,
        json={"run_digests": run_digests, "recipe": "causal-tcn-v1"},
    )
    assert failed.status_code == 202
    failed_id = failed.json()["job_id"]
    deadline = time.monotonic() + 10
    failed_job = client.get(f"/v1/jobs/{failed_id}", headers=AUTHORIZATION).json()
    while time.monotonic() < deadline and failed_job["state"] in {"queued", "running"}:
        time.sleep(0.05)
        failed_job = client.get(f"/v1/jobs/{failed_id}", headers=AUTHORIZATION).json()
    assert failed_job["state"] == "failed"
    assert "not a zip file" in failed_job["terminal_summary"]


def test_job_admission_reaps_a_dead_active_worker(tmp_path: Path, home_client: TestClient) -> None:
    run_digest = upload_run(home_client, "active-job-run")
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        connection.execute(
            "INSERT INTO jobs(id, state, recipe, input_digests_json) "
            "VALUES ('already-running', 'running', 'causal-tcn-v1', '[]')"
        )

    response = home_client.post(
        "/v1/jobs",
        headers=AUTHORIZATION,
        json={"run_digests": [run_digest], "recipe": "causal-tcn-v1"},
    )

    assert response.status_code == 202
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        orphan = connection.execute(
            "SELECT state, terminal_summary FROM jobs WHERE id='already-running'"
        ).fetchone()
        notification = connection.execute(
            "SELECT payload_json FROM notifications WHERE event_id='orphan:already-running'"
        ).fetchone()
    assert orphan == (
        "failed",
        "managed training worker exited before recording a terminal state",
    )
    assert json.loads(notification[0])["event_type"] == "training_failure"


def test_valid_but_ineligible_corpus_finishes_as_no_change(
    home_client: TestClient,
) -> None:
    client = home_client
    run_digest = upload_run(client, "single-valid-run")
    response = client.post(
        "/v1/jobs",
        headers=AUTHORIZATION,
        json={"run_digests": [run_digest], "recipe": "causal-tcn-v1"},
    )
    assert response.status_code == 202
    job_id = response.json()["job_id"]
    deadline = time.monotonic() + 10
    job = client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).json()
    while time.monotonic() < deadline and job["state"] in {"queued", "running"}:
        time.sleep(0.05)
        job = client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).json()
    assert job["state"] == "no_change"
    assert job["artifact_digest"] is None


def test_training_uses_only_acknowledged_control_decisions(
    tmp_path: Path, home_client: TestClient
) -> None:
    run_digests = [upload_run(home_client, "ack-run-a"), upload_run(home_client, "ack-run-b")]
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        manifests = [
            json.loads(
                connection.execute(
                    "SELECT manifest_json FROM runs WHERE digest=?", (run_digest,)
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


def test_webhook_failure_never_changes_completed_job(tmp_path: Path) -> None:
    database = tmp_path / "home.sqlite3"
    migrate(database)
    with sqlite3.connect(database) as connection:
        connection.execute(
            "INSERT INTO jobs(id, state, recipe, input_digests_json, terminal_summary) "
            "VALUES ('job-1', 'completed', 'causal-tcn-v1', '[]', 'model staged')"
        )
        connection.execute(
            "INSERT INTO notifications(event_id, job_id, payload_json, state) "
            "VALUES ('event-1', 'job-1', '{}', 'pending')"
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
        headers=AUTHORIZATION,
        json={
            "schema_version": 1,
            "completed_run_digests": [],
            "active_model_digest": rejected_digest,
            "staged_model_digest": None,
            "rejected_model_digests": [rejected_digest],
        },
    )
    assert response.status_code == 200
    assert response.json()["desired_model_digest"] == known_good_digest
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        state = connection.execute(
            "SELECT state FROM models WHERE digest=?", (rejected_digest,)
        ).fetchone()
        events = connection.execute(
            "SELECT payload_json FROM notifications ORDER BY event_id"
        ).fetchall()
    assert state == ("rejected",)
    assert {json.loads(event[0])["event_type"] for event in events} == {
        "demotion",
        "rollback",
    }


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
            json.loads(row[0])["event_type"]
            for row in connection.execute("SELECT payload_json FROM notifications").fetchall()
        }
    assert jobs == {"fail-me": "failed"}
    assert events == {"training_failure"}


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
        job = restarted.get("/v1/jobs/recover-me", headers=AUTHORIZATION).json()
        while time.monotonic() < deadline and job["state"] != "completed":
            time.sleep(0.05)
            job = restarted.get("/v1/jobs/recover-me", headers=AUTHORIZATION).json()
        assert job["state"] == "completed", (
            f"{job}\n{Path(job['stderr_path']).read_text(encoding='utf-8')}"
        )
        assert job["pid"] > 0
