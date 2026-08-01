use celerity_protocol::{Command, Configuration, Frame, RuntimeLease, decode, encode};
use celerity_runtime::{ControllerEmulator, ControllerMode, EmulatorProvisioning};
use duct_controller::{ControllerState, FirmwareConfiguration, FirmwareMode};

fn ingest(emulator: &mut ControllerEmulator, now_ms: u64, frame: &Frame) -> Frame {
    let mut payload = [0_u8; 64];
    let encoded = encode(frame, &mut payload).expect("fixture must encode");
    let response = emulator
        .ingest(now_ms, encoded.can_id, &payload[..encoded.len])
        .expect("fixture must decode")
        .expect("fixture must produce a response");
    decode(response.can_id, response.payload()).expect("response must decode")
}

#[test]
fn firmware_state_and_emulator_keep_the_same_authority_semantics() {
    let mut firmware = ControllerState::new(7, 9_000);
    let mut emulator = ControllerEmulator::new(EmulatorProvisioning {
        node: 1,
        identity: 42,
        boot_session: 7,
        firmware_generation: 1,
        capability_generation: 1,
        fallback_basis_points: 9_000,
    });
    let configuration = Configuration {
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
    };
    assert!(firmware.configure(FirmwareConfiguration {
        generation: configuration.configuration_generation,
        fallback_basis_points: configuration.fallback_basis_points,
        pwm_endpoint_a_us: configuration.pwm_endpoint_a_us,
        pwm_endpoint_b_us: configuration.pwm_endpoint_b_us,
        direction: configuration.direction,
        runtime_lease_ms: configuration.runtime_lease_ms,
        command_lease_ms: configuration.command_lease_ms,
        heartbeat_period_ms: configuration.heartbeat_period_ms,
    }));
    let Frame::ConfigurationAck { message, .. } = ingest(
        &mut emulator,
        0,
        &Frame::Configuration {
            node: 1,
            message: configuration,
        },
    ) else {
        panic!("expected configuration acknowledgement")
    };
    assert_eq!(message.result, 1);

    let lease = RuntimeLease {
        boot_session: 7,
        configuration_generation: 3,
        epoch: 9,
        renewal_sequence: 1,
        validity_ms: 500,
    };
    assert!(firmware.accept_runtime_lease(
        0,
        lease.boot_session,
        lease.configuration_generation,
        lease.epoch,
        lease.renewal_sequence,
        lease.validity_ms,
    ));
    let Frame::RuntimeLeaseAck { message, .. } = ingest(
        &mut emulator,
        0,
        &Frame::RuntimeLease {
            node: 1,
            message: lease,
        },
    ) else {
        panic!("expected lease acknowledgement")
    };
    assert_eq!(message.result, 1);

    let command = Command {
        boot_session: 7,
        configuration_generation: 3,
        epoch: 9,
        command_sequence: 1,
        radiator_split_basis_points: 4_000,
        flags: 0,
    };
    assert!(firmware.accept_command(
        10,
        command.boot_session,
        command.configuration_generation,
        command.epoch,
        command.command_sequence,
        command.radiator_split_basis_points,
    ));
    let Frame::CommandAck { message, .. } = ingest(
        &mut emulator,
        10,
        &Frame::Command {
            node: 1,
            message: command,
        },
    ) else {
        panic!("expected command acknowledgement")
    };
    assert_eq!(message.result, 1);
    assert_eq!(firmware.mode(), FirmwareMode::RemoteAuthority);
    assert_eq!(emulator.mode(), ControllerMode::RemoteAuthority);
    assert_eq!(
        firmware.accepted_basis_points(),
        emulator.accepted_basis_points()
    );

    firmware.advance_to(111);
    emulator.advance_to(111);
    assert_eq!(firmware.mode(), FirmwareMode::LocalFallback);
    assert_eq!(emulator.mode(), ControllerMode::LocalFallback);
    assert_eq!(
        firmware.accepted_basis_points(),
        emulator.accepted_basis_points()
    );
    assert_eq!(
        firmware.runtime_lease_valid(111),
        emulator.runtime_lease_valid(111)
    );
}
