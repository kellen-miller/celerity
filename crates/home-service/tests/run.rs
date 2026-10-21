use celerity_proto::celerity::v1::{ControlDecision, RunEvent, SignalObservation, run_event::Payload};
use home_service::run::{RunBounds, validate_records};
use prost::Message;

fn varint(mut value: u64) -> Vec<u8> {
    let mut encoded = Vec::new();
    while value >= 128 {
        encoded.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }

    encoded.push(value as u8);
    encoded
}

fn frame(record: &[u8]) -> Vec<u8> {
    let mut encoded = varint(record.len() as u64);
    encoded.extend_from_slice(record);
    encoded
}

fn event(sequence: u64, monotonic_ns: u64) -> RunEvent {
    RunEvent {
        schema_major: 1,
        sequence,
        monotonic_ns,
        source: "test".to_owned(),
        payload: Some(Payload::ControlDecision(ControlDecision {
            encoded: "{\"state\":\"accepted\"}".to_owned(),
        })),
        ..RunEvent::default()
    }
}

fn assert_invalid(data: &[u8], expected: &str) {
    let Err(error) = validate_records(data) else {
        panic!("expected invalid Run: {expected}");
    };
    let error = error.to_string();
    assert!(error.contains(expected), "expected {expected:?}, got {error:?}");
}

#[test]
fn validates_bounds_with_sequence_gaps_and_equal_times() {
    let mut data = event(3, 10).encode_length_delimited_to_vec();
    data.extend(event(8, 10).encode_length_delimited_to_vec());
    data.extend(event(9, 20).encode_length_delimited_to_vec());
    assert_eq!(
        validate_records(&data).unwrap(),
        RunBounds {
            first_sequence: 3,
            last_sequence: 9,
            first_monotonic_ns: 10,
            last_monotonic_ns: 20,
        }
    );
}

#[test]
fn rejects_duplicate_payloads_before_oneof_normalization() {
    for (field, payload) in [
        (
            24,
            ControlDecision {
                encoded: "x".to_owned(),
            }
            .encode_to_vec(),
        ),
        (
            21,
            SignalObservation {
                signal: "coolant".to_owned(),
                value: 90.0,
                ..SignalObservation::default()
            }
            .encode_to_vec(),
        ),
    ] {
        let mut record = event(1, 1).encode_to_vec();
        record.extend(varint((field << 3) | 2));
        record.extend(frame(&payload));
        assert_invalid(&frame(&record), "exactly one typed payload");
    }
}

#[test]
fn rejects_missing_and_empty_control_payloads() {
    let mut record = event(1, 1);
    record.payload = None;
    assert_invalid(&record.encode_length_delimited_to_vec(), "exactly one typed payload");
    record.payload = Some(Payload::ControlDecision(ControlDecision::default()));
    assert_invalid(
        &record.encode_length_delimited_to_vec(),
        "encoded Run payload is absent",
    );
    let mut raw = event(1, 1);
    raw.payload = None;
    let mut raw = raw.encode_to_vec();
    raw.extend([0xc2, 1, 2, 10, 0]);
    assert_invalid(&frame(&raw), "encoded Run payload is absent");
    raw.truncate(raw.len() - 5);
    raw.extend([0xc2, 1, 5, 10, 1, b'x', 16, 1]);
    assert_invalid(&frame(&raw), "encoded Run payload has trailing fields");
}

#[test]
fn rejects_nonfinite_missing_or_unnamed_signals() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0] {
        let mut record = event(1, 1);
        record.payload = Some(Payload::SignalObservation(SignalObservation {
            signal: "coolant".to_owned(),
            value,
            ..SignalObservation::default()
        }));
        assert_invalid(&record.encode_length_delimited_to_vec(), "invalid signal observation");
    }

    let mut record = event(1, 1);
    record.payload = Some(Payload::SignalObservation(SignalObservation {
        value: 90.0,
        ..SignalObservation::default()
    }));
    assert_invalid(&record.encode_length_delimited_to_vec(), "invalid signal observation");
    record.payload = Some(Payload::SignalObservation(SignalObservation {
        signal: "coolant".to_owned(),
        value: -12.5,
        ..SignalObservation::default()
    }));
    assert!(validate_records(&record.encode_length_delimited_to_vec()).is_ok());
}

#[test]
fn rejects_sequence_time_source_and_schema_errors() {
    for sequence in [0, 1, 2] {
        let mut data = event(2, 10).encode_length_delimited_to_vec();
        data.extend(event(sequence, 11).encode_length_delimited_to_vec());
        assert_invalid(&data, "sequence is not strictly increasing");
    }

    let mut data = event(1, 10).encode_length_delimited_to_vec();
    data.extend(event(2, 9).encode_length_delimited_to_vec());
    assert_invalid(&data, "monotonic time regressed");
    assert_invalid(
        &event(1, 0).encode_length_delimited_to_vec(),
        "monotonic time regressed",
    );
    let mut record = event(1, 1);
    record.source.clear();
    assert_invalid(&record.encode_length_delimited_to_vec(), "source is absent");
    record.source = "test".to_owned();
    record.schema_major = 2;
    assert_invalid(&record.encode_length_delimited_to_vec(), "not schema major 1");
}

struct InvalidFrame<'a> {
    data: &'a [u8],
    expected: &'a str,
}

#[test]
fn rejects_bad_framing_and_wire_types() {
    for case in [
        InvalidFrame {
            data: &[],
            expected: "contains no records",
        },
        InvalidFrame {
            data: &[0],
            expected: "truncated Run record",
        },
        InvalidFrame {
            data: &[0x80],
            expected: "invalid protobuf varint",
        },
        InvalidFrame {
            data: &[2, 8],
            expected: "truncated Run record",
        },
        InvalidFrame {
            data: &[1, 8],
            expected: "invalid protobuf varint",
        },
        InvalidFrame {
            data: &[1, 15],
            expected: "unsupported Run protobuf wire type 7",
        },
        InvalidFrame {
            data: &[2, 10, 5],
            expected: "truncated length-delimited",
        },
        InvalidFrame {
            data: &[2, 9, 0],
            expected: "truncated fixed-width",
        },
    ] {
        assert_invalid(case.data, case.expected);
    }

    let mut overflow = vec![0xff; 10];
    overflow.push(1);
    assert_invalid(&overflow, "invalid protobuf varint");
    let mut crossed = vec![1, 8];
    crossed.extend(event(1, 1).encode_length_delimited_to_vec());
    assert_invalid(&crossed, "invalid protobuf varint");
}

#[test]
fn accepts_explicit_zero_monotonic_and_signal_values() {
    let mut record = event(1, 0);
    record.payload = None;
    let mut raw = record.encode_to_vec();
    raw.extend([40, 0]);
    let mut signal = vec![10, 1, b'x', 17];
    signal.extend(0_f64.to_le_bytes());
    raw.extend([0xaa, 1]);
    raw.extend(frame(&signal));
    assert_eq!(validate_records(&frame(&raw)).unwrap().first_monotonic_ns, 0);
}
