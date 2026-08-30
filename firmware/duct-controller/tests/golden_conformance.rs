use control_protocol::{Frame, encode};
use duct_controller::{ControllerState, FirmwareConfiguration, FirmwareMode};
use serde::Deserialize;

#[derive(Deserialize)]
struct GoldenFile {
    schema_version: u32,
    vectors: Vec<GoldenVector>,
}

#[derive(Deserialize)]
struct GoldenVector {
    can_id: String,
    payload_hex: String,
}

#[test]
fn firmware_native_state_consumes_shared_raw_vectors() {
    let golden: GoldenFile = serde_json::from_str(include_str!(
        "../../../contracts/golden/controller-can-v1/vectors.json"
    ))
    .expect("shared golden vectors");
    assert_eq!(golden.schema_version, 1);
    assert_eq!(golden.vectors.len(), 14);

    let decoded = golden
        .vectors
        .iter()
        .map(|vector| {
            let can_id = u16::from_str_radix(vector.can_id.trim_start_matches("0x"), 16)
                .expect("golden CAN ID");
            let payload = hex::decode(&vector.payload_hex).expect("golden payload");
            control_protocol::decode(can_id, &payload).expect("firmware protocol decode")
        })
        .collect::<Vec<_>>();

    for (frame, vector) in decoded.iter().zip(&golden.vectors) {
        let mut payload = [0_u8; 64];
        let encoded = encode(frame, &mut payload).expect("firmware protocol encode");
        assert_eq!(
            encoded.can_id,
            u16::from_str_radix(vector.can_id.trim_start_matches("0x"), 16).expect("golden CAN ID")
        );
        assert_eq!(hex::encode(&payload[..encoded.len]), vector.payload_hex);
    }

    let configuration = decoded.iter().find_map(|frame| match frame {
        Frame::Configuration { message, .. } => Some(message),
        _ => None,
    });
    let lease = decoded.iter().find_map(|frame| match frame {
        Frame::RuntimeLease { message, .. } => Some(message),
        _ => None,
    });
    let command = decoded.iter().find_map(|frame| match frame {
        Frame::Command { message, .. } if message.command_sequence == 4 => Some(message),
        _ => None,
    });

    let mut state = ControllerState::new(1, 9_000);
    let configuration = configuration.expect("configuration vector");
    assert!(state.configure(FirmwareConfiguration {
        generation: configuration.configuration_generation,
        fallback_basis_points: configuration.fallback_basis_points,
        pwm_endpoint_a_us: configuration.pwm_endpoint_a_us,
        pwm_endpoint_b_us: configuration.pwm_endpoint_b_us,
        direction: configuration.direction,
        runtime_lease_ms: configuration.runtime_lease_ms,
        command_lease_ms: configuration.command_lease_ms,
        heartbeat_period_ms: configuration.heartbeat_period_ms,
    }));
    let lease = lease.expect("Runtime Lease vector");
    assert!(state.accept_runtime_lease(
        0,
        lease.boot_session,
        lease.configuration_generation,
        lease.epoch,
        lease.renewal_sequence,
        lease.validity_ms,
    ));
    let command = command.expect("Command vector");
    assert!(state.accept_command(
        10,
        command.boot_session,
        command.configuration_generation,
        command.epoch,
        command.command_sequence,
        command.radiator_split_basis_points,
    ));
    assert_eq!(state.mode(), FirmwareMode::RemoteAuthority);
    assert_eq!(state.accepted_basis_points(), 0);
    state.advance_to(111);
    assert_eq!(state.mode(), FirmwareMode::LocalFallback);
    assert!(state.runtime_lease_valid(111));
}
