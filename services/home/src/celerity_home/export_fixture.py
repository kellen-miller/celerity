"""Deterministically export the tiny real ONNX parity fixture."""

from argparse import ArgumentParser
from pathlib import Path

import onnx
from onnx import TensorProto, helper


def export(output: Path) -> None:
    input_value = helper.make_tensor_value_info("input", TensorProto.FLOAT, [1, 2])
    output_value = helper.make_tensor_value_info("output", TensorProto.FLOAT, [1, 2])
    node = helper.make_node("Identity", ["input"], ["output"])
    graph = helper.make_graph([node], "celerity-identity", [input_value], [output_value])
    model = helper.make_model(
        graph,
        producer_name="celerity-fixture",
        opset_imports=[helper.make_opsetid("", 18)],
    )
    model.ir_version = 9
    onnx.checker.check_model(model)
    output.mkdir(parents=True, exist_ok=True)
    onnx.save(model, output / "identity.onnx")


def main() -> None:
    parser = ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    arguments = parser.parse_args()
    export(arguments.output)


if __name__ == "__main__":
    main()
