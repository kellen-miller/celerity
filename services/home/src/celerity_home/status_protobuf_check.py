"""Dependency-free assertion for the systemd status protobuf response."""

import sys
from pathlib import Path


def assert_status(response_path: Path, expected_state: str, expected_snapshot: str) -> None:
    expected_state_value = {"current": 1, "stale": 2, "unavailable": 3}[expected_state]
    fields: dict[int, int | bytes] = {}
    data = response_path.read_bytes()
    offset = 0

    while offset < len(data):
        key = 0
        shift = 0
        while True:
            if offset >= len(data):
                raise ValueError("truncated protobuf key")
            byte = data[offset]
            offset += 1
            key |= (byte & 0x7F) << shift
            if byte < 0x80:
                break
            shift += 7
            if shift >= 70:
                raise ValueError("invalid protobuf key")
        field_number = key >> 3
        wire_type = key & 0x07
        if wire_type == 0:
            value = 0
            shift = 0
            while True:
                if offset >= len(data):
                    raise ValueError("truncated protobuf varint")
                byte = data[offset]
                offset += 1
                value |= (byte & 0x7F) << shift
                if byte < 0x80:
                    break
                shift += 7
                if shift >= 70:
                    raise ValueError("invalid protobuf varint")
            fields[field_number] = value
        elif wire_type == 1:
            offset += 8
        elif wire_type == 2:
            length = 0
            shift = 0
            while True:
                if offset >= len(data):
                    raise ValueError("truncated protobuf length")
                byte = data[offset]
                offset += 1
                length |= (byte & 0x7F) << shift
                if byte < 0x80:
                    break
                shift += 7
                if shift >= 70:
                    raise ValueError("invalid protobuf length")
            end = offset + length
            if end > len(data):
                raise ValueError("truncated protobuf field")
            fields[field_number] = data[offset:end]
            offset = end
        elif wire_type == 5:
            offset += 4
        else:
            raise ValueError(f"unsupported protobuf wire type {wire_type}")

    assert fields.get(1) == 1
    assert fields.get(2) == expected_state_value
    assert (5 in fields) == (expected_snapshot == "present")


def main() -> None:
    response_path, expected_state, expected_snapshot = sys.argv[1:]
    assert_status(Path(response_path), expected_state, expected_snapshot)


if __name__ == "__main__":
    main()
