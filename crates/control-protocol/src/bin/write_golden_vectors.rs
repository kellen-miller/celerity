use std::{env, fmt::Write as _, fs, path::PathBuf};

use control_protocol::{
    CapabilityReport, Command, CommandAck, Configuration, ConfigurationAck, DiscoveryProbe,
    FallbackAck, FallbackRequest, FaultReport, Frame, Heartbeat, NodeAnnounce, RuntimeLease,
    RuntimeLeaseAck, encode,
};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_directory = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: write-golden-vectors <output-directory>")?;
    fs::create_dir_all(&output_directory)?;

    let fixtures = [
        (
            "discovery-probe",
            Frame::DiscoveryProbe(DiscoveryProbe {
                protocol_major: 1,
                protocol_minor: 0,
                flags: 0,
                probe_sequence: 1,
            }),
        ),
        (
            "fault-report",
            Frame::FaultReport {
                node: 1,
                message: FaultReport {
                    boot_session: 1,
                    fault_sequence: 2,
                    fault_code: 3,
                    severity: 1,
                    flags: 0,
                    related_epoch: 4,
                    related_command_sequence: 5,
                },
            },
        ),
        (
            "command-minimum",
            Frame::Command {
                node: 1,
                message: Command {
                    boot_session: 1,
                    configuration_generation: 2,
                    epoch: 3,
                    command_sequence: 4,
                    radiator_split_basis_points: 0,
                    flags: 0,
                },
            },
        ),
        (
            "command-maximum",
            Frame::Command {
                node: 63,
                message: Command {
                    boot_session: u32::MAX,
                    configuration_generation: u32::MAX,
                    epoch: u64::MAX,
                    command_sequence: u32::MAX,
                    radiator_split_basis_points: 10_000,
                    flags: u8::MAX,
                },
            },
        ),
        (
            "command-ack",
            Frame::CommandAck {
                node: 1,
                message: CommandAck {
                    boot_session: 1,
                    configuration_generation: 2,
                    epoch: 3,
                    command_sequence: 4,
                    accepted_basis_points: 5_000,
                    mode: 1,
                    fault_latch: 1,
                    result: 1,
                    output_state: 1,
                    command_lease_remaining_ms: 100,
                    acknowledgement_sequence: 5,
                },
            },
        ),
        (
            "runtime-lease",
            Frame::RuntimeLease {
                node: 1,
                message: RuntimeLease {
                    boot_session: 1,
                    configuration_generation: 2,
                    epoch: 3,
                    renewal_sequence: 4,
                    validity_ms: 200,
                },
            },
        ),
        (
            "runtime-lease-ack",
            Frame::RuntimeLeaseAck {
                node: 1,
                message: RuntimeLeaseAck {
                    boot_session: 1,
                    configuration_generation: 2,
                    epoch: 3,
                    renewal_sequence: 4,
                    result: 1,
                },
            },
        ),
        (
            "heartbeat",
            Frame::Heartbeat {
                node: 1,
                message: Heartbeat {
                    boot_session: 1,
                    configuration_generation: 2,
                    capability_generation: 3,
                    heartbeat_sequence: 4,
                    current_epoch: 5,
                    last_command_sequence: 6,
                    accepted_basis_points: 5_000,
                    state_flags: 1,
                },
            },
        ),
        (
            "fallback-request",
            Frame::FallbackRequest {
                node: 1,
                message: FallbackRequest {
                    boot_session: 1,
                    epoch: 2,
                    request_sequence: 3,
                },
            },
        ),
        (
            "fallback-ack",
            Frame::FallbackAck {
                node: 1,
                message: FallbackAck {
                    boot_session: 1,
                    request_sequence: 2,
                    accepted_basis_points: 9_000,
                    mode: 1,
                    fault_latch: 1,
                    result: 1,
                },
            },
        ),
        (
            "configuration",
            Frame::Configuration {
                node: 1,
                message: Configuration {
                    boot_session: 1,
                    configuration_generation: 2,
                    fallback_basis_points: 9_000,
                    pwm_endpoint_a_us: 1_000,
                    pwm_endpoint_b_us: 2_000,
                    direction: 1,
                    flags: 0,
                    runtime_lease_ms: 200,
                    command_lease_ms: 100,
                    heartbeat_period_ms: 50,
                    acknowledgement_deadline_ms: 20,
                    normal_slew_basis_points_per_second: 500,
                    protection_slew_basis_points_per_second: 1_000,
                    digest_prefix: 3,
                },
            },
        ),
        (
            "configuration-ack",
            Frame::ConfigurationAck {
                node: 1,
                message: ConfigurationAck {
                    boot_session: 1,
                    configuration_generation: 2,
                    digest_prefix: 3,
                    result: 1,
                },
            },
        ),
        (
            "node-announce",
            Frame::NodeAnnounce {
                node: 1,
                message: NodeAnnounce {
                    protocol_major: 1,
                    protocol_minor: 0,
                    lifecycle: 1,
                    state_flags: 0,
                    boot_session: 2,
                    provisioned_identity: 3,
                    firmware_generation: 4,
                    capability_generation: 5,
                    configuration_generation: 6,
                    announce_sequence: 7,
                },
            },
        ),
        (
            "capability-report",
            Frame::CapabilityReport {
                node: 1,
                message: CapabilityReport {
                    boot_session: 1,
                    capability_generation: 2,
                    resource_id: 3,
                    minimum_basis_points: 1_000,
                    maximum_basis_points: 9_000,
                    capability_flags: 1,
                    maximum_command_rate_hz: 50,
                },
            },
        ),
    ];

    let mut vectors = Vec::with_capacity(fixtures.len());
    for (name, frame) in fixtures {
        let mut payload = [0_u8; 64];
        let encoded = encode(&frame, &mut payload)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        let mut payload_hex = String::with_capacity(encoded.len * 2);
        for byte in &payload[..encoded.len] {
            write!(payload_hex, "{byte:02x}")?;
        }
        vectors.push(json!({
            "name": name,
            "can_id": format!("0x{:03x}", encoded.can_id),
            "payload_hex": payload_hex,
        }));
    }

    let document = json!({ "schema_version": 1, "vectors": vectors });
    let mut serialized = serde_json::to_string_pretty(&document)?;
    serialized.push('\n');
    fs::write(output_directory.join("vectors.json"), serialized)?;
    Ok(())
}
