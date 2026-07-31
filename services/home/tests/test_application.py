from __future__ import annotations

import hashlib
import json
import sqlite3
import time
from collections.abc import Iterator
from pathlib import Path

import httpx
import pytest
import yaml
from fastapi.testclient import TestClient

from celerity_home import worker
from celerity_home.application import create_app, deliver_notifications, migrate
from celerity_home.worker import classify_evaluation

AUTHORIZATION = {"Authorization": "Bearer secret"}
WEBHOOK_URL = "https://webhook.invalid/celerity"


@pytest.fixture
def home_client(tmp_path: Path) -> Iterator[TestClient]:
    with TestClient(create_app(tmp_path, "secret", WEBHOOK_URL)) as client:
        yield client


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def upload_run(client: TestClient, run_name: str) -> str:
    chunk = f"events-{run_name}".encode()
    chunk_digest = digest(chunk)
    manifest = json.dumps(
        {
            "schema_version": 1,
            "run_id": run_name,
            "completion": "complete",
            "chunk_file": "events.chunk",
            "chunk_sha256": chunk_digest,
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
    run_digest = upload_run(client, "run-training")
    admitted = client.post(
        "/v1/jobs",
        headers=AUTHORIZATION,
        json={"run_digests": [run_digest], "recipe": "causal-tcn-v1"},
    )
    assert admitted.status_code == 202
    job_id = admitted.json()["job_id"]
    deadline = time.monotonic() + 10
    job = client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).json()
    while time.monotonic() < deadline and job["state"] != "completed":
        time.sleep(0.05)
        job = client.get(f"/v1/jobs/{job_id}", headers=AUTHORIZATION).json()
    assert job["state"] == "completed"
    assert job["pid"] > 0
    assert Path(job["stdout_path"]).exists()
    assert Path(job["stderr_path"]).exists()
    assert len(job["artifact_digest"]) == 64

    repeated = client.post(
        "/v1/jobs",
        headers=AUTHORIZATION,
        json={"run_digests": [run_digest], "recipe": "causal-tcn-v1"},
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
    assert notification_count == (0,)


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
            "VALUES ('reject-me', 'running', 'causal-tcn-v1', '[]')"
        )
        connection.execute(
            "INSERT INTO jobs(id, state, recipe, input_digests_json) "
            "VALUES ('fail-me', 'running', 'causal-tcn-v1', '[]')"
        )

    def reject_candidate(path: Path) -> dict[str, object]:
        path.write_bytes(b"rejected-candidate")
        return {"maximum_parity_error": 1.0}

    monkeypatch.setattr(worker, "export_and_compare", reject_candidate)
    worker.run(tmp_path, "reject-me")

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
    assert jobs == {"reject-me": "rejected", "fail-me": "failed"}
    assert events == {"candidate_rejection", "training_failure"}


def test_recipe_classification_covers_pass_no_change_and_reject() -> None:
    assert classify_evaluation(0.0, False) == "completed"
    assert classify_evaluation(0.0, True) == "no_change"
    assert classify_evaluation(1e-4, False) == "rejected"
    assert classify_evaluation(float("nan"), False) == "rejected"


def test_restart_recovers_an_orphaned_durable_job(tmp_path: Path, home_client: TestClient) -> None:
    client = home_client
    run_digest = upload_run(client, "run-recovery")
    jobs = tmp_path / "jobs"
    jobs.mkdir(exist_ok=True)
    with sqlite3.connect(tmp_path / "home.sqlite3") as connection:
        connection.execute(
            "INSERT INTO jobs(id, state, recipe, input_digests_json, stdout_path, stderr_path) "
            "VALUES (?, 'queued', 'causal-tcn-v1', ?, ?, ?)",
            (
                "recover-me",
                json.dumps([run_digest]),
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
        assert job["state"] == "completed"
        assert job["pid"] > 0
