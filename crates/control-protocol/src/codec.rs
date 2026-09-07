use super::{MAX_NODE_ADDRESS, MAX_SPLIT_BASIS_POINTS, WireError};

pub(crate) fn frame_family(can_id: u16) -> Result<(u16, u8), WireError> {
    if can_id == 0x080 {
        return Ok((0x080, 0));
    }

    for base in [
        0x040, 0x100, 0x140, 0x180, 0x1c0, 0x200, 0x240, 0x280, 0x2c0, 0x300, 0x340, 0x380,
    ] {
        if can_id > base && can_id <= base + u16::from(MAX_NODE_ADDRESS) {
            return Ok((
                base,
                u8::try_from(can_id - base).expect("node range fits u8"),
            ));
        }
    }

    Err(WireError::UnknownId(can_id))
}

pub(crate) fn node_id(base: u16, node: u8) -> Result<u16, WireError> {
    if !(1..=MAX_NODE_ADDRESS).contains(&node) {
        return Err(WireError::InvalidNode(node));
    }

    Ok(base + u16::from(node))
}

pub(crate) const fn exact_length(payload: &[u8], expected: usize) -> Result<(), WireError> {
    if payload.len() != expected {
        return Err(WireError::WrongLength {
            expected,
            actual: payload.len(),
        });
    }

    Ok(())
}

pub(crate) fn reserved_zero(
    payload: &[u8],
    range: core::ops::Range<usize>,
) -> Result<(), WireError> {
    if payload[range].iter().any(|byte| *byte != 0) {
        return Err(WireError::ReservedNotZero);
    }

    Ok(())
}

pub(crate) const fn validate_split(value: u16) -> Result<(), WireError> {
    if value > MAX_SPLIT_BASIS_POINTS {
        return Err(WireError::SplitOutOfRange(value));
    }

    Ok(())
}

pub(crate) const fn validate_enum(
    field: &'static str,
    value: u8,
    maximum: u8,
) -> Result<(), WireError> {
    if value == 0 || value > maximum {
        return Err(WireError::InvalidEnum { field, value });
    }

    Ok(())
}

pub(crate) fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

pub(crate) fn get_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        input[offset..offset + 2]
            .try_into()
            .expect("validated payload length"),
    )
}

pub(crate) fn get_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        input[offset..offset + 4]
            .try_into()
            .expect("validated payload length"),
    )
}

pub(crate) fn get_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        input[offset..offset + 8]
            .try_into()
            .expect("validated payload length"),
    )
}
