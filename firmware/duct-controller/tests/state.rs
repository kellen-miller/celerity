use duct_controller::{ControllerState, FirmwareConfiguration, FirmwareMode};

fn configured_state() -> ControllerState {
    let mut state = ControllerState::new(7, 9_000);
    assert!(state.configure(FirmwareConfiguration {
        generation: 3,
        fallback_basis_points: 9_000,
        pwm_endpoint_a_us: 1_000,
        pwm_endpoint_b_us: 2_000,
        direction: 1,
        runtime_lease_ms: 500,
        command_lease_ms: 100,
    }));
    state
}

#[test]
fn firmware_command_lease_expires_independently() {
    let mut state = configured_state();
    assert!(state.accept_runtime_lease(0, 7, 3, 9, 1, 500));
    assert!(state.accept_command(10, 7, 3, 9, 1, 4_000));
    assert_eq!(state.mode(), FirmwareMode::RemoteAuthority);
    assert_eq!(state.pwm_microseconds(), 1_400);

    state.advance_to(111);
    assert_eq!(state.mode(), FirmwareMode::LocalFallback);
    assert!(state.runtime_lease_valid(111));
    assert_eq!(state.pwm_microseconds(), 1_900);
}

#[test]
fn duplicate_command_cannot_extend_deadline() {
    let mut state = configured_state();
    assert!(state.accept_runtime_lease(0, 7, 3, 9, 1, 500));
    assert!(state.accept_command(10, 7, 3, 9, 1, 4_000));
    assert!(!state.accept_command(90, 7, 3, 9, 1, 5_000));

    state.advance_to(111);
    assert_eq!(state.mode(), FirmwareMode::LocalFallback);
}

#[test]
fn expired_runtime_lease_allows_a_new_daemon_epoch() {
    let mut state = configured_state();
    assert!(state.accept_runtime_lease(0, 7, 3, 9, 1, 100));
    assert!(state.accept_command(10, 7, 3, 9, 1, 4_000));

    state.advance_to(100);

    assert_eq!(state.mode(), FirmwareMode::LocalFallback);
    assert_eq!(state.current_epoch(), None);
    assert!(state.accept_runtime_lease(101, 7, 3, 10, 1, 100));
}

#[test]
fn interrupted_configuration_keeps_prior_generation() {
    let mut state = configured_state();
    state.begin_configuration_write(FirmwareConfiguration {
        generation: 4,
        fallback_basis_points: 8_000,
        pwm_endpoint_a_us: 1_100,
        pwm_endpoint_b_us: 2_100,
        direction: 1,
        runtime_lease_ms: 500,
        command_lease_ms: 100,
    });
    state.recover_after_interrupted_write();

    assert_eq!(state.configuration_generation(), Some(3));
    assert_eq!(state.accepted_basis_points(), 9_000);
}
