from pathlib import Path

import pyarrow.parquet as pq

from celerity_home.training import Recipe, derive_run_index, export_and_compare, whole_run_split


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
    evaluation = export_and_compare(tmp_path / "candidate.onnx", Recipe())
    assert evaluation["input_shape"] == [1, 8]
    assert evaluation["output_shape"] == [1, 2]
    assert evaluation["maximum_parity_error"] <= 1e-5
    assert len(evaluation["onnx_sha256"]) == 64
