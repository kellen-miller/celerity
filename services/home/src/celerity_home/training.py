"""Bounded v1 whole-Run derivation and causal TCN recipe."""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from pathlib import Path

import numpy as np
import onnx
import pyarrow as pa
import pyarrow.parquet as pq
import torch
from onnx.reference import ReferenceEvaluator
from torch import nn

from celerity_home.run import decode_run_records

# Both model outputs are temperatures. This remains far below source sensor resolution while
# allowing harmless floating-point differences between supported ONNX/PyTorch platforms.
MAXIMUM_PARITY_ERROR = 1e-4


@dataclass(frozen=True)
class Recipe:
    """The bounded, versioned home recipe recorded with each artifact."""

    name: str = "causal-tcn-v1"
    signal_count: int = 2
    history_length: int = 4
    channels: int = 4
    kernel_size: int = 3
    dilations: tuple[int, ...] = (1, 2)
    output_count: int = 2


class NoEligibleTrainingData(RuntimeError):
    """The admitted corpus is valid but cannot produce isolated examples."""


def whole_run_split(run_digests: list[str]) -> dict[str, str]:
    """Assign every immutable Run wholly to a stable train/validation/test split."""
    split: dict[str, str] = {}
    for digest in sorted(set(run_digests)):
        bucket = int(hashlib.sha256(digest.encode()).hexdigest()[:8], 16) % 100
        split[digest] = "train" if bucket < 70 else "validation" if bucket < 85 else "test"
    if len(split) >= 2 and (
        not any(value == "train" for value in split.values())
        or not any(value != "train" for value in split.values())
    ):
        ordered = sorted(split)
        split = {digest: "train" for digest in ordered}
        split[ordered[-1]] = "validation"
    return split


def derive_run_index(run_digests: list[str], output: Path) -> str:
    """Write the v1 whole-Run split index to Parquet with a derivation digest."""
    assignments = whole_run_split(run_digests)
    table = pa.table(
        {
            "run_digest": list(assignments),
            "split": list(assignments.values()),
            "derivation_spec": ["run-index-v1"] * len(assignments),
        }
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    pq.write_table(table, output, compression="zstd")
    return hashlib.sha256(output.read_bytes()).hexdigest()


def load_training_examples(
    storage_root: Path,
    run_digests: list[str],
    manifests: list[dict[str, object]],
    recipe: Recipe,
) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, list[list[float]]]:
    """Decode canonical Run protobuf records into causal history/candidate examples."""
    if len(run_digests) < 2:
        raise NoEligibleTrainingData("training requires at least two whole Runs")
    assignments = whole_run_split(run_digests)
    train_inputs: list[list[float]] = []
    train_targets: list[list[float]] = []
    held_inputs: list[list[float]] = []
    held_targets: list[list[float]] = []
    observed_values: list[list[float]] = []
    signals = manifests[0]["model_input_signals"]
    coolant_index = signals.index("coolant_temperature_c")
    iat_index = signals.index("air_temperature_c")
    for run_digest, manifest in zip(run_digests, manifests, strict=True):
        chunks = manifest.get("chunks")
        if not isinstance(chunks, list) or not chunks:
            chunks = [{"sha256": manifest["chunk_sha256"]}]
        records = []
        for chunk in chunks:
            chunk_digest = str(chunk["sha256"])
            records.extend(
                decode_run_records((storage_root / "runs" / run_digest / chunk_digest).read_bytes())
            )
        snapshots: list[list[float]] = []
        accepted_commands: list[tuple[int, float]] = []
        pending_snapshot_index: int | None = None
        signal_group: dict[str, float] = {}
        for record in records:
            if record.kind == "signal_observation":
                observation = json.loads(record.payload)
                signal_group[observation["signal"]] = float(observation["value"])
                if all(signal in signal_group for signal in signals):
                    values = [signal_group.pop(signal) for signal in signals]
                    if assignments[run_digest] == "train":
                        observed_values.append(values)
                    snapshots.append(values)
                    pending_snapshot_index = len(snapshots) - 1
            elif record.kind == "control_decision" and pending_snapshot_index is not None:
                command = json.loads(record.payload)
                if command.get("state") == "accepted":
                    accepted_commands.append(
                        (
                            pending_snapshot_index,
                            float(command["radiator_split_basis_points"]) / 10_000.0,
                        )
                    )
                    pending_snapshot_index = None
        for index, command in accepted_commands:
            if index < recipe.history_length - 1 or index + 1 >= len(snapshots):
                continue
            history = snapshots[index + 1 - recipe.history_length : index + 1]
            flattened = [
                history[offset][signal]
                for signal in range(recipe.signal_count)
                for offset in range(recipe.history_length)
            ]
            inputs = train_inputs if assignments[run_digest] == "train" else held_inputs
            targets = train_targets if assignments[run_digest] == "train" else held_targets
            inputs.append([*flattened, command])
            next_snapshot = snapshots[index + 1]
            targets.append([next_snapshot[coolant_index], next_snapshot[iat_index]])
    if not train_inputs or not held_inputs:
        raise NoEligibleTrainingData("Runs contain no isolated train and held-out examples")
    return (
        np.asarray(train_inputs, dtype=np.float32),
        np.asarray(train_targets, dtype=np.float32),
        np.asarray(held_inputs, dtype=np.float32),
        np.asarray(held_targets, dtype=np.float32),
        observed_values,
    )


