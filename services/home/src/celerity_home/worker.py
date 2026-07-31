"""Managed subprocess for one durable home job."""

from __future__ import annotations

import argparse
import hashlib
import json
import sqlite3
import zipfile
from datetime import UTC, datetime
from pathlib import Path

import numpy as np
import onnx
from onnx.reference import ReferenceEvaluator

from celerity_home.training import (
    MAXIMUM_PARITY_ERROR,
    NoEligibleTrainingData,
    Recipe,
    derive_run_index,
    export_and_compare,
    load_training_examples,
)


def classify_evaluation(
    maximum_parity_error: float,
    already_staged: bool,
    improves_baseline: bool = True,
    calibration_error: float = 0.0,
    maximum_calibration_error: float = float("inf"),
) -> str:
    """Classify a bounded recipe result before changing desired model state."""
    if (
        not 0 <= maximum_parity_error <= MAXIMUM_PARITY_ERROR
        or not 0 <= calibration_error <= maximum_calibration_error
    ):
        return "rejected"
    if already_staged or not improves_baseline:
        return "no_change"
    return "completed"


def run(storage_root: Path, job_id: str) -> None:
    """Train and package one immutable, self-describing model bundle."""
    database = storage_root / "home.sqlite3"
    connection = sqlite3.connect(database)
    connection.row_factory = sqlite3.Row
    job = connection.execute("SELECT * FROM jobs WHERE id=?", (job_id,)).fetchone()
    if job is None:
        raise RuntimeError(f"unknown job {job_id}")
    output = storage_root / "jobs" / job_id
    output.mkdir(parents=True, exist_ok=True)
    run_digests = json.loads(job["input_digests_json"])
    manifests = [
        json.loads(
            connection.execute(
                "SELECT manifest_json FROM runs WHERE digest=?", (digest,)
            ).fetchone()["manifest_json"]
        )
        for digest in run_digests
    ]
    contract_fields = (
        "model_abi",
        "model_input_signals",
        "model_history_length",
        "sample_period_ms",
        "command_lattice",
        "maximum_calibration_error",
    )
    contract = {field: manifests[0].get(field) for field in contract_fields}
    if any(not contract[field] for field in contract_fields) or any(
        manifest.get(field) != contract[field]
        for manifest in manifests
        for field in contract_fields
    ):
        raise RuntimeError("Run model contracts are absent or incompatible")
    derivation_digest = derive_run_index(run_digests, output / "derived.parquet")
    model_path = output / "candidate.onnx"
    recipe = Recipe(
        signal_count=len(contract["model_input_signals"]),
        history_length=contract["model_history_length"],
    )
    try:
        training_inputs, training_targets, held_inputs, held_targets, observed_values = (
            load_training_examples(storage_root, run_digests, manifests, recipe)
        )
    except NoEligibleTrainingData as error:
        with connection:
            connection.execute(
                "UPDATE jobs SET state='no_change', terminal_summary=? WHERE id=?",
                (str(error), job_id),
            )
        connection.close()
        return
    signal_means = np.asarray(
        [float(np.mean(values)) for values in zip(*observed_values, strict=True)],
        dtype=np.float32,
    )
    signal_scales = np.asarray(
        [max(float(np.std(values)), 1e-6) for values in zip(*observed_values, strict=True)],
        dtype=np.float32,
    )
    evaluation = export_and_compare(
        model_path,
        recipe,
        training_inputs,
        training_targets,
        held_inputs,
        held_targets,
        signal_means,
        signal_scales,
    )
    model_bytes = model_path.read_bytes()
    manifest = {
        "schema_version": 1,
        "onnx_sha256": hashlib.sha256(model_bytes).hexdigest(),
        "signal_order": contract["model_input_signals"],
        "units": ["native"] * len(contract["model_input_signals"]),
        "sample_period_ms": contract["sample_period_ms"],
        "history_length": contract["model_history_length"],
        "horizons": [1],
        "output_order": ["coolant", "post_intercooler_iat"],
        "command_lattice": contract["command_lattice"],
        "compatibility": {
            "model_abi": contract["model_abi"],
            "input_shape": evaluation["input_shape"],
            "derivation_digest": derivation_digest,
            "input_runs": run_digests,
            "maximum_parity_error": evaluation["maximum_parity_error"],
            "held_out_mse": evaluation["held_out_mse"],
        },
        "input_ranges": [
            {"minimum": min(values), "maximum": max(values)}
            for values in zip(*observed_values, strict=True)
        ],
        "normalization": evaluation["normalization"],
        "calibration_error": float(evaluation["held_out_mse"]) ** 0.5,
    }
    manifest_bytes = json.dumps(manifest, indent=2, sort_keys=True).encode() + b"\n"
    bundle_path = output / "candidate.zip"
    with zipfile.ZipFile(bundle_path, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, data in (("manifest.json", manifest_bytes), ("model.onnx", model_bytes)):
            entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            entry.compress_type = zipfile.ZIP_STORED
            entry.external_attr = 0o100644 << 16
            archive.writestr(entry, data)
    bundle_bytes = bundle_path.read_bytes()
    digest = hashlib.sha256(bundle_bytes).hexdigest()
    model_path = bundle_path.replace(output / f"{digest}.zip")
    (output / "evaluation.json").write_text(
        json.dumps(
            {**evaluation, "derivation_digest": derivation_digest, "input_runs": run_digests},
            indent=2,
            sort_keys=True,
        )
    )
    existing = connection.execute("SELECT state FROM models WHERE digest=?", (digest,)).fetchone()
    desired = connection.execute(
        "SELECT value FROM settings WHERE key='desired_model_digest'"
    ).fetchone()
    baseline_mse = None
    if desired is not None:
        baseline = connection.execute(
            "SELECT path FROM models WHERE digest=? AND state='staged'", (desired["value"],)
        ).fetchone()
        if baseline is not None:
            try:
                with zipfile.ZipFile(baseline["path"]) as archive:
                    baseline_manifest = json.loads(archive.read("manifest.json"))
                    baseline_model_bytes = archive.read("model.onnx")
                baseline_contract_matches = (
                    baseline_manifest.get("signal_order") == contract["model_input_signals"]
                    and baseline_manifest.get("sample_period_ms") == contract["sample_period_ms"]
                    and baseline_manifest.get("history_length") == contract["model_history_length"]
                    and baseline_manifest.get("command_lattice") == contract["command_lattice"]
                    and baseline_manifest.get("compatibility", {}).get("model_abi")
                    == contract["model_abi"]
                )
                if baseline_contract_matches:
                    evaluator = ReferenceEvaluator(onnx.load_from_string(baseline_model_bytes))
                    prediction = np.concatenate(
                        [
                            evaluator.run(None, {"thermal_history": row.reshape(1, -1)})[0]
                            for row in held_inputs
                        ]
                    )
                    baseline_mse = float(np.mean((prediction - held_targets) ** 2))
            except (OSError, ValueError, zipfile.BadZipFile, KeyError):
                baseline_mse = None
    candidate_mse = float(evaluation["held_out_mse"])
    calibration_error = candidate_mse**0.5
    materially_improves = baseline_mse is None or candidate_mse < baseline_mse - max(
        1e-9, abs(baseline_mse) * 1e-6
    )
    state = classify_evaluation(
        float(evaluation["maximum_parity_error"]),
        existing is not None and existing["state"] == "staged",
        materially_improves,
        calibration_error,
        float(contract["maximum_calibration_error"]),
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
