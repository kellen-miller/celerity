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


def whole_run_split(run_digests: list[str]) -> dict[str, str]:
    """Assign every immutable Run wholly to a stable train/validation/test split."""
    split: dict[str, str] = {}
    for digest in sorted(set(run_digests)):
        bucket = int(hashlib.sha256(digest.encode()).hexdigest()[:8], 16) % 100
        split[digest] = "train" if bucket < 70 else "validation" if bucket < 85 else "test"
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

    def __init__(self, recipe: Recipe) -> None:
        super().__init__()
        self.recipe = recipe
        self.input_projection = nn.Conv1d(recipe.signal_count, recipe.channels, 1)
        self.blocks = nn.Sequential(
            *(
                CausalBlock(recipe.channels, recipe.kernel_size, dilation)
                for dilation in recipe.dilations
            )
        )
        self.output_projection = nn.Linear(recipe.channels, recipe.output_count)

    def forward(self, flat_window: torch.Tensor) -> torch.Tensor:
        history = flat_window.reshape(
            flat_window.shape[0], self.recipe.signal_count, self.recipe.history_length
        )
        encoded = self.blocks(self.input_projection(history))
        return self.output_projection(encoded[..., -1])


def export_and_compare(output: Path, recipe: Recipe | None = None) -> dict[str, object]:
    """Export fixed-shape ONNX and compare it to PyTorch with the ONNX reference evaluator."""
    recipe = recipe or Recipe()
    torch.manual_seed(7)
    model = CausalTcn(recipe).eval()
    example = torch.linspace(-1.0, 1.0, recipe.signal_count * recipe.history_length).reshape(1, -1)
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
    if maximum_error > 1e-5:
        raise RuntimeError(f"PyTorch/ONNX parity exceeded tolerance: {maximum_error}")
    return {
        "recipe": recipe.name,
        "input_shape": list(example.shape),
        "output_shape": list(expected.shape),
        "maximum_parity_error": maximum_error,
        "onnx_sha256": hashlib.sha256(output.read_bytes()).hexdigest(),
        "recipe_parameters": json.loads(json.dumps(recipe.__dict__)),
    }
