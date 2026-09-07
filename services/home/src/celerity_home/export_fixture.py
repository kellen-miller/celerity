"""Deterministically export the tiny real ONNX parity fixture."""

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


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    export(arguments.output)


if __name__ == "__main__":
    main()
