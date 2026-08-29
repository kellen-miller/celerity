use control_core::{ControllerEmulator, ControllerMode, EmulatorProvisioning};
use control_protocol::{
    Command, Configuration, DiscoveryProbe, Frame, RuntimeLease, decode, encode,
};

fn ingest(emulator: &mut ControllerEmulator, now_ms: u64, frame: &Frame) -> Frame {
    let mut payload = [0_u8; 64];
    let encoded = encode(frame, &mut payload).expect("fixture must encode");
    let response = emulator
        .ingest(now_ms, encoded.can_id, &payload[..encoded.len])
        .expect("fixture must be valid")
        .expect("fixture must produce a response");
    decode(response.can_id, response.payload()).expect("response must use production codec")
}

fn configured_emulator() -> ControllerEmulator {
    let mut emulator = ControllerEmulator::new(EmulatorProvisioning {
        node: 1,
        identity: 42,
        boot_session: 7,
        firmware_generation: 1,
        capability_generation: 1,
        fallback_basis_points: 9_000,
    });
    let response = ingest(
        &mut emulator,
        0,
        &Frame::Configuration {
            node: 1,
            message: Configuration {
                boot_session: 7,
                configuration_generation: 3,
                fallback_basis_points: 9_000,
                pwm_endpoint_a_us: 1_000,
                pwm_endpoint_b_us: 2_000,
                direction: 1,
                flags: 0,
                runtime_lease_ms: 500,
                command_lease_ms: 100,
                heartbeat_period_ms: 50,
                acknowledgement_deadline_ms: 20,
                normal_slew_basis_points_per_second: 500,
                protection_slew_basis_points_per_second: 1_000,
                digest_prefix: 4,
            },
        },
    );
    assert!(matches!(response, Frame::ConfigurationAck { .. }));
    emulator
}

#[test]
fn discovery_emits_announce_then_capability() {
    let mut emulator = configured_emulator();
    let mut payload = [0_u8; 64];
    let encoded = encode(
        &Frame::DiscoveryProbe(DiscoveryProbe {
            protocol_major: 1,
            protocol_minor: 0,
            flags: 0,
            probe_sequence: 4,
        }),
        &mut payload,
    )
    .expect("probe must encode");
    let announce = emulator
        .ingest(0, encoded.can_id, &payload[..encoded.len])
        .expect("probe must be valid")
        .expect("announce must be emitted");
    let Frame::NodeAnnounce { message, .. } =
        decode(announce.can_id, announce.payload()).expect("announce must decode")
    else {
        panic!("expected node announce")
    };
    assert_eq!(message.state_flags, 1);
    let capability = announce
        .follow_up()
        .expect("capability must follow announce");
    assert!(matches!(
        decode(capability.can_id, capability.payload()).expect("capability must decode"),
        Frame::CapabilityReport { .. }
    ));
}

#[test]
fn heartbeat_uses_the_configured_period_and_current_truth() {
    let mut emulator = configured_emulator();
    assert!(emulator.heartbeat(49).expect("heartbeat timer").is_none());

    let heartbeat = emulator
        .heartbeat(50)
        .expect("heartbeat timer")
        .expect("configured deadline must emit");
    let Frame::Heartbeat { message, .. } =
        decode(heartbeat.can_id, heartbeat.payload()).expect("heartbeat must decode")
    else {
        panic!("expected heartbeat")
    };
    assert_eq!(message.heartbeat_sequence, 1);
    assert_eq!(message.current_epoch, 0);
    assert_eq!(message.state_flags, 1);
    assert!(emulator.heartbeat(99).expect("heartbeat timer").is_none());
    assert!(emulator.heartbeat(100).expect("heartbeat timer").is_some());
}