class CausalBlock(nn.Module):
    """Residual Conv1d block whose crop removes all future padding."""

    def __init__(self, channels: int, kernel_size: int, dilation: int) -> None:
        super().__init__()
        self.future_padding = (kernel_size - 1) * dilation
        self.convolution = nn.Conv1d(
            channels,
            channels,
            kernel_size,
            padding=self.future_padding,
            dilation=dilation,
        )
        self.activation = nn.ReLU()

    def forward(self, values: torch.Tensor) -> torch.Tensor:
        convolved = self.convolution(values)
        if self.future_padding:
            convolved = convolved[..., : -self.future_padding]
        return self.activation(convolved) + values


class CausalTcn(nn.Module):
    """Small fixed-shape causal multi-output thermal network."""

    def __init__(
        self,
        recipe: Recipe,
        signal_means: np.ndarray | None = None,
        signal_scales: np.ndarray | None = None,
    ) -> None:
        super().__init__()
        self.recipe = recipe
        means = (
            np.zeros(recipe.signal_count, dtype=np.float32)
            if signal_means is None
            else signal_means.astype(np.float32)
        )
        scales = (
            np.ones(recipe.signal_count, dtype=np.float32)
            if signal_scales is None
            else signal_scales.astype(np.float32)
        )
        if means.shape != (recipe.signal_count,) or scales.shape != (recipe.signal_count,):
            raise ValueError("normalization shape must match signal_count")
        if not np.isfinite(means).all() or not np.isfinite(scales).all() or (scales <= 0).any():
            raise ValueError("normalization values must be finite with positive scales")
        self.register_buffer("signal_means", torch.from_numpy(means).reshape(1, -1, 1))
        self.register_buffer("signal_scales", torch.from_numpy(scales).reshape(1, -1, 1))
        self.input_projection = nn.Conv1d(recipe.signal_count, recipe.channels, 1)
        self.blocks = nn.Sequential(
            *(
                CausalBlock(recipe.channels, recipe.kernel_size, dilation)
                for dilation in recipe.dilations
            )
        )
        self.output_projection = nn.Sequential(
            nn.Linear(recipe.channels + 1, recipe.channels),
            nn.ReLU(),
            nn.Linear(recipe.channels, recipe.output_count),
        )

    def forward(self, flat_window: torch.Tensor) -> torch.Tensor:
        history = flat_window[:, :-1].reshape(
            flat_window.shape[0], self.recipe.signal_count, self.recipe.history_length
        )
        history = (history - self.signal_means) / self.signal_scales
        encoded = self.blocks(self.input_projection(history))
        state_and_candidate = torch.cat((encoded[..., -1], flat_window[:, -1:]), dim=1)
        return self.output_projection(state_and_candidate)


def export_and_compare(
    output: Path,
    recipe: Recipe | None = None,
    training_inputs: np.ndarray | None = None,
    training_targets: np.ndarray | None = None,
    evaluation_inputs: np.ndarray | None = None,
    evaluation_targets: np.ndarray | None = None,
    signal_means: np.ndarray | None = None,
    signal_scales: np.ndarray | None = None,
) -> dict[str, object]:
    """Export fixed-shape ONNX and compare it to PyTorch with the ONNX reference evaluator."""
    recipe = recipe or Recipe()
    torch.manual_seed(7)
    model = CausalTcn(recipe, signal_means, signal_scales).eval()
    held_out_mse = None
    if training_inputs is not None and training_targets is not None:
        inputs = torch.from_numpy(training_inputs.astype(np.float32))
        targets = torch.from_numpy(training_targets.astype(np.float32))
        model.train()
        optimizer = torch.optim.Adam(model.parameters(), lr=0.01)
        for _ in range(250):
            optimizer.zero_grad()
            loss = nn.functional.mse_loss(model(inputs), targets)
            loss.backward()
            optimizer.step()
        model.eval()
        if evaluation_inputs is None or evaluation_targets is None:
            raise RuntimeError("fitted export requires isolated evaluation examples")
        evaluation_values = torch.from_numpy(evaluation_inputs.astype(np.float32))
        evaluation_expected = torch.from_numpy(evaluation_targets.astype(np.float32))
        held_out_mse = float(
            nn.functional.mse_loss(model(evaluation_values), evaluation_expected).detach()
        )
    example = torch.linspace(-1.0, 1.0, recipe.signal_count * recipe.history_length + 1).reshape(
        1, -1
    )
    output.parent.mkdir(parents=True, exist_ok=True)
    torch.onnx.export(
        model,
        (example,),
        output,
        input_names=["thermal_history"],
        output_names=["thermal_prediction"],
        opset_version=18,
    )
    onnx_model = onnx.load(output)
    onnx.checker.check_model(onnx_model)
    expected = model(example).detach().numpy()
    actual = ReferenceEvaluator(onnx_model).run(None, {"thermal_history": example.numpy()})[0]
    maximum_error = float(np.max(np.abs(expected - actual)))
    if maximum_error > MAXIMUM_PARITY_ERROR:
        raise RuntimeError(f"PyTorch/ONNX parity exceeded tolerance: {maximum_error}")
    return {
        "recipe": recipe.name,
        "input_shape": list(example.shape),
        "output_shape": list(expected.shape),
        "maximum_parity_error": maximum_error,
        "held_out_mse": held_out_mse,
        "normalization": [
            {"mean": float(mean), "scale": float(scale)}
            for mean, scale in zip(
                model.signal_means.detach().numpy().reshape(-1),
                model.signal_scales.detach().numpy().reshape(-1),
                strict=True,
            )
        ],
        "onnx_sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
        "recipe_parameters": json.loads(json.dumps(recipe.__dict__)),
    }
