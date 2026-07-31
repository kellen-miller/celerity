"""Deterministically export the tiny real ONNX parity fixture."""

import hashlib
import json
from argparse import ArgumentParser
from pathlib import Path

import onnx
from onnx import TensorProto, helper


def export(output: Path) -> None:
    input_value = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 3])
    output_value = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 2])
    weights = helper.make_tensor(
        "weights",
        TensorProto.FLOAT,
        [3, 2],
        [1.0, 0.0, 0.0, 1.0, -20.0, 10.0],
    )
    bias = helper.make_tensor("bias", TensorProto.FLOAT, [2], [0.0, 0.0])
    multiply = helper.make_node("MatMul", ["input", "weights"], ["candidate"])
    add = helper.make_node("Add", ["candidate", "bias"], ["output"])
    graph = helper.make_graph(
        [multiply, add],
        "celerity-candidate-thermal",
        [input_value],
        [output_value],
        [weights, bias],
    )
    model = helper.make_model(
        graph,
        producer_name="celerity-fixture",
        opset_imports=[helper.make_opsetid("", 18)],
    )
    model.ir_version = 9
    onnx.checker.check_model(model)
    output.mkdir(parents=True, exist_ok=True)
    model_path = output / "identity.onnx"
    onnx.save(model, model_path)
    manifest = {
        "schema_version": 1,
        "onnx_sha256": hashlib.sha256(model_path.read_bytes()).hexdigest(),
        "signal_order": ["coolant_temperature_c", "air_temperature_c"],
        "units": ["degC", "degC"],
        "sample_period_ms": 20,
        "history_length": 1,
        "horizons": [1],
        "output_order": ["coolant", "post_intercooler_iat"],
        "command_lattice": [1000, 5000, 9000],
        "compatibility": {"model_abi": "thermal-v1", "input_shape": [1, 3]},
        "input_ranges": [
            {"minimum": 60.0, "maximum": 120.0},
            {"minimum": 0.0, "maximum": 100.0},
        ],
        "normalization": [
            {"mean": 90.0, "scale": 10.0},
            {"mean": 40.0, "scale": 10.0},
        ],
        "calibration_error": 0.01,
    }
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    export(arguments.output)


if __name__ == "__main__":
    main()
