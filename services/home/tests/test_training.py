from pathlib import Path

import numpy as np
import onnx
import pyarrow.parquet as pq
from onnx.reference import ReferenceEvaluator

from celerity_home.training import (
    MAXIMUM_PARITY_ERROR,
    Recipe,
    derive_run_index,
    export_and_compare,
    whole_run_split,
)


def test_whole_run_split_is_stable_and_never_splits_one_run(tmp_path: Path) -> None:
    digests = [f"{index:064x}" for index in range(40)]
    first = whole_run_split(digests)
    assert first == whole_run_split(list(reversed(digests)))
    assert set(first.values()) == {"train", "validation", "test"}
    derivation = tmp_path / "derived.parquet"
    derivation_digest = derive_run_index(digests, derivation)
    assert len(derivation_digest) == 64
    table = pq.read_table(derivation)
    assert table.num_rows == len(digests)
    assert len(set(table.column("run_digest").to_pylist())) == len(digests)


def test_causal_tcn_exports_fixed_shape_with_reference_parity(tmp_path: Path) -> None:
    output = tmp_path / "candidate.onnx"
    evaluation = export_and_compare(
        output,
        Recipe(),
        signal_means=np.asarray([90.0, 40.0], dtype=np.float32),
        signal_scales=np.asarray([10.0, 5.0], dtype=np.float32),
    )
    assert evaluation["input_shape"] == [1, 9]
    assert evaluation["output_shape"] == [1, 2]
    assert evaluation["maximum_parity_error"] <= MAXIMUM_PARITY_ERROR
    assert len(evaluation["onnx_sha256"]) == 64
    assert evaluation["normalization"] == [
        {"mean": 90.0, "scale": 10.0},
        {"mean": 40.0, "scale": 5.0},
    ]
    operation_types = {node.op_type for node in onnx.load(output).graph.node}
    assert {"Sub", "Div"}.issubset(operation_types)


def test_changed_run_observations_change_fitted_graph_predictions(tmp_path: Path) -> None:
    recipe = Recipe(history_length=1)
    inputs = np.asarray([[90.0, 40.0, 0.1], [95.0, 45.0, 0.9]], dtype=np.float32)
    held_inputs = np.asarray([[92.0, 42.0, 0.5]], dtype=np.float32)
    first_path = tmp_path / "first.onnx"
    second_path = tmp_path / "second.onnx"
    first = export_and_compare(
        first_path,
        recipe,
        inputs,
        np.asarray([[89.0, 39.0], [90.0, 40.0]], dtype=np.float32),
        held_inputs,
        np.asarray([[91.0, 41.0]], dtype=np.float32),
    )
    second = export_and_compare(
        second_path,
        recipe,
        inputs,
        np.asarray([[79.0, 34.0], [80.0, 35.0]], dtype=np.float32),
        held_inputs,
        np.asarray([[81.0, 36.0]], dtype=np.float32),
    )
    assert first["onnx_sha256"] != second["onnx_sha256"]
    first_prediction = ReferenceEvaluator(onnx.load(first_path)).run(
        None, {"thermal_history": held_inputs}
    )[0]
    second_prediction = ReferenceEvaluator(onnx.load(second_path)).run(
        None, {"thermal_history": held_inputs}
    )[0]
    assert not np.allclose(first_prediction, second_prediction)
