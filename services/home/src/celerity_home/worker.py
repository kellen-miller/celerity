"""Numerical training adapter; Rust owns job, model and notification state."""

from __future__ import annotations

import argparse
import hashlib
import json
import zipfile
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any, cast

import numpy as np
import onnx
from onnx.reference import ReferenceEvaluator

from celerity.v1.celerity_pb2 import (
    ModelBundleManifest,
    ModelCompatibility,
    ModelInputRange,
    ModelNormalization,
    RunManifest,
)
from celerity_home.training import (
    NoEligibleTrainingData,
    Recipe,
    derive_run_index,
    export_and_compare,
    load_training_examples,
)


@dataclass(frozen=True)
class TrainingRequest:
    schema_version: int
    storage_root: Path
    run_digests: list[str]
    manifests_hex: list[str]
    baseline_path: str | None


@dataclass(frozen=True)
class TrainingResult:
    schema_version: int
    outcome: str
    summary: str
    bundle_digest: str | None = None
    maximum_parity_error: float | None = None
    held_out_mse: float | None = None
    baseline_mse: float | None = None


def write_result(output: Path, result: TrainingResult) -> None:
    temporary = output / "result.json.staging"
    temporary.write_text(json.dumps(asdict(result), allow_nan=False, sort_keys=True))
    temporary.replace(output / "result.json")


def run(request_path: Path) -> None:
    """Train/evaluate immutable inputs and atomically publish a candidate result."""
    payload = json.loads(request_path.read_text())
    request = TrainingRequest(
        schema_version=payload["schema_version"],
        storage_root=Path(payload["storage_root"]),
        run_digests=payload["run_digests"],
        manifests_hex=payload["manifests_hex"],
        baseline_path=payload.get("baseline_path"),
    )
    if request.schema_version != 1:
        raise ValueError("unsupported training request schema_version")
    run_digests = request.run_digests
    manifests = [RunManifest.FromString(bytes.fromhex(value)) for value in request.manifests_hex]
    if not manifests or len(manifests) != len(run_digests):
        raise ValueError("training request requires one manifest per Run")
    storage_root = request.storage_root
    output = request_path.parent
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
        write_result(output, TrainingResult(1, "no_change", str(error)))
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
    bundle_path.replace(output / f"{digest}.zip")
    (output / "evaluation.json").write_text(
        json.dumps(
            {**evaluation, "derivation_digest": derivation_digest, "input_runs": run_digests},
            indent=2,
            sort_keys=True,
        )
    )
    baseline_mse = None
    if request.baseline_path is not None:
        with zipfile.ZipFile(request.baseline_path) as archive:
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
    write_result(
        output,
        TrainingResult(
            schema_version=1,
            outcome="candidate",
            summary="candidate evaluated",
            bundle_digest=digest,
            maximum_parity_error=float(evaluation["maximum_parity_error"]),
            held_out_mse=float(evaluation["held_out_mse"]),
            baseline_mse=baseline_mse,
        ),
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--request", type=Path, required=True)
    arguments = parser.parse_args()
    run(arguments.request)


if __name__ == "__main__":
    main()
