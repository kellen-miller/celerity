"""Managed subprocess for one durable home job."""

from __future__ import annotations

import argparse
import hashlib
import json
import sqlite3
from datetime import UTC, datetime
from pathlib import Path

from celerity_home.training import derive_run_index, export_and_compare


def classify_evaluation(maximum_parity_error: float, already_staged: bool) -> str:
    """Classify a bounded recipe result before changing desired model state."""
    if not 0 <= maximum_parity_error <= 1e-5:
        return "rejected"
    if already_staged:
        return "no_change"
    return "completed"


def run(storage_root: Path, job_id: str) -> None:
    """Produce a deterministic fixture bundle and atomically record terminal state."""
    database = storage_root / "home.sqlite3"
    connection = sqlite3.connect(database)
    connection.row_factory = sqlite3.Row
    job = connection.execute("SELECT * FROM jobs WHERE id=?", (job_id,)).fetchone()
    if job is None:
        raise RuntimeError(f"unknown job {job_id}")
    output = storage_root / "jobs" / job_id
    output.mkdir(parents=True, exist_ok=True)
    run_digests = json.loads(job["input_digests_json"])
    derivation_digest = derive_run_index(run_digests, output / "derived.parquet")
    model_path = output / "candidate.onnx"
    evaluation = export_and_compare(model_path)
    model_bytes = model_path.read_bytes()
    digest = hashlib.sha256(model_bytes).hexdigest()
    model_path = model_path.replace(output / f"{digest}.onnx")
    (output / "evaluation.json").write_text(
        json.dumps(
            {**evaluation, "derivation_digest": derivation_digest, "input_runs": run_digests},
            indent=2,
            sort_keys=True,
        )
    )
    existing = connection.execute("SELECT state FROM models WHERE digest=?", (digest,)).fetchone()
    state = classify_evaluation(
        float(evaluation["maximum_parity_error"]),
        existing is not None and existing["state"] == "staged",
    )
    with connection:
        if state == "completed":
            connection.execute(
                "INSERT INTO models(digest, path, state) VALUES (?, ?, 'staged')",
                (digest, str(model_path)),
            )
            connection.execute(
                "INSERT INTO settings(key, value) VALUES ('desired_model_digest', ?) "
                "ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                (digest,),
            )
        elif state == "rejected":
            connection.execute(
                "INSERT OR REPLACE INTO models(digest, path, state) VALUES (?, ?, 'rejected')",
                (digest, str(model_path)),
            )
        connection.execute(
            "UPDATE jobs SET state=?, artifact_digest=?, terminal_summary=? WHERE id=?",
            (state, digest, f"model {state}", job_id),
        )
        if state == "rejected":
            payload = {
                "schema_version": 1,
                "event_id": str(job_id),
                "event_type": "candidate_rejection",
                "occurred_at": datetime.now(UTC).isoformat(),
                "job_id": job_id,
                "artifact_digest": digest,
                "summary": f"recipe {job['recipe']} rejected",
            }
            connection.execute(
                "INSERT INTO notifications(event_id, job_id, payload_json, state) "
                "VALUES (?, ?, ?, 'pending')",
                (str(job_id), job_id, json.dumps(payload, sort_keys=True)),
            )
    connection.close()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--storage-root", type=Path, required=True)
    parser.add_argument("--job-id", required=True)
    arguments = parser.parse_args()
    try:
        run(arguments.storage_root, arguments.job_id)
    except Exception as error:
        database = arguments.storage_root / "home.sqlite3"
        connection = sqlite3.connect(database)
        payload = {
            "schema_version": 1,
            "event_id": str(arguments.job_id),
            "event_type": "training_failure",
            "occurred_at": datetime.now(UTC).isoformat(),
            "job_id": arguments.job_id,
            "artifact_digest": None,
            "summary": str(error),
        }
        with connection:
            connection.execute(
                "UPDATE jobs SET state='failed', terminal_summary=? WHERE id=?",
                (str(error), arguments.job_id),
            )
            connection.execute(
                "INSERT OR REPLACE INTO notifications"
                "(event_id, job_id, payload_json, state) VALUES (?, ?, ?, 'pending')",
                (
                    str(arguments.job_id),
                    arguments.job_id,
                    json.dumps(payload, sort_keys=True),
                ),
            )
        connection.close()
        raise


if __name__ == "__main__":
    main()
