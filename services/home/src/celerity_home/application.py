"""Token-authenticated immutable Run, model, job, and notification lifecycle."""

from __future__ import annotations

import hashlib
import hmac
import json
import os
import sqlite3
import subprocess
import sys
import threading
import uuid
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager
from datetime import UTC, datetime
from pathlib import Path
from typing import Annotated, Literal

import httpx
from fastapi import Depends, FastAPI, Header, HTTPException, Request, Response
from fastapi import Path as FastAPIPath
from fastapi.responses import FileResponse
from pydantic import BaseModel, ConfigDict

from celerity_home.database import connect as _connect
from celerity_home.database import migrate
from celerity_home.database import transaction as _transaction
from celerity_home.notifications import deliver_notifications
from celerity_home.run import RunFormatError, decode_run_records

Digest = Annotated[str, FastAPIPath(pattern=r"^[0-9a-f]{64}$")]


class ReconcileRequest(BaseModel):
    """Exact vehicle content inventory."""

    model_config = ConfigDict(extra="forbid")
    schema_version: int
    completed_run_digests: list[str]
    active_model_digest: str | None
    staged_model_digest: str | None
    rejected_model_digests: list[str]


class JobRequest(BaseModel):
    """Immutable inputs for one managed training job."""

    model_config = ConfigDict(extra="forbid")
    run_digests: list[str]
    recipe: Literal["causal-tcn-v1"]


def _digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _is_digest(value: object) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def _process_start_ticks(pid: int) -> str | None:
    try:
        fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
        return fields[19]
    except (FileNotFoundError, IndexError, OSError):
        return None


def _owned_process_is_running(pid: int | None, expected_start_ticks: str | None) -> bool:
    if pid is None or expected_start_ticks is None:
        return False
    return _process_start_ticks(pid) == expected_start_ticks


def _launch_managed_job(
    storage_root: Path,
    database: Path,
    job_id: str,
    stdout_path: Path,
    stderr_path: Path,
) -> None:
    with stdout_path.open("ab") as stdout, stderr_path.open("ab") as stderr:
        process = subprocess.Popen(
            [
                sys.executable,
                "-m",
                "celerity_home.worker",
                "--storage-root",
                str(storage_root),
                "--job-id",
                job_id,
            ],
            stdout=stdout,
            stderr=stderr,
        )
    with _transaction(database) as connection:
        connection.execute(
            "UPDATE jobs SET state='running', pid=?, pid_start_ticks=? "
            "WHERE id=? AND state IN ('queued', 'running')",
            (process.pid, _process_start_ticks(process.pid), job_id),
        )


def recover_managed_jobs(storage_root: Path) -> int:
    """Launch queued jobs and fail dead running jobs without blocking future work."""
    database = storage_root / "home.sqlite3"
    mark_orphaned_jobs_failed(database)
    with _connect(database) as connection:
        jobs = connection.execute(
            "SELECT id, pid, pid_start_ticks, stdout_path, stderr_path FROM jobs "
            "WHERE state='queued' ORDER BY created_at, id"
        ).fetchall()
    recovered = 0
    for job in jobs:
        if _owned_process_is_running(job["pid"], job["pid_start_ticks"]):
            continue
        _launch_managed_job(
            storage_root,
            database,
            job["id"],
            Path(job["stdout_path"]),
            Path(job["stderr_path"]),
        )
        recovered += 1
    return recovered


def mark_orphaned_jobs_failed(database: Path) -> int:
    """Fail running jobs whose owned worker no longer exists."""
    with _transaction(database) as connection:
        jobs = connection.execute(
            "SELECT id, pid, pid_start_ticks FROM jobs WHERE state='running'"
        ).fetchall()
        orphaned = [
            job for job in jobs if not _owned_process_is_running(job["pid"], job["pid_start_ticks"])
        ]
        for job in orphaned:
            payload = {
                "schema_version": 1,
                "event_id": f"orphan:{job['id']}",
                "event_type": "training_failure",
                "occurred_at": datetime.now(UTC).isoformat(),
                "job_id": job["id"],
                "artifact_digest": None,
                "summary": "managed training worker exited before recording a terminal state",
            }
            connection.execute(
                "UPDATE jobs SET state='failed', terminal_summary=? WHERE id=? AND state='running'",
                (payload["summary"], job["id"]),
            )
            connection.execute(
                "INSERT OR IGNORE INTO notifications"
                "(event_id, job_id, payload_json, state) VALUES (?, ?, ?, 'pending')",
                (payload["event_id"], job["id"], json.dumps(payload, sort_keys=True)),
            )
    return len(orphaned)


