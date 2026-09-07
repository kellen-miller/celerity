"""Canonical Run protobuf framing and admission validation."""

from __future__ import annotations

import json
import math
from dataclasses import dataclass

from google.protobuf.message import DecodeError

from celerity.v1.celerity_pb2 import RunEvent, RunManifest


class RunFormatError(RuntimeError):
    """A Run chunk is not structurally valid canonical v1 evidence."""


@dataclass(frozen=True)
class DecodedRunRecord:
    """The ordered fields needed by home-side admission and derivation."""

    kind: str
    payload: str
    sequence: int
    monotonic_ns: int


def decode_run_records(chunk: bytes) -> list[DecodedRunRecord]:
    """Decode and validate one length-delimited canonical Run v1 chunk."""
    records: list[DecodedRunRecord] = []
    offset = 0
    last_sequence = 0
    last_monotonic_ns = 0
    while offset < len(chunk):
        length, record_offset = _read_varint(chunk, offset)
        end = record_offset + length
        if length == 0 or end > len(chunk):
            raise RunFormatError("truncated Run record")
        try:
            record = RunEvent.FromString(chunk[record_offset:end])
        except DecodeError as error:
            raise RunFormatError("invalid Run record protobuf") from error
        offset = end

        if record.schema_major != 1:
            raise RunFormatError("Run record is not schema major 1")
        if record.sequence <= last_sequence:
            raise RunFormatError("Run record sequence is not strictly increasing")
        if record.monotonic_ns < last_monotonic_ns:
            raise RunFormatError("Run record monotonic time regressed")
        if not record.source:
            raise RunFormatError("Run record source is absent")

        payload_name = record.WhichOneof("payload")
        if payload_name is None:
            raise RunFormatError("Run record must contain exactly one typed payload")
        if payload_name == "signal_observation":
            observation = record.signal_observation
            if not observation.signal or not math.isfinite(observation.value):
                raise RunFormatError("invalid signal observation")
            kind = "signal_observation"
            payload = json.dumps({"signal": observation.signal, "value": observation.value})
        elif payload_name == "control_decision":
            kind = "control_decision"
            payload = record.control_decision.encoded
        else:
            kind = record.source
            payload = ""

        records.append(
            DecodedRunRecord(
                kind=kind,
                payload=payload,
                sequence=record.sequence,
                monotonic_ns=record.monotonic_ns,
            )
        )
        last_sequence = record.sequence
        last_monotonic_ns = record.monotonic_ns

    if not records:
        raise RunFormatError("Run chunk contains no records")
    return records


def _read_varint(data: bytes, offset: int) -> tuple[int, int]:
    value = 0
    shift = 0
    while offset < len(data) and shift < 70:
        byte = data[offset]
        offset += 1
        value |= (byte & 0x7F) << shift
        if byte < 0x80:
            return value, offset
        shift += 7
    raise RunFormatError("invalid protobuf varint")


def run_contract(manifest: RunManifest) -> str:
    """Return the stable model contract key for one immutable Run."""
    return json.dumps(
        [
            manifest.model_abi,
            list(manifest.model_input_signals),
            manifest.model_history_length,
            manifest.sample_period_ms,
            list(manifest.command_lattice),
            manifest.maximum_calibration_error,
        ],
        sort_keys=True,
    )