#[test]
fn accepted_command_refreshes_only_command_lease() {
    let mut emulator = configured_emulator();
    ingest(
        &mut emulator,
        0,
        &Frame::RuntimeLease {
            node: 1,
            message: RuntimeLease {
                boot_session: 7,
                configuration_generation: 3,
                epoch: 9,
                renewal_sequence: 1,
                validity_ms: 500,
            },
        },
    );
    let response = ingest(
        &mut emulator,
        10,
        &Frame::Command {
            node: 1,
            message: Command {
                boot_session: 7,
                configuration_generation: 3,
                epoch: 9,
                command_sequence: 1,
                radiator_split_basis_points: 4_000,
                flags: 0,
            },
        },
    );
    assert!(matches!(response, Frame::CommandAck { .. }));
    assert_eq!(emulator.mode(), ControllerMode::RemoteAuthority);
    assert_eq!(emulator.accepted_basis_points(), 4_000);

    emulator.advance_to(111);
    assert_eq!(emulator.mode(), ControllerMode::LocalFallback);
    assert_eq!(emulator.accepted_basis_points(), 9_000);
    assert!(emulator.runtime_lease_valid(111));
}

#[test]
fn rejected_command_does_not_refresh_command_lease() {
    let mut emulator = configured_emulator();
    ingest(
        &mut emulator,
        0,
        &Frame::RuntimeLease {
            node: 1,
            message: RuntimeLease {
                boot_session: 7,
                configuration_generation: 3,
                epoch: 9,
                renewal_sequence: 1,
                validity_ms: 500,
            },
        },
    );
    ingest(
        &mut emulator,
        10,
        &Frame::Command {
            node: 1,
            message: Command {
                boot_session: 7,
                configuration_generation: 3,
                epoch: 9,
                command_sequence: 1,
                radiator_split_basis_points: 4_000,
                flags: 0,
            },
        },
    );
    let rejected = ingest(
        &mut emulator,
        90,
        &Frame::Command {
            node: 1,
            message: Command {
                boot_session: 7,
                configuration_generation: 3,
                epoch: 9,
                command_sequence: 1,
                radiator_split_basis_points: 5_000,
                flags: 0,
            },
        },
    );
    let Frame::CommandAck { message, .. } = rejected else {
        panic!("expected command acknowledgement")
    };
    assert_eq!(message.result, 2);

    emulator.advance_to(111);
    assert_eq!(emulator.mode(), ControllerMode::LocalFallback);
}

#[test]
fn reboot_never_restores_authority() {
    let mut emulator = configured_emulator();
    ingest(
        &mut emulator,
        0,
        &Frame::RuntimeLease {
            node: 1,
            message: RuntimeLease {
                boot_session: 7,
                configuration_generation: 3,
                epoch: 9,
                renewal_sequence: 1,
                validity_ms: 500,
            },
        },
    );
    ingest(
        &mut emulator,
        1,
        &Frame::Command {
            node: 1,
            message: Command {
                boot_session: 7,
                configuration_generation: 3,
                epoch: 9,
                command_sequence: 1,
                radiator_split_basis_points: 4_000,
                flags: 0,
            },
        },
    );

    emulator.reboot(8);
    assert_eq!(emulator.boot_session(), 8);
    assert_eq!(emulator.mode(), ControllerMode::LocalFallback);
    assert!(!emulator.runtime_lease_valid(1));
}

#[test]
fn expired_runtime_lease_allows_a_new_daemon_epoch() {
    let mut emulator = configured_emulator();
    ingest(
        &mut emulator,
        0,
        &Frame::RuntimeLease {
            node: 1,
            message: RuntimeLease {
                boot_session: 7,
                configuration_generation: 3,
                epoch: 9,
                renewal_sequence: 1,
                validity_ms: 100,
            },
        },
    );

    emulator.advance_to(100);

    let Frame::RuntimeLeaseAck { message, .. } = ingest(
        &mut emulator,
        101,
        &Frame::RuntimeLease {
            node: 1,
            message: RuntimeLease {
                boot_session: 7,
                configuration_generation: 3,
                epoch: 10,
                renewal_sequence: 1,
                validity_ms: 100,
            },
        },
    ) else {
        panic!("expected runtime lease acknowledgement")
    };
    assert_eq!(message.result, 1);
}
