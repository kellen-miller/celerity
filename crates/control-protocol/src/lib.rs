#![no_std]

/// The production controller protocol major version.
pub const PROTOCOL_MAJOR: u8 = 1;
pub const MAX_NODE_ADDRESS: u8 = 63;
pub const MAX_SPLIT_BASIS_POINTS: u16 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EncodedFrame {
    pub can_id: u16,
    pub len: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WireError {
    UnknownId(u16),
    InvalidNode(u8),
    WrongLength { expected: usize, actual: usize },
    ReservedNotZero,
    SplitOutOfRange(u16),
    InvalidEnum { field: &'static str, value: u8 },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscoveryProbe {
    pub protocol_major: u8,
    pub protocol_minor: u8,
    pub flags: u16,
    pub probe_sequence: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeAnnounce {
    pub protocol_major: u8,
    pub protocol_minor: u8,
    pub lifecycle: u8,
    pub state_flags: u8,
    pub boot_session: u32,
    pub provisioned_identity: u64,
    pub firmware_generation: u32,
    pub capability_generation: u32,
    pub configuration_generation: u32,
    pub announce_sequence: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityReport {
    pub boot_session: u32,
    pub capability_generation: u32,
    pub resource_id: u32,
    pub minimum_basis_points: u16,
    pub maximum_basis_points: u16,
    pub capability_flags: u16,
    pub maximum_command_rate_hz: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Configuration {
    pub boot_session: u32,
    pub configuration_generation: u32,
    pub fallback_basis_points: u16,
    pub pwm_endpoint_a_us: u16,
    pub pwm_endpoint_b_us: u16,
    pub direction: u8,
    pub flags: u8,
    pub runtime_lease_ms: u16,
    pub command_lease_ms: u16,
    pub heartbeat_period_ms: u16,
    pub acknowledgement_deadline_ms: u16,
    pub normal_slew_basis_points_per_second: u16,
    pub protection_slew_basis_points_per_second: u16,
    pub digest_prefix: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigurationAck {
    pub boot_session: u32,
    pub configuration_generation: u32,
    pub digest_prefix: u32,
    pub result: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeLease {
    pub boot_session: u32,
    pub configuration_generation: u32,
    pub epoch: u64,
    pub renewal_sequence: u32,
    pub validity_ms: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeLeaseAck {
    pub boot_session: u32,
    pub configuration_generation: u32,
    pub epoch: u64,
    pub renewal_sequence: u32,
    pub result: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Command {
    pub boot_session: u32,
    pub configuration_generation: u32,
    pub epoch: u64,
    pub command_sequence: u32,
    pub radiator_split_basis_points: u16,
    pub flags: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandAck {
    pub boot_session: u32,
    pub configuration_generation: u32,
    pub epoch: u64,
    pub command_sequence: u32,
    pub accepted_basis_points: u16,
    pub mode: u8,
    pub fault_latch: u8,
    pub result: u8,
    pub output_state: u8,
    pub command_lease_remaining_ms: u16,
    pub acknowledgement_sequence: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Heartbeat {
    pub boot_session: u32,
    pub configuration_generation: u32,
    pub capability_generation: u32,
    pub heartbeat_sequence: u32,
    pub current_epoch: u64,
    pub last_command_sequence: u32,
    pub accepted_basis_points: u16,
    pub state_flags: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FaultReport {
    pub boot_session: u32,
    pub fault_sequence: u32,
    pub fault_code: u16,
    pub severity: u8,
    pub flags: u8,
    pub related_epoch: u64,
    pub related_command_sequence: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FallbackRequest {
    pub boot_session: u32,
    pub epoch: u64,
    pub request_sequence: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FallbackAck {
    pub boot_session: u32,
    pub request_sequence: u32,
    pub accepted_basis_points: u16,
    pub mode: u8,
    pub fault_latch: u8,
    pub result: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Frame {
    DiscoveryProbe(DiscoveryProbe),
    FaultReport { node: u8, message: FaultReport },
    Command { node: u8, message: Command },
    CommandAck { node: u8, message: CommandAck },
    RuntimeLease { node: u8, message: RuntimeLease },
    RuntimeLeaseAck { node: u8, message: RuntimeLeaseAck },
    Heartbeat { node: u8, message: Heartbeat },
    FallbackRequest { node: u8, message: FallbackRequest },
    FallbackAck { node: u8, message: FallbackAck },
    Configuration { node: u8, message: Configuration },
    ConfigurationAck { node: u8, message: ConfigurationAck },
    NodeAnnounce { node: u8, message: NodeAnnounce },
    CapabilityReport { node: u8, message: CapabilityReport },
}

/// Encodes one typed v1 frame into a caller-owned CAN FD buffer.
///
/// # Errors
///
/// Returns a [`WireError`] when a typed value is outside the fixed v1 domain.
pub fn encode(frame: &Frame, output: &mut [u8; 64]) -> Result<EncodedFrame, WireError> {
    output.fill(0);
    let (can_id, len) = match frame {
        Frame::DiscoveryProbe(message) => {
            output[0] = message.protocol_major;
            output[1] = message.protocol_minor;
            put_u16(output, 2, message.flags);
            put_u32(output, 4, message.probe_sequence);
            (0x080, 8)
        }
        Frame::FaultReport { node, message } => {
            let can_id = node_id(0x040, *node)?;
            validate_enum("severity", message.severity, 4)?;
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.fault_sequence);
            put_u16(output, 8, message.fault_code);
            output[10] = message.severity;
            output[11] = message.flags;
            put_u64(output, 12, message.related_epoch);
            put_u32(output, 20, message.related_command_sequence);
            (can_id, 24)
        }
        Frame::Command { node, message } => {
            let can_id = node_id(0x100, *node)?;
            validate_split(message.radiator_split_basis_points)?;
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.configuration_generation);
            put_u64(output, 8, message.epoch);
            put_u32(output, 16, message.command_sequence);
            put_u16(output, 20, message.radiator_split_basis_points);
            output[22] = message.flags;
            (can_id, 24)
        }
        Frame::CommandAck { node, message } => {
            let can_id = node_id(0x140, *node)?;
            validate_split(message.accepted_basis_points)?;
            validate_enum("mode", message.mode, 2)?;
            validate_enum("fault_latch", message.fault_latch, 2)?;
            validate_enum("result", message.result, 8)?;
            validate_enum("output_state", message.output_state, 3)?;
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.configuration_generation);
            put_u64(output, 8, message.epoch);
            put_u32(output, 16, message.command_sequence);
            put_u16(output, 20, message.accepted_basis_points);
            output[22] = message.mode;
            output[23] = message.fault_latch;
            output[24] = message.result;
            output[25] = message.output_state;
            put_u16(output, 26, message.command_lease_remaining_ms);
            put_u32(output, 28, message.acknowledgement_sequence);
            (can_id, 32)
        }
        Frame::RuntimeLease { node, message } => {
            let can_id = node_id(0x180, *node)?;
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.configuration_generation);
            put_u64(output, 8, message.epoch);
            put_u32(output, 16, message.renewal_sequence);
            put_u16(output, 20, message.validity_ms);
            (can_id, 24)
        }
        Frame::RuntimeLeaseAck { node, message } => {
            let can_id = node_id(0x1c0, *node)?;
            validate_enum("result", message.result, 8)?;
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.configuration_generation);
            put_u64(output, 8, message.epoch);
            put_u32(output, 16, message.renewal_sequence);
            output[20] = message.result;
            (can_id, 24)
        }
        Frame::Heartbeat { node, message } => {
            let can_id = node_id(0x200, *node)?;
            validate_split(message.accepted_basis_points)?;
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.configuration_generation);
            put_u32(output, 8, message.capability_generation);
            put_u32(output, 12, message.heartbeat_sequence);
            put_u64(output, 16, message.current_epoch);
            put_u32(output, 24, message.last_command_sequence);
            put_u16(output, 28, message.accepted_basis_points);
            put_u16(output, 30, message.state_flags);
            (can_id, 32)
        }
        Frame::FallbackRequest { node, message } => {
            let can_id = node_id(0x240, *node)?;
            put_u32(output, 0, message.boot_session);
            put_u64(output, 4, message.epoch);
            put_u32(output, 12, message.request_sequence);
            (can_id, 16)
        }
        Frame::FallbackAck { node, message } => {
            let can_id = node_id(0x280, *node)?;
            validate_split(message.accepted_basis_points)?;
            validate_enum("mode", message.mode, 2)?;
            validate_enum("fault_latch", message.fault_latch, 2)?;
            validate_enum("result", message.result, 8)?;
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.request_sequence);
            put_u16(output, 8, message.accepted_basis_points);
            output[10] = message.mode;
            output[11] = message.fault_latch;
            output[12] = message.result;
            (can_id, 16)
        }
        Frame::Configuration { node, message } => {
            let can_id = node_id(0x2c0, *node)?;
            validate_split(message.fallback_basis_points)?;
            validate_enum("direction", message.direction, 2)?;
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.configuration_generation);
            put_u16(output, 8, message.fallback_basis_points);
            put_u16(output, 10, message.pwm_endpoint_a_us);
            put_u16(output, 12, message.pwm_endpoint_b_us);
            output[14] = message.direction;
            output[15] = message.flags;
            put_u16(output, 16, message.runtime_lease_ms);
            put_u16(output, 18, message.command_lease_ms);
            put_u16(output, 20, message.heartbeat_period_ms);
            put_u16(output, 22, message.acknowledgement_deadline_ms);
            put_u16(output, 24, message.normal_slew_basis_points_per_second);
            put_u16(output, 26, message.protection_slew_basis_points_per_second);
            put_u32(output, 28, message.digest_prefix);
            (can_id, 32)
        }
        Frame::ConfigurationAck { node, message } => {
            let can_id = node_id(0x300, *node)?;
            validate_enum("result", message.result, 8)?;
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.configuration_generation);
            put_u32(output, 8, message.digest_prefix);
            output[12] = message.result;
            (can_id, 16)
        }
        Frame::NodeAnnounce { node, message } => {
            let can_id = node_id(0x340, *node)?;
            validate_enum("lifecycle", message.lifecycle, 4)?;
            output[0] = message.protocol_major;
            output[1] = message.protocol_minor;
            output[2] = message.lifecycle;
            output[3] = message.state_flags;
            put_u32(output, 4, message.boot_session);
            put_u64(output, 8, message.provisioned_identity);
            put_u32(output, 16, message.firmware_generation);
            put_u32(output, 20, message.capability_generation);
            put_u32(output, 24, message.configuration_generation);
            put_u32(output, 28, message.announce_sequence);
            (can_id, 32)
        }
        Frame::CapabilityReport { node, message } => {
            let can_id = node_id(0x380, *node)?;
            validate_split(message.minimum_basis_points)?;
            validate_split(message.maximum_basis_points)?;
            if message.minimum_basis_points > message.maximum_basis_points {
                return Err(WireError::SplitOutOfRange(message.minimum_basis_points));
            }
            put_u32(output, 0, message.boot_session);
            put_u32(output, 4, message.capability_generation);
            put_u32(output, 8, message.resource_id);
            put_u16(output, 12, message.minimum_basis_points);
            put_u16(output, 14, message.maximum_basis_points);
            put_u16(output, 16, message.capability_flags);
            put_u16(output, 18, message.maximum_command_rate_hz);
            (can_id, 24)
        }
    };

    Ok(EncodedFrame { can_id, len })
}

/// Decodes and validates one fixed v1 CAN FD payload.
///
/// # Errors
///
/// Returns a [`WireError`] for unknown IDs, wrong lengths, invalid fields, or
/// nonzero reserved bytes.
pub fn decode(can_id: u16, payload: &[u8]) -> Result<Frame, WireError> {
    let (family, node) = frame_family(can_id)?;
    let frame = match family {
        0x080 => {
            exact_length(payload, 8)?;
            Frame::DiscoveryProbe(DiscoveryProbe {
                protocol_major: payload[0],
                protocol_minor: payload[1],
                flags: get_u16(payload, 2),
                probe_sequence: get_u32(payload, 4),
            })
        }
        0x040 => {
            exact_length(payload, 24)?;
            validate_enum("severity", payload[10], 4)?;
            Frame::FaultReport {
                node,
                message: FaultReport {
                    boot_session: get_u32(payload, 0),
                    fault_sequence: get_u32(payload, 4),
                    fault_code: get_u16(payload, 8),
                    severity: payload[10],
                    flags: payload[11],
                    related_epoch: get_u64(payload, 12),
                    related_command_sequence: get_u32(payload, 20),
                },
            }
        }
        0x100 => {
            exact_length(payload, 24)?;
            reserved_zero(payload, 23..24)?;
            let split = get_u16(payload, 20);
            validate_split(split)?;
            Frame::Command {
                node,
                message: Command {
                    boot_session: get_u32(payload, 0),
                    configuration_generation: get_u32(payload, 4),
                    epoch: get_u64(payload, 8),
                    command_sequence: get_u32(payload, 16),
                    radiator_split_basis_points: split,
                    flags: payload[22],
                },
            }
        }
        0x140 => {
            exact_length(payload, 32)?;
            let split = get_u16(payload, 20);
            validate_split(split)?;
            validate_enum("mode", payload[22], 2)?;
            validate_enum("fault_latch", payload[23], 2)?;
            validate_enum("result", payload[24], 8)?;
            validate_enum("output_state", payload[25], 3)?;
            Frame::CommandAck {
                node,
                message: CommandAck {
                    boot_session: get_u32(payload, 0),
                    configuration_generation: get_u32(payload, 4),
                    epoch: get_u64(payload, 8),
                    command_sequence: get_u32(payload, 16),
                    accepted_basis_points: split,
                    mode: payload[22],
                    fault_latch: payload[23],
                    result: payload[24],
                    output_state: payload[25],
                    command_lease_remaining_ms: get_u16(payload, 26),
                    acknowledgement_sequence: get_u32(payload, 28),
                },
            }
        }
        0x180 => {
            exact_length(payload, 24)?;
            reserved_zero(payload, 22..24)?;
            Frame::RuntimeLease {
                node,
                message: RuntimeLease {
                    boot_session: get_u32(payload, 0),
                    configuration_generation: get_u32(payload, 4),
                    epoch: get_u64(payload, 8),
                    renewal_sequence: get_u32(payload, 16),
                    validity_ms: get_u16(payload, 20),
                },
            }
        }
        0x1c0 => {
            exact_length(payload, 24)?;
            validate_enum("result", payload[20], 8)?;
            reserved_zero(payload, 21..24)?;
            Frame::RuntimeLeaseAck {
                node,
                message: RuntimeLeaseAck {
                    boot_session: get_u32(payload, 0),
                    configuration_generation: get_u32(payload, 4),
                    epoch: get_u64(payload, 8),
                    renewal_sequence: get_u32(payload, 16),
                    result: payload[20],
                },
            }
        }
        0x200 => {
            exact_length(payload, 32)?;
            let split = get_u16(payload, 28);
            validate_split(split)?;
            Frame::Heartbeat {
                node,
                message: Heartbeat {
                    boot_session: get_u32(payload, 0),
                    configuration_generation: get_u32(payload, 4),
                    capability_generation: get_u32(payload, 8),
                    heartbeat_sequence: get_u32(payload, 12),
                    current_epoch: get_u64(payload, 16),
                    last_command_sequence: get_u32(payload, 24),
                    accepted_basis_points: split,
                    state_flags: get_u16(payload, 30),
                },
            }
        }
        0x240 => {
            exact_length(payload, 16)?;
            Frame::FallbackRequest {
                node,
                message: FallbackRequest {
                    boot_session: get_u32(payload, 0),
                    epoch: get_u64(payload, 4),
                    request_sequence: get_u32(payload, 12),
                },
            }
        }
        0x280 => {
            exact_length(payload, 16)?;
            let split = get_u16(payload, 8);
            validate_split(split)?;
            validate_enum("mode", payload[10], 2)?;
            validate_enum("fault_latch", payload[11], 2)?;
            validate_enum("result", payload[12], 8)?;
            reserved_zero(payload, 13..16)?;
            Frame::FallbackAck {
                node,
                message: FallbackAck {
                    boot_session: get_u32(payload, 0),
                    request_sequence: get_u32(payload, 4),
                    accepted_basis_points: split,
                    mode: payload[10],
                    fault_latch: payload[11],
                    result: payload[12],
                },
            }
        }
        0x2c0 => {
            exact_length(payload, 32)?;
            let split = get_u16(payload, 8);
            validate_split(split)?;
            validate_enum("direction", payload[14], 2)?;
            Frame::Configuration {
                node,
                message: Configuration {
                    boot_session: get_u32(payload, 0),
                    configuration_generation: get_u32(payload, 4),
                    fallback_basis_points: split,
                    pwm_endpoint_a_us: get_u16(payload, 10),
                    pwm_endpoint_b_us: get_u16(payload, 12),
                    direction: payload[14],
                    flags: payload[15],
                    runtime_lease_ms: get_u16(payload, 16),
                    command_lease_ms: get_u16(payload, 18),
                    heartbeat_period_ms: get_u16(payload, 20),
                    acknowledgement_deadline_ms: get_u16(payload, 22),
                    normal_slew_basis_points_per_second: get_u16(payload, 24),
                    protection_slew_basis_points_per_second: get_u16(payload, 26),
                    digest_prefix: get_u32(payload, 28),
                },
            }
        }
        0x300 => {
            exact_length(payload, 16)?;
            validate_enum("result", payload[12], 8)?;
            reserved_zero(payload, 13..16)?;
            Frame::ConfigurationAck {
                node,
                message: ConfigurationAck {
                    boot_session: get_u32(payload, 0),
                    configuration_generation: get_u32(payload, 4),
                    digest_prefix: get_u32(payload, 8),
                    result: payload[12],
                },
            }
        }
        0x340 => {
            exact_length(payload, 32)?;
            validate_enum("lifecycle", payload[2], 4)?;
            Frame::NodeAnnounce {
                node,
                message: NodeAnnounce {
                    protocol_major: payload[0],
                    protocol_minor: payload[1],
                    lifecycle: payload[2],
                    state_flags: payload[3],
                    boot_session: get_u32(payload, 4),
                    provisioned_identity: get_u64(payload, 8),
                    firmware_generation: get_u32(payload, 16),
                    capability_generation: get_u32(payload, 20),
                    configuration_generation: get_u32(payload, 24),
                    announce_sequence: get_u32(payload, 28),
                },
            }
        }
        0x380 => {
            exact_length(payload, 24)?;
            let minimum = get_u16(payload, 12);
            let maximum = get_u16(payload, 14);
            validate_split(minimum)?;
            validate_split(maximum)?;
            if minimum > maximum {
                return Err(WireError::SplitOutOfRange(minimum));
            }
            reserved_zero(payload, 20..24)?;
            Frame::CapabilityReport {
                node,
                message: CapabilityReport {
                    boot_session: get_u32(payload, 0),
                    capability_generation: get_u32(payload, 4),
                    resource_id: get_u32(payload, 8),
                    minimum_basis_points: minimum,
                    maximum_basis_points: maximum,
                    capability_flags: get_u16(payload, 16),
                    maximum_command_rate_hz: get_u16(payload, 18),
                },
            }
        }
        _ => return Err(WireError::UnknownId(can_id)),
    };

    Ok(frame)
}

fn frame_family(can_id: u16) -> Result<(u16, u8), WireError> {
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

fn node_id(base: u16, node: u8) -> Result<u16, WireError> {
    if !(1..=MAX_NODE_ADDRESS).contains(&node) {
        return Err(WireError::InvalidNode(node));
    }

    Ok(base + u16::from(node))
}

fn exact_length(payload: &[u8], expected: usize) -> Result<(), WireError> {
    if payload.len() != expected {
        return Err(WireError::WrongLength {
            expected,
            actual: payload.len(),
        });
    }

    Ok(())
}

fn reserved_zero(payload: &[u8], range: core::ops::Range<usize>) -> Result<(), WireError> {
    if payload[range].iter().any(|byte| *byte != 0) {
        return Err(WireError::ReservedNotZero);
    }

    Ok(())
}

fn validate_split(value: u16) -> Result<(), WireError> {
    if value > MAX_SPLIT_BASIS_POINTS {
        return Err(WireError::SplitOutOfRange(value));
    }

    Ok(())
}

fn validate_enum(field: &'static str, value: u8, maximum: u8) -> Result<(), WireError> {
    if value == 0 || value > maximum {
        return Err(WireError::InvalidEnum { field, value });
    }

    Ok(())
}

fn put_u16(output: &mut [u8], offset: usize, value: u16) {
    output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(output: &mut [u8], offset: usize, value: u32) {
    output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut [u8], offset: usize, value: u64) {
    output[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn get_u16(input: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(
        input[offset..offset + 2]
            .try_into()
            .expect("validated payload length"),
    )
}

fn get_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        input[offset..offset + 4]
            .try_into()
            .expect("validated payload length"),
    )
}

fn get_u64(input: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        input[offset..offset + 8]
            .try_into()
            .expect("validated payload length"),
    )
}
