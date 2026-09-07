"""Managed subprocess for one durable home job."""

from __future__ import annotations

import argparse
import hashlib
import json
import zipfile
from pathlib import Path
from typing import Any, cast

import numpy as np
import onnx
from onnx.reference import ReferenceEvaluator

from celerity.v1.celerity_pb2 import (
    WEBHOOK_EVENT_TYPE_CANDIDATE_REJECTION,
    WEBHOOK_EVENT_TYPE_TRAINING_FAILURE,
    ModelBundleManifest,
    ModelCompatibility,
    ModelInputRange,
    ModelNormalization,
    RunManifest,
)
from celerity_home.database import connect as _connect
from celerity_home.notifications import webhook_event as _webhook_event
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
    connection = _connect(database)
    job = connection.execute("SELECT * FROM jobs WHERE id=?", (job_id,)).fetchone()
    if job is None:
        raise RuntimeError(f"unknown job {job_id}")
    output = storage_root / "jobs" / job_id
    output.mkdir(parents=True, exist_ok=True)
    run_digests = json.loads(job["input_digests_json"])
    manifests = [
        RunManifest.FromString(
            connection.execute("SELECT manifest_pb FROM runs WHERE digest=?", (digest,)).fetchone()[
                "manifest_pb"
            ]
        )
        for digest in run_digests
    ]
    contract: dict[str, Any] = {
        "model_abi": manifests[0].model_abi,
        "model_input_signals": list(manifests[0].model_input_signals),
        "model_history_length": manifests[0].model_history_length,
        "sample_period_ms": manifests[0].sample_period_ms,
        "command_lattice": list(manifests[0].command_lattice),
        "maximum_calibration_error": manifests[0].maximum_calibration_error,
    }
    if any(not contract[field] for field in contract) or any(
        (
            list(manifest.model_input_signals) != contract["model_input_signals"]
            or list(manifest.command_lattice) != contract["command_lattice"]
            or manifest.model_abi != contract["model_abi"]
            or manifest.model_history_length != contract["model_history_length"]
            or manifest.sample_period_ms != contract["sample_period_ms"]
            or manifest.maximum_calibration_error != contract["maximum_calibration_error"]
        )
        for manifest in manifests
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
    evaluation: dict[str, Any] = export_and_compare(
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
    manifest = ModelBundleManifest(
        schema_version=1,
        onnx_sha256=hashlib.sha256(model_bytes).hexdigest(),
        signal_order=contract["model_input_signals"],
        units=["native"] * len(contract["model_input_signals"]),
        sample_period_ms=contract["sample_period_ms"],
        history_length=contract["model_history_length"],
        horizons=[1],
        output_order=["coolant", "post_intercooler_iat"],
        command_lattice=contract["command_lattice"],
        compatibility=ModelCompatibility(
            model_abi=contract["model_abi"],
            input_shape=evaluation["input_shape"],
            derivation_digest=derivation_digest,
            input_runs=run_digests,
            maximum_parity_error=evaluation["maximum_parity_error"],
            held_out_mse=evaluation["held_out_mse"],
        ),
        input_ranges=[
            ModelInputRange(minimum=min(values), maximum=max(values))
            for values in zip(*observed_values, strict=True)
        ],
        normalization=[
            ModelNormalization(mean=value["mean"], scale=value["scale"])
            for value in evaluation["normalization"]
        ],
        calibration_error=float(evaluation["held_out_mse"]) ** 0.5,
    )
    manifest_bytes = manifest.SerializeToString()
    bundle_path = output / "candidate.zip"
    with zipfile.ZipFile(bundle_path, "w", compression=zipfile.ZIP_STORED) as archive:
        for name, data in (("manifest.pb", manifest_bytes), ("model.onnx", model_bytes)):
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
        if baseline is None:
            raise RuntimeError("desired baseline model is absent or not staged")
        with zipfile.ZipFile(baseline["path"]) as archive:
            baseline_manifest = ModelBundleManifest.FromString(archive.read("manifest.pb"))
            baseline_model_bytes = archive.read("model.onnx")
        baseline_contract_matches = (
            list(baseline_manifest.signal_order) == contract["model_input_signals"]
            and baseline_manifest.sample_period_ms == contract["sample_period_ms"]
            and baseline_manifest.history_length == contract["model_history_length"]
            and list(baseline_manifest.command_lattice) == contract["command_lattice"]
            and baseline_manifest.compatibility.model_abi == contract["model_abi"]
        )
        if not baseline_contract_matches:
            raise RuntimeError("desired baseline model contract is incompatible")
        evaluator = ReferenceEvaluator(onnx.load_from_string(baseline_model_bytes))
        prediction = np.concatenate(
            [
                cast(
                    list[np.ndarray],
                    evaluator.run(None, {"thermal_history": row.reshape(1, -1)}),
                )[0]
                for row in held_inputs
            ]
        )
        baseline_mse = float(np.mean((prediction - held_targets) ** 2))
        if not np.isfinite(baseline_mse):
            raise RuntimeError("desired baseline evaluation is not finite")
    candidate_mse = float(evaluation["held_out_mse"])
    calibration_error = candidate_mse**0.5
    materially_improves = baseline_mse is None or candidate_mse < baseline_mse - max(
        1e-9, abs(baseline_mse) * 1e-6
    )
    state = classify_evaluation(
        float(evaluation["maximum_parity_error"]),
        existing is not None,
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
            payload = _webhook_event(
                str(job_id),
                WEBHOOK_EVENT_TYPE_CANDIDATE_REJECTION,
                f"recipe {job['recipe']} rejected",
                job_id=job_id,
                artifact_digest=digest,
            )
            connection.execute(
                "INSERT OR IGNORE INTO notifications(event_id, job_id, payload_pb, state) "
                "VALUES (?, ?, ?, 'pending')",
                (str(job_id), job_id, payload),
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
        connection = _connect(database)
        payload = _webhook_event(
            str(arguments.job_id),
            WEBHOOK_EVENT_TYPE_TRAINING_FAILURE,
            str(error),
            job_id=arguments.job_id,
        )
        with connection:
            connection.execute(
                "UPDATE jobs SET state='failed', terminal_summary=? WHERE id=?",
                (str(error), arguments.job_id),
            )
            connection.execute(
                "INSERT OR REPLACE INTO notifications"
                "(event_id, job_id, payload_pb, state) VALUES (?, ?, ?, 'pending')",
                (
                    str(arguments.job_id),
                    arguments.job_id,
                    payload,
                ),
            )
        connection.close()
        raise


if __name__ == "__main__":
    main()
