#![no_std]

pub(crate) mod codec;
mod decode;
mod encode;

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

pub use decode::decode;
pub use encode::encode;
