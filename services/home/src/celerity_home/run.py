"""Canonical Run protobuf framing and admission validation."""

from __future__ import annotations

import json
import math
import struct
from dataclasses import dataclass


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
        length, offset = _read_varint(chunk, offset)
        end = offset + length
        if length == 0 or end > len(chunk):
            raise RunFormatError("truncated Run record")

        schema_major: int | None = None
        sequence: int | None = None
        monotonic_ns: int | None = None
        source = ""
        kind = ""
        payload = ""
        payload_count = 0
        while offset < end:
            key, offset = _read_varint(chunk, offset)
            field = key >> 3
            wire = key & 7
            if wire == 0:
                value, offset = _read_varint(chunk, offset)
                if field == 1:
                    schema_major = value
                elif field == 4:
                    sequence = value
                elif field == 5:
                    monotonic_ns = value
            elif wire == 1:
                offset = _skip_fixed(chunk, offset, end, 8)
            elif wire == 2:
                value, offset = _take_length_delimited(chunk, offset, end)
                if field == 8:
                    source = _decode_utf8(value, "Run source")
                elif 20 <= field <= 29:
                    payload_count += 1
                    if field == 21:
                        signal, signal_value = _decode_signal_observation(value)
                        kind = "signal_observation"
                        payload = json.dumps({"signal": signal, "value": signal_value})
                    elif field == 24:
                        kind = "control_decision"
                        payload = _decode_embedded_string(value)
                    else:
                        kind = source
            elif wire == 5:
                offset = _skip_fixed(chunk, offset, end, 4)
            else:
                raise RunFormatError(f"unsupported Run protobuf wire type {wire}")

        if offset != end:
            raise RunFormatError("Run record exceeded its frame")
        if schema_major != 1:
            raise RunFormatError("Run record is not schema major 1")
        if sequence is None or sequence <= last_sequence:
            raise RunFormatError("Run record sequence is not strictly increasing")
        if monotonic_ns is None or monotonic_ns < last_monotonic_ns:
            raise RunFormatError("Run record monotonic time regressed")
        if not source:
            raise RunFormatError("Run record source is absent")
        if payload_count != 1:
            raise RunFormatError("Run record must contain exactly one typed payload")

        records.append(
            DecodedRunRecord(
                kind=kind or source,
                payload=payload,
                sequence=sequence,
                monotonic_ns=monotonic_ns,
            )
        )
        last_sequence = sequence
        last_monotonic_ns = monotonic_ns

    if not records:
        raise RunFormatError("Run chunk contains no records")
    return records


def _decode_embedded_string(message: bytes) -> str:
    if not message:
        raise RunFormatError("encoded Run payload is absent")
    key, offset = _read_varint(message, 0)
    if key != ((1 << 3) | 2):
        raise RunFormatError("invalid encoded Run payload")
    value, offset = _take_length_delimited(message, offset, len(message))
    if offset != len(message):
        raise RunFormatError("encoded Run payload has trailing fields")
    decoded = _decode_utf8(value, "encoded Run payload")
    if not decoded:
        raise RunFormatError("encoded Run payload is absent")
    return decoded


def _decode_signal_observation(message: bytes) -> tuple[str, float]:
    offset = 0
    signal = ""
    value = float("nan")
    while offset < len(message):
        key, offset = _read_varint(message, offset)
        field = key >> 3
        wire = key & 7
        if wire == 0:
            _, offset = _read_varint(message, offset)
        elif wire == 1:
            end = _skip_fixed(message, offset, len(message), 8)
            if field == 2:
                value = struct.unpack("<d", message[offset:end])[0]
            offset = end
        elif wire == 2:
            encoded, offset = _take_length_delimited(message, offset, len(message))
            if field == 1:
                signal = _decode_utf8(encoded, "signal name")
        elif wire == 5:
            offset = _skip_fixed(message, offset, len(message), 4)
        else:
            raise RunFormatError(f"unsupported signal protobuf wire type {wire}")
    if not signal or not math.isfinite(value):
        raise RunFormatError("invalid signal observation")
    return signal, value


def _take_length_delimited(data: bytes, offset: int, limit: int) -> tuple[bytes, int]:
    length, offset = _read_varint(data, offset)
    end = offset + length
    if end > limit or end > len(data):
        raise RunFormatError("truncated length-delimited protobuf field")
    return data[offset:end], end


def _skip_fixed(data: bytes, offset: int, limit: int, size: int) -> int:
    end = offset + size
    if end > limit or end > len(data):
        raise RunFormatError("truncated fixed-width protobuf field")
    return end


def _decode_utf8(value: bytes, field: str) -> str:
    try:
        return value.decode()
    except UnicodeDecodeError as error:
        raise RunFormatError(f"{field} is not UTF-8") from error


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
