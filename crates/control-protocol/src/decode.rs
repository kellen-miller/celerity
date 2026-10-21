use super::{
    CapabilityReport, Command, CommandAck, Configuration, ConfigurationAck, DiscoveryProbe,
    FallbackAck, FallbackRequest, FaultReport, Frame, Heartbeat, NodeAnnounce, RuntimeLease,
    RuntimeLeaseAck, WireError,
};
use crate::codec::{
    exact_length, frame_family, get_u16, get_u32, get_u64, reserved_zero, validate_enum,
    validate_split,
};

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
