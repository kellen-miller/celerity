from __future__ import annotations

import hashlib
import json
import zipfile
from dataclasses import dataclass
from pathlib import Path

import pytest
from celerity.v1.celerity_pb2 import (
    COMPLETION_COMPLETE,
    TRAINING_OUTCOME_CANDIDATE,
    TRAINING_OUTCOME_NO_CHANGE,
    ControlDecision,
    ModelBundleManifest,
    RunEvent,
    RunManifest,
    SignalObservation,
    TrainingRequest,
    TrainingResult,
)

from celerity_home.training import MAXIMUM_PARITY_ERROR, Recipe, load_training_examples
from celerity_home.worker import run, write_result


@dataclass(frozen=True)
class TrainingCorpus:
    root: Path
    digests: list[str]
    manifests: list[RunManifest]


@pytest.fixture
def training_corpus(tmp_path: Path) -> TrainingCorpus:
    digests = []
    manifests = []
    for run_index in range(2):
        records = []
        sequence = 0
        for index in range(7):
            for signal, value in (
                ("coolant_temperature_c", 90.0 + run_index - index),
                ("air_temperature_c", 45.0 + run_index / 2 - index / 2),
            ):
                sequence += 1
                records.append(
                    RunEvent(
                        schema_major=1,
                        sequence=sequence,
                        monotonic_ns=sequence * 1_000_000,
                        source="test",
                        signal_observation=SignalObservation(signal=signal, value=value),
                    )
                )
            if index < 6:
                for state, command in (
                    ("requested", 2000),
                    ("accepted", [1000, 5000, 9000][index % 3]),
                ):
                    sequence += 1
                    records.append(
                        RunEvent(
                            schema_major=1,
                            sequence=sequence,
                            monotonic_ns=sequence * 1_000_000,
                            source="test",
                            control_decision=ControlDecision(
                                encoded=json.dumps(
                                    {"state": state, "radiator_split_basis_points": command}
                                )
                            ),
                        )
                    )
        chunk = bytearray()
        for record in records:
            encoded = record.SerializeToString()
            length = len(encoded)
            while length >= 0x80:
                chunk.append((length & 0x7F) | 0x80)
                length >>= 7
            chunk.append(length)
            chunk.extend(encoded)
        chunk_digest = hashlib.sha256(chunk).hexdigest()
        manifest = RunManifest(
            schema_version=1,
            run_id=f"training-{run_index}",
            completion=COMPLETION_COMPLETE,
            chunk_sha256=chunk_digest,
            model_abi="thermal-v1",
            model_input_signals=["coolant_temperature_c", "air_temperature_c"],
            model_history_length=4,
            sample_period_ms=20,
            command_lattice=[1000, 5000, 9000],
            maximum_calibration_error=100.0,
        )
        digest = hashlib.sha256(manifest.SerializeToString()).hexdigest()
        directory = tmp_path / "runs" / digest
        directory.mkdir(parents=True)
        (directory / chunk_digest).write_bytes(chunk)
        digests.append(digest)
        manifests.append(manifest)
    return TrainingCorpus(tmp_path, digests, manifests)


def write_request(corpus: TrainingCorpus, name: str, baseline_path: Path | None = None) -> Path:
    directory = corpus.root / "jobs" / name
    directory.mkdir(parents=True)
    request = directory / "request.pb"
    message = TrainingRequest(
        schema_version=1,
        storage_root=str(corpus.root),
        run_digests=corpus.digests,
        manifests=corpus.manifests,
    )
    if baseline_path is not None:
        message.baseline_path = str(baseline_path)
    request.write_bytes(message.SerializeToString())
    return request


