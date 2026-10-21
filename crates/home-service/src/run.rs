//! Canonical framed Run admission, before protobuf oneof normalization.

use celerity_proto::celerity::v1::RunEvent;
use prost::Message;

use crate::HomeResult;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RunBounds {
    pub first_sequence: u64,
    pub last_sequence: u64,
    pub first_monotonic_ns: u64,
    pub last_monotonic_ns: u64,
}

enum WireValue<'a> {
    Varint,
    Fixed64(&'a [u8]),
    Bytes(&'a [u8]),
    Fixed32,
}

struct Cursor<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn varint(&mut self) -> HomeResult<u64> {
        let mut value = 0_u64;
        for shift in (0..70).step_by(7) {
            let Some(&byte) = self.data.get(self.offset) else {
                return Err("invalid protobuf varint".into());
            };
            self.offset += 1;
            if shift == 63 && byte > 1 {
                return Err("invalid protobuf varint".into());
            }

            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Ok(value);
            }
        }

        Err("invalid protobuf varint".into())
    }

    fn take(&mut self, length: usize, error: &str) -> HomeResult<&'a [u8]> {
        let end = self.offset.checked_add(length).ok_or_else(|| error.to_owned())?;
        let value = self.data.get(self.offset..end).ok_or_else(|| error.to_owned())?;
        self.offset = end;
        Ok(value)
    }

    fn field(&mut self, label: &str) -> HomeResult<(u64, WireValue<'a>)> {
        let key = self.varint()?;
        let value = match key & 7 {
            0 => {
                self.varint()?;
                WireValue::Varint
            }
            1 => WireValue::Fixed64(self.take(8, "truncated fixed-width protobuf field")?),
            2 => {
                let length = usize::try_from(self.varint()?)?;
                WireValue::Bytes(self.take(length, "truncated length-delimited protobuf field")?)
            }
            5 => {
                self.take(4, "truncated fixed-width protobuf field")?;
                WireValue::Fixed32
            }
            wire => return Err(format!("unsupported {label} protobuf wire type {wire}").into()),
        };
        Ok((key >> 3, value))
    }
}

/// Validate canonical Run v1 frames and return their sequence/time bounds.
///
/// # Errors
/// Rejects malformed framing, invalid records, duplicate typed payloads,
/// non-finite signal values, and nonmonotonic sequence/time bounds.
pub fn validate_records(data: &[u8]) -> HomeResult<RunBounds> {
    let mut chunk = Cursor::new(data);
    let mut bounds = None::<RunBounds>;
    while chunk.offset < data.len() {
        let length = usize::try_from(chunk.varint()?)?;
        if length == 0 {
            return Err("truncated Run record".into());
        }

        let frame = chunk.take(length, "truncated Run record")?;
        let mut record = Cursor::new(frame);
        let mut payload_count = 0;
        let mut sequence_present = false;
        let mut monotonic_present = false;
        while record.offset < frame.len() {
            let (field, value) = record.field("Run")?;
            match (field, value) {
                (4, WireValue::Varint) => sequence_present = true,
                (5, WireValue::Varint) => monotonic_present = true,
                (8, WireValue::Bytes(source)) => {
                    std::str::from_utf8(source).map_err(|_| "Run source is not UTF-8")?;
                }
                (20..=29, WireValue::Bytes(payload)) => {
                    payload_count += 1;
                    match field {
                        21 => validate_signal(payload)?,
                        24 => validate_control(payload)?,
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        let event = RunEvent::decode(frame)?;
        if event.schema_major != 1 {
            return Err("Run record is not schema major 1".into());
        }

        let last_sequence = bounds.map_or(0, |bounds| bounds.last_sequence);
        let last_monotonic_ns = bounds.map_or(0, |bounds| bounds.last_monotonic_ns);
        if !sequence_present || event.sequence <= last_sequence {
            return Err("Run record sequence is not strictly increasing".into());
        }

        if !monotonic_present || event.monotonic_ns < last_monotonic_ns {
            return Err("Run record monotonic time regressed".into());
        }

        if event.source.is_empty() {
            return Err("Run record source is absent".into());
        }

        if payload_count != 1 {
            return Err("Run record must contain exactly one typed payload".into());
        }

        match &mut bounds {
            Some(bounds) => {
                bounds.last_sequence = event.sequence;
                bounds.last_monotonic_ns = event.monotonic_ns;
            }
            None => {
                bounds = Some(RunBounds {
                    first_sequence: event.sequence,
                    last_sequence: event.sequence,
                    first_monotonic_ns: event.monotonic_ns,
                    last_monotonic_ns: event.monotonic_ns,
                });
            }
        }
    }

    bounds.ok_or_else(|| "Run chunk contains no records".into())
}

fn validate_signal(payload: &[u8]) -> HomeResult<()> {
    let mut signal = Cursor::new(payload);
    let mut name = "";
    let mut value = f64::NAN;
    while signal.offset < payload.len() {
        match signal.field("signal")? {
            (1, WireValue::Bytes(encoded)) => {
                name = std::str::from_utf8(encoded).map_err(|_| "signal name is not UTF-8")?;
            }
            (2, WireValue::Fixed64(bytes)) => value = f64::from_le_bytes(bytes.try_into()?),
            _ => {}
        }
    }

    if name.is_empty() || !value.is_finite() {
        return Err("invalid signal observation".into());
    }

    Ok(())
}

fn validate_control(payload: &[u8]) -> HomeResult<()> {
    if payload.is_empty() {
        return Err("encoded Run payload is absent".into());
    }

    let mut control = Cursor::new(payload);
    if control.varint()? != 10 {
        return Err("invalid encoded Run payload".into());
    }

    let size = usize::try_from(control.varint()?)?;
    let encoded = control.take(size, "truncated length-delimited protobuf field")?;
    if control.offset != payload.len() {
        return Err("encoded Run payload has trailing fields".into());
    }

    let encoded = std::str::from_utf8(encoded).map_err(|_| "encoded Run payload is not UTF-8")?;
    if encoded.is_empty() {
        return Err("encoded Run payload is absent".into());
    }

    Ok(())
}
