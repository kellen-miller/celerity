"""Managed Home training-job lifecycle."""

from __future__ import annotations

import json
import sqlite3
import subprocess
import sys
from pathlib import Path

from celerity.v1.celerity_pb2 import (
    JOB_STATE_COMPLETED,
    JOB_STATE_FAILED,
    JOB_STATE_NO_CHANGE,
    JOB_STATE_QUEUED,
    JOB_STATE_REJECTED,
    JOB_STATE_RUNNING,
    JobStatus,
    WEBHOOK_EVENT_TYPE_TRAINING_FAILURE,
)
from celerity_home.database import connect, transaction
from celerity_home.notifications import webhook_event


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


def launch_managed_job(
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
    with transaction(database) as connection:
        connection.execute(
            "UPDATE jobs SET state='running', pid=?, pid_start_ticks=? "
            "WHERE id=? AND state IN ('queued', 'running')",
            (process.pid, _process_start_ticks(process.pid), job_id),
        )


def recover_managed_jobs(storage_root: Path) -> int:
    """Launch queued jobs and fail dead running jobs without blocking future work."""
    database = storage_root / "home.sqlite3"
    mark_orphaned_jobs_failed(database)
    with connect(database) as connection:
        jobs = connection.execute(
            "SELECT id, pid, pid_start_ticks, stdout_path, stderr_path FROM jobs "
            "WHERE state='queued' ORDER BY created_at, id"
        ).fetchall()
    recovered = 0
    for job in jobs:
        if _owned_process_is_running(job["pid"], job["pid_start_ticks"]):
            continue
        launch_managed_job(
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
    with transaction(database) as connection:
        jobs = connection.execute(
            "SELECT id, pid, pid_start_ticks FROM jobs WHERE state='running'"
        ).fetchall()
        orphaned = [
            job for job in jobs if not _owned_process_is_running(job["pid"], job["pid_start_ticks"])
        ]
        for job in orphaned:
            summary = "managed training worker exited before recording a terminal state"
            connection.execute(
                "UPDATE jobs SET state='failed', terminal_summary=? WHERE id=? AND state='running'",
                (summary, job["id"]),
            )
            connection.execute(
                "INSERT OR IGNORE INTO notifications"
                "(event_id, job_id, payload_pb, state) VALUES (?, ?, ?, 'pending')",
                (
                    f"orphan:{job['id']}",
                    job["id"],
                    webhook_event(
                        f"orphan:{job['id']}",
                        WEBHOOK_EVENT_TYPE_TRAINING_FAILURE,
                        summary,
                        job_id=job["id"],
                    ),
                ),
            )
    return len(orphaned)


def job_status(row: sqlite3.Row) -> JobStatus:
    states = {
        "queued": JOB_STATE_QUEUED,
        "running": JOB_STATE_RUNNING,
        "completed": JOB_STATE_COMPLETED,
        "no_change": JOB_STATE_NO_CHANGE,
        "rejected": JOB_STATE_REJECTED,
        "failed": JOB_STATE_FAILED,
    }
    status = JobStatus(
        id=row["id"],
        state=states[row["state"]],
        recipe=row["recipe"],
        run_digests=json.loads(row["input_digests_json"]),
        created_at=row["created_at"],
    )
    for field in ("pid", "stdout_path", "stderr_path", "artifact_digest", "terminal_summary"):
        if row[field] is not None:
            setattr(status, field, row[field])
    return status
