use super::{EncodedFrame, Frame, WireError};
use crate::codec::{node_id, put_u16, put_u32, put_u64, validate_enum, validate_split};

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