def test_training_adapter_publishes_candidate_without_database_access(
    training_corpus: TrainingCorpus,
) -> None:
    request = write_request(training_corpus, "candidate")
    original = request.read_bytes()
    run(request)
    assert request.read_bytes() == original
    assert not (request.parent / "result.pb.staging").exists()
    result = TrainingResult.FromString((request.parent / "result.pb").read_bytes())
    assert result.schema_version == 1
    assert result.outcome == TRAINING_OUTCOME_CANDIDATE
    assert result.maximum_parity_error <= MAXIMUM_PARITY_ERROR
    assert result.held_out_mse >= 0
    assert not result.HasField("baseline_mse")
    archive_path = request.parent / f"{result.bundle_digest}.zip"
    assert hashlib.sha256(archive_path.read_bytes()).hexdigest() == result.bundle_digest
    with zipfile.ZipFile(archive_path) as archive:
        manifest = ModelBundleManifest.FromString(archive.read("manifest.pb"))
        assert hashlib.sha256(archive.read("model.onnx")).hexdigest() == manifest.onnx_sha256
        assert list(manifest.compatibility.input_runs) == training_corpus.digests
    assert not (training_corpus.root / "home.sqlite3").exists()

    repeated = write_request(training_corpus, "baseline", archive_path)
    run(repeated)
    evaluated = TrainingResult.FromString((repeated.parent / "result.pb").read_bytes())
    assert evaluated.HasField("baseline_mse")
    assert evaluated.baseline_mse == pytest.approx(evaluated.held_out_mse, abs=MAXIMUM_PARITY_ERROR)
    assert not (training_corpus.root / "home.sqlite3").exists()


def test_training_adapter_reports_ineligible_corpus_without_artifact(
    training_corpus: TrainingCorpus,
) -> None:
    single = TrainingCorpus(
        training_corpus.root, training_corpus.digests[:1], training_corpus.manifests[:1]
    )
    request = write_request(single, "single")
    run(request)
    result = TrainingResult.FromString((request.parent / "result.pb").read_bytes())
    assert result.outcome == TRAINING_OUTCOME_NO_CHANGE
    assert not result.HasField("bundle_digest")
    assert "two whole Runs" in result.summary
    assert not list(request.parent.glob("*.zip"))
    assert not (single.root / "home.sqlite3").exists()


def test_training_adapter_rejects_corrupt_baseline(training_corpus: TrainingCorpus) -> None:
    baseline = training_corpus.root / "corrupt.zip"
    baseline.write_bytes(b"not a model bundle")
    request = write_request(training_corpus, "corrupt", baseline)
    with pytest.raises(zipfile.BadZipFile):
        run(request)
    assert not (request.parent / "result.pb").exists()
    assert not (training_corpus.root / "home.sqlite3").exists()


def test_training_uses_only_acknowledged_control_decisions(training_corpus: TrainingCorpus) -> None:
    train_inputs, _, held_inputs, _, _ = load_training_examples(
        training_corpus.root, training_corpus.digests, training_corpus.manifests, Recipe()
    )
    commands = {round(float(value), 1) for value in [*train_inputs[:, -1], *held_inputs[:, -1]]}
    assert commands == {0.1, 0.5, 0.9}


@pytest.mark.parametrize("outcome", [0, 99])
def test_result_rejects_unspecified_and_unknown_outcomes(tmp_path: Path, outcome: int) -> None:
    result = TrainingResult(schema_version=1)
    result.outcome = outcome
    with pytest.raises(ValueError, match="unspecified or unknown"):
        write_result(tmp_path, result)
    assert not (tmp_path / "result.pb").exists()
    assert not (tmp_path / "result.pb.staging").exists()


def test_result_preserves_present_zero_baseline(tmp_path: Path) -> None:
    result = TrainingResult(schema_version=1, outcome=TRAINING_OUTCOME_CANDIDATE, baseline_mse=0.0)
    write_result(tmp_path, result)
    decoded = TrainingResult.FromString((tmp_path / "result.pb").read_bytes())
    assert decoded.HasField("baseline_mse")
    assert decoded.baseline_mse == 0.0
    assert not decoded.HasField("held_out_mse")


@pytest.mark.parametrize("metric", ["maximum_parity_error", "held_out_mse", "baseline_mse"])
def test_result_rejects_nonfinite_metrics(tmp_path: Path, metric: str) -> None:
    result = TrainingResult(schema_version=1, outcome=TRAINING_OUTCOME_CANDIDATE)
    setattr(result, metric, float("nan"))
    with pytest.raises(ValueError, match="not finite"):
        write_result(tmp_path, result)
    assert not (tmp_path / "result.pb").exists()