def create_app(storage_root: Path, vehicle_token: str, webhook_url: str) -> FastAPI:
    """Create the one-replica home API over filesystem storage and SQLite."""
    if not webhook_url.startswith("https://"):
        raise ValueError("CELERITY_HOME_WEBHOOK_URL must use HTTPS")
    storage_root.mkdir(parents=True, exist_ok=True)
    database = storage_root / "home.sqlite3"
    migrate(database)
    recover_managed_jobs(storage_root)

    @asynccontextmanager
    async def lifespan(_app: FastAPI) -> AsyncIterator[None]:
        stop_delivery = threading.Event()

        def deliver_pending_notifications() -> None:
            with httpx.Client(timeout=5) as client:
                while not stop_delivery.is_set():
                    try:
                        deliver_notifications(database, webhook_url, client)
                    except sqlite3.OperationalError as error:
                        if not database.parent.exists():
                            return
                        print(f"webhook delivery deferred: {error}", file=sys.stderr)
                    stop_delivery.wait(5)

        delivery_thread = threading.Thread(
            target=deliver_pending_notifications,
            name="celerity-webhook-delivery",
            daemon=True,
        )
        delivery_thread.start()
        try:
            yield
        finally:
            stop_delivery.set()
            delivery_thread.join(timeout=6)

    app = FastAPI(title="Celerity home", version="1.0.0", lifespan=lifespan)

    @app.get("/health/live")
    def live() -> dict[str, str]:
        return {"status": "live"}

    @app.get("/health/ready")
    def ready() -> dict[str, str]:
        with _connect(database) as connection:
            connection.execute("SELECT 1").fetchone()
        return {"status": "ready"}

    def authenticate(authorization: str | None = Header(default=None)) -> None:
        if authorization is None or not hmac.compare_digest(
            authorization, f"Bearer {vehicle_token}"
        ):
            raise HTTPException(status_code=401, detail="invalid vehicle token")

    @app.post("/v1/reconcile", dependencies=[Depends(authenticate)])
    def reconcile(request: ReconcileRequest) -> dict[str, object]:
        if request.schema_version != 1:
            raise HTTPException(status_code=422, detail="unsupported schema_version")
        recover_managed_jobs(storage_root)
        training_job: tuple[str, Path, Path] | None = None
        with _transaction(database) as connection:
            present = {
                row["digest"] for row in connection.execute("SELECT digest FROM runs").fetchall()
            }
            rejected = sorted(set(request.rejected_model_digests))
            for digest in rejected:
                model = connection.execute(
                    "SELECT state FROM models WHERE digest=?", (digest,)
                ).fetchone()
                if model is None or model["state"] == "rejected":
                    continue
                connection.execute("UPDATE models SET state='rejected' WHERE digest=?", (digest,))
                event_id = f"demotion:{digest}"
                payload = {
                    "schema_version": 1,
                    "event_id": event_id,
                    "event_type": "demotion",
                    "occurred_at": datetime.now(UTC).isoformat(),
                    "job_id": None,
                    "artifact_digest": digest,
                    "summary": "vehicle rejected staged model evidence",
                }
                connection.execute(
                    "INSERT OR IGNORE INTO notifications"
                    "(event_id, job_id, payload_json, state) VALUES (?, NULL, ?, 'pending')",
                    (event_id, json.dumps(payload, sort_keys=True)),
                )
            desired = connection.execute(
                "SELECT value FROM settings WHERE key='desired_model_digest'"
            ).fetchone()
            desired_digest = None if desired is None else desired["value"]
            if desired_digest in rejected:
                demoted_digest = desired_digest
                known_good = connection.execute(
                    "SELECT digest FROM models WHERE state='staged' "
                    "ORDER BY created_at DESC, digest DESC LIMIT 1"
                ).fetchone()
                if known_good is None:
                    connection.execute("DELETE FROM settings WHERE key='desired_model_digest'")
                    desired_digest = None
                else:
                    desired_digest = known_good["digest"]
                    connection.execute(
                        "UPDATE settings SET value=? WHERE key='desired_model_digest'",
                        (desired_digest,),
                    )
                    event_id = f"rollback:{demoted_digest}:{desired_digest}"
                    payload = {
                        "schema_version": 1,
                        "event_id": event_id,
                        "event_type": "rollback",
                        "occurred_at": datetime.now(UTC).isoformat(),
                        "job_id": None,
                        "artifact_digest": desired_digest,
                        "summary": "known-good model selected after vehicle rejection",
                    }
                    connection.execute(
                        "INSERT OR IGNORE INTO notifications"
                        "(event_id, job_id, payload_json, state) "
                        "VALUES (?, NULL, ?, 'pending')",
                        (event_id, json.dumps(payload, sort_keys=True)),
                    )
            reported = sorted(set(request.completed_run_digests))
            if reported and set(reported).issubset(present):
                contract_fields = (
                    "model_abi",
                    "model_input_signals",
                    "model_history_length",
                    "sample_period_ms",
                    "command_lattice",
                    "maximum_calibration_error",
                )
                manifests = {
                    row["digest"]: json.loads(row["manifest_json"])
                    for row in connection.execute(
                        "SELECT digest, manifest_json FROM runs"
                    ).fetchall()
                }
                reported_contracts = {
                    json.dumps(
                        [manifests[digest].get(field) for field in contract_fields],
                        sort_keys=True,
                    )
                    for digest in reported
                }
                if len(reported_contracts) != 1:
                    raise HTTPException(status_code=409, detail="reported Run contracts disagree")
                contract = reported_contracts.pop()
                corpus = sorted(
                    digest
                    for digest, manifest in manifests.items()
                    if json.dumps(
                        [manifest.get(field) for field in contract_fields], sort_keys=True
                    )
                    == contract
                )
                active_job = connection.execute(
                    "SELECT 1 FROM jobs WHERE state IN ('queued', 'running') LIMIT 1"
                ).fetchone()
                latest_job = connection.execute(
                    "SELECT input_digests_json FROM jobs ORDER BY created_at DESC, id DESC LIMIT 1"
                ).fetchone()
                latest_inputs = [] if latest_job is None else json.loads(latest_job[0])
                if active_job is None and latest_inputs != corpus:
                    job_id = str(uuid.uuid4())
                    jobs = storage_root / "jobs"
                    jobs.mkdir(parents=True, exist_ok=True)
                    stdout_path = jobs / f"{job_id}.stdout"
                    stderr_path = jobs / f"{job_id}.stderr"
                    connection.execute(
                        "INSERT INTO jobs(id, state, recipe, input_digests_json, stdout_path, "
                        "stderr_path) VALUES (?, 'queued', 'causal-tcn-v1', ?, ?, ?)",
                        (
                            job_id,
                            json.dumps(corpus),
                            str(stdout_path),
                            str(stderr_path),
                        ),
                    )
                    training_job = (job_id, stdout_path, stderr_path)
        if training_job is not None:
            _launch_managed_job(storage_root, database, *training_job)
        return {
            "missing_run_digests": sorted(set(request.completed_run_digests) - present),
            "desired_model_digest": desired_digest,
        }

    @app.put(
        "/v1/runs/{run_digest}/chunks/{chunk_digest}",
        status_code=204,
        dependencies=[Depends(authenticate)],
    )
    async def put_chunk(run_digest: Digest, chunk_digest: Digest, request: Request) -> Response:
        data = await request.body()
        if _digest(data) != chunk_digest:
            raise HTTPException(status_code=422, detail="chunk digest mismatch")
        directory = storage_root / "runs" / run_digest
        directory.mkdir(parents=True, exist_ok=True)
        path = directory / chunk_digest
        if path.exists() and path.read_bytes() != data:
            raise HTTPException(status_code=409, detail="immutable chunk conflict")
        temporary = directory / f".{chunk_digest}.staging"
        with temporary.open("wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(path)
        with _transaction(database) as connection:
            connection.execute(
                "INSERT OR IGNORE INTO chunks(run_digest, digest, path) VALUES (?, ?, ?)",
                (run_digest, chunk_digest, str(path)),
            )
        return Response(status_code=204)

    @app.post("/v1/runs/{run_digest}/complete", dependencies=[Depends(authenticate)])
    async def complete_run(run_digest: Digest, request: Request) -> dict[str, str]:
        manifest_bytes = await request.body()
        if _digest(manifest_bytes) != run_digest:
            raise HTTPException(status_code=422, detail="Run digest mismatch")
        try:
            manifest = json.loads(manifest_bytes)
            chunks = manifest.get("chunks")
            if not isinstance(chunks, list) or not chunks:
                chunks = [{"sha256": manifest["chunk_sha256"]}]
        except (json.JSONDecodeError, KeyError, TypeError) as error:
            raise HTTPException(status_code=422, detail="invalid Run manifest") from error
        if manifest.get("schema_version") != 1 or manifest.get("completion") != "complete":
            raise HTTPException(
                status_code=422, detail="only complete Run v1 manifests are admitted"
            )
        last_sequence: int | None = None
        last_monotonic_ns: int | None = None
        for chunk in chunks:
            if not isinstance(chunk, dict) or not _is_digest(chunk.get("sha256")):
                raise HTTPException(status_code=422, detail="invalid Run chunk manifest")
            chunk_digest = chunk["sha256"]
            chunk_path = storage_root / "runs" / run_digest / chunk_digest
            chunk_bytes = chunk_path.read_bytes() if chunk_path.exists() else b""
            if not chunk_bytes or _digest(chunk_bytes) != chunk_digest:
                raise HTTPException(status_code=422, detail="manifest chunk is absent or corrupt")
            try:
                records = decode_run_records(chunk_bytes)
            except RunFormatError as error:
                raise HTTPException(
                    status_code=422, detail=f"invalid Run chunk: {error}"
                ) from error
            if last_sequence is not None and records[0].sequence <= last_sequence:
                raise HTTPException(status_code=422, detail="Run sequence regressed across chunks")
            if last_monotonic_ns is not None and records[0].monotonic_ns < last_monotonic_ns:
                raise HTTPException(status_code=422, detail="Run time regressed across chunks")
            last_sequence = records[-1].sequence
            last_monotonic_ns = records[-1].monotonic_ns
        with _transaction(database) as connection:
            existing = connection.execute(
                "SELECT manifest_json FROM runs WHERE digest=?", (run_digest,)
            ).fetchone()
            if existing is not None and existing["manifest_json"] != manifest_bytes:
                raise HTTPException(status_code=409, detail="immutable Run conflict")
            connection.execute(
                "INSERT OR IGNORE INTO runs(digest, manifest_json) VALUES (?, ?)",
                (run_digest, manifest_bytes),
            )
        return {"run_digest": run_digest}

    @app.get("/v1/models/{digest}", dependencies=[Depends(authenticate)])
    def get_model(digest: Digest) -> FileResponse:
        with _connect(database) as connection:
            model = connection.execute(
                "SELECT path FROM models WHERE digest=? AND state='staged'", (digest,)
            ).fetchone()
        if model is None:
            raise HTTPException(status_code=404, detail="model not found")
        return FileResponse(model["path"], media_type="application/octet-stream")

    @app.post("/v1/jobs", status_code=202, dependencies=[Depends(authenticate)])
    def start_job(request: JobRequest) -> dict[str, str]:
        if not request.run_digests:
            raise HTTPException(status_code=422, detail="job requires at least one Run")
        recover_managed_jobs(storage_root)
        with _transaction(database) as connection:
            if connection.execute(
                "SELECT 1 FROM jobs WHERE state IN ('queued', 'running') LIMIT 1"
            ).fetchone():
                raise HTTPException(status_code=409, detail="another training job is active")
            admitted = {
                row["digest"]
                for row in connection.execute(
                    "SELECT digest FROM runs WHERE digest IN "
                    f"({','.join('?' for _ in request.run_digests)})",
                    request.run_digests,
                ).fetchall()
            }
            missing = sorted(set(request.run_digests) - admitted)
            if missing:
                raise HTTPException(status_code=409, detail={"incomplete_runs": missing})
            job_id = str(uuid.uuid4())
            jobs = storage_root / "jobs"
            jobs.mkdir(parents=True, exist_ok=True)
            stdout_path = jobs / f"{job_id}.stdout"
            stderr_path = jobs / f"{job_id}.stderr"
            connection.execute(
                "INSERT INTO jobs(id, state, recipe, input_digests_json, stdout_path, stderr_path) "
                "VALUES (?, 'queued', ?, ?, ?, ?)",
                (
                    job_id,
                    request.recipe,
                    json.dumps(sorted(request.run_digests)),
                    str(stdout_path),
                    str(stderr_path),
                ),
            )
        _launch_managed_job(storage_root, database, job_id, stdout_path, stderr_path)
        return {"job_id": job_id, "state": "running"}

    @app.get("/v1/jobs/{job_id}", dependencies=[Depends(authenticate)])
    def get_job(job_id: str) -> dict[str, object]:
        with _connect(database) as connection:
            job = connection.execute("SELECT * FROM jobs WHERE id=?", (job_id,)).fetchone()
        if job is None:
            raise HTTPException(status_code=404, detail="job not found")
        return dict(job)

    return app
