use control_protocol::{
    Command, CommandAck, Configuration, ConfigurationAck, DiscoveryProbe, FallbackAck,
    FallbackRequest, FaultReport, Frame, Heartbeat, NodeAnnounce, RuntimeLease, RuntimeLeaseAck,
    WireError, decode, encode,
};

fn encoded(frame: &Frame) -> (u16, Vec<u8>) {
    let mut bytes = [0_u8; 64];
    let encoded = encode(frame, &mut bytes).expect("fixture must encode");
    (encoded.can_id, bytes[..encoded.len].to_vec())
}

#[test]
fn every_v1_frame_has_exact_id_length_and_round_trip() {
    let frames = [
        Frame::DiscoveryProbe(DiscoveryProbe {
            protocol_major: 1,
            protocol_minor: 0,
            flags: 0x0201,
            probe_sequence: 0x0605_0403,
        }),
        Frame::Command {
            node: 1,
            message: Command {
                boot_session: 1,
                configuration_generation: 2,
                epoch: 3,
                command_sequence: 4,
                radiator_split_basis_points: 5_000,
                flags: 1,
            },
        },
        Frame::CommandAck {
            node: 2,
            message: CommandAck {
                boot_session: 1,
                configuration_generation: 2,
                epoch: 3,
                command_sequence: 4,
                accepted_basis_points: 5_000,
                mode: 1,
                fault_latch: 2,
                result: 1,
                output_state: 1,
                command_lease_remaining_ms: 50,
                acknowledgement_sequence: 5,
            },
        },
        Frame::RuntimeLease {
            node: 3,
            message: RuntimeLease {
                boot_session: 1,
                configuration_generation: 2,
                epoch: 3,
                renewal_sequence: 4,
                validity_ms: 100,
            },
        },
        Frame::RuntimeLeaseAck {
            node: 4,
            message: RuntimeLeaseAck {
                boot_session: 1,
                configuration_generation: 2,
                epoch: 3,
                renewal_sequence: 4,
                result: 1,
            },
        },
        Frame::Heartbeat {
            node: 5,
            message: Heartbeat {
                boot_session: 1,
                configuration_generation: 2,
                capability_generation: 3,
                heartbeat_sequence: 4,
                current_epoch: 5,
                last_command_sequence: 6,
                accepted_basis_points: 7_500,
                state_flags: 7,
            },
        },
        Frame::FallbackRequest {
            node: 6,
            message: FallbackRequest {
                boot_session: 1,
                epoch: 2,
                request_sequence: 3,
            },
        },
        Frame::FallbackAck {
            node: 7,
            message: FallbackAck {
                boot_session: 1,
                request_sequence: 2,
                accepted_basis_points: 8_000,
                mode: 1,
                fault_latch: 1,
                result: 1,
            },
        },
        Frame::Configuration {
            node: 8,
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
        Frame::ConfigurationAck {
            node: 9,
            message: ConfigurationAck {
                boot_session: 1,
                configuration_generation: 2,
                digest_prefix: 3,
                result: 1,
            },
        },
        Frame::NodeAnnounce {
            node: 10,
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
        Frame::CapabilityReport {
            node: 11,
            message: control_protocol::CapabilityReport {
                boot_session: 1,
                capability_generation: 2,
                resource_id: 3,
                minimum_basis_points: 1_000,
                maximum_basis_points: 9_000,
                capability_flags: 1,
                maximum_command_rate_hz: 50,
            },
        },
        Frame::FaultReport {
            node: 63,
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
    ];

    let expected = [
        (0x080, 8),
        (0x101, 24),
        (0x142, 32),
        (0x183, 24),
        (0x1c4, 24),
        (0x205, 32),
        (0x246, 16),
        (0x287, 16),
        (0x2c8, 32),
        (0x309, 16),
        (0x34a, 32),
        (0x38b, 24),
        (0x07f, 24),
    ];

    for (frame, (can_id, length)) in frames.iter().zip(expected) {
        let (actual_id, bytes) = encoded(frame);
        assert_eq!((actual_id, bytes.len()), (can_id, length));
        assert_eq!(decode(actual_id, &bytes), Ok(frame.clone()));
    }
}

#[test]
fn command_wire_bytes_are_little_endian_and_reserved_zero() {
    let frame = Frame::Command {
        node: 1,
        message: Command {
            boot_session: 0x0403_0201,
            configuration_generation: 0x0807_0605,
            epoch: 0x100f_0e0d_0c0b_0a09,
            command_sequence: 0x1413_1211,
            radiator_split_basis_points: 10_000,
            flags: 0x15,
        },
    };
    let (can_id, bytes) = encoded(&frame);
    assert_eq!(can_id, 0x101);
    assert_eq!(
        bytes,
        [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 0x10, 0x27,
            0x15, 0
        ]
    );
}

#[test]
fn invalid_wire_values_are_rejected() {
    let command = Frame::Command {
        node: 1,
        message: Command {
            boot_session: 1,
            configuration_generation: 1,
            epoch: 1,
            command_sequence: 1,
            radiator_split_basis_points: 5_000,
            flags: 0,
        },
    };
    let (_, mut bytes) = encoded(&command);
    assert_eq!(
        decode(0x101, &bytes[..23]),
        Err(WireError::WrongLength {
            expected: 24,
            actual: 23
        })
    );
    bytes[23] = 1;
    assert_eq!(decode(0x101, &bytes), Err(WireError::ReservedNotZero));
    bytes[23] = 0;
    bytes[20..22].copy_from_slice(&10_001_u16.to_le_bytes());
    assert_eq!(
        decode(0x101, &bytes),
        Err(WireError::SplitOutOfRange(10_001))
    );
    assert_eq!(decode(0x100, &bytes), Err(WireError::UnknownId(0x100)));
    assert_eq!(decode(0x7ff, &[]), Err(WireError::UnknownId(0x7ff)));
}

#[test]
fn zero_enum_values_are_rejected() {
    let frame = Frame::ConfigurationAck {
        node: 1,
        message: ConfigurationAck {
            boot_session: 1,
            configuration_generation: 1,
            digest_prefix: 1,
            result: 1,
        },
    };
    let (_, mut bytes) = encoded(&frame);
    bytes[12] = 0;
    assert_eq!(
        decode(0x301, &bytes),
        Err(WireError::InvalidEnum {
            field: "result",
            value: 0
        })
    );
}
