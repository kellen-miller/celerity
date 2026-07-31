use std::path::PathBuf;

use celerity_runtime::{
    CommandSource, ExternalAdapters, FeatureAuthority, Runtime, RuntimeEffect, RuntimeEvent,
    StartupMode, ValidatedBundle,
};

fn bundle() -> ValidatedBundle {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/examples/simulation.toml");
    ValidatedBundle::load(&source, StartupMode::Simulation).expect("valid fixture")
}

fn healthy_events() -> Vec<RuntimeEvent> {
    vec![
        RuntimeEvent::InputSnapshot {
            monotonic_ms: 0,
            coolant_c: 95.0,
            iat_c: 50.0,
        },
        RuntimeEvent::ControllerTruth {
            monotonic_ms: 1,
            boot_session: 7,
            configuration_generation: 3,
            identity_matches: true,
        },
        RuntimeEvent::Cycle { monotonic_ms: 2 },
        RuntimeEvent::CommandAcknowledged {
            monotonic_ms: 3,
            boot_session: 7,
            command_sequence: 1,
            accepted: true,
        },
        RuntimeEvent::Cycle { monotonic_ms: 4 },
    ]
}

#[test]
fn healthy_events_reach_deterministic_authority_in_fixed_order() {
    let outcome = Runtime::new(bundle(), ExternalAdapters::simulation(healthy_events()))
        .expect("matching composition")
        .run();
    assert_eq!(outcome.final_feature_authority, FeatureAuthority::Active);
    assert_eq!(outcome.command_source, CommandSource::Deterministic);
    assert_eq!(outcome.accepted_basis_points, Some(5_000));
    assert_eq!(
        outcome.effects,
        vec![
            RuntimeEffect::RenewRuntimeLease { sequence: 1 },
            RuntimeEffect::SendCommand {
                sequence: 1,
                radiator_split_basis_points: 5_000
            },
            RuntimeEffect::RecordAuthority(FeatureAuthority::Arming),
            RuntimeEffect::RecordAuthority(FeatureAuthority::Active),
            RuntimeEffect::RenewRuntimeLease { sequence: 2 },
            RuntimeEffect::SendCommand {
                sequence: 2,
                radiator_split_basis_points: 5_000
            },
        ]
    );
}

#[test]
fn repeated_logical_time_has_identical_semantics() {
    let first = Runtime::new(bundle(), ExternalAdapters::simulation(healthy_events()))
        .expect("matching composition")
        .run();
    let second = Runtime::new(bundle(), ExternalAdapters::simulation(healthy_events()))
        .expect("matching composition")
        .run();
    assert_eq!(first, second);
}

#[test]
fn stale_input_revokes_authority_and_stops_renewal() {
    let mut events = healthy_events();
    events.push(RuntimeEvent::Cycle { monotonic_ms: 50 });
    let outcome = Runtime::new(bundle(), ExternalAdapters::simulation(events))
        .expect("matching composition")
        .run();
    assert_eq!(outcome.final_feature_authority, FeatureAuthority::Fallback);
    assert_eq!(
        outcome.command_source,
        CommandSource::ControllerLocalFallback
    );
    assert!(outcome.effects.ends_with(&[
        RuntimeEffect::StopLeaseRenewal,
        RuntimeEffect::RequestFallback,
        RuntimeEffect::RecordAuthority(FeatureAuthority::Fallback),
    ]));
}

#[test]
fn controller_reboot_requires_fresh_acknowledgement() {
    let mut events = healthy_events();
    events.extend([
        RuntimeEvent::ControllerTruth {
            monotonic_ms: 5,
            boot_session: 8,
            configuration_generation: 3,
            identity_matches: true,
        },
        RuntimeEvent::Cycle { monotonic_ms: 6 },
    ]);
    let outcome = Runtime::new(bundle(), ExternalAdapters::simulation(events))
        .expect("matching composition")
        .run();
    assert_eq!(outcome.final_feature_authority, FeatureAuthority::Arming);
    assert_ne!(outcome.accepted_basis_points, Some(5_000));
}

#[test]
fn stale_command_acknowledgement_stops_lease_renewal() {
    let mut events = healthy_events();
    events.extend([
        RuntimeEvent::InputSnapshot {
            monotonic_ms: 44,
            coolant_c: 95.0,
            iat_c: 50.0,
        },
        RuntimeEvent::ControllerTruth {
            monotonic_ms: 45,
            boot_session: 7,
            configuration_generation: 3,
            identity_matches: true,
        },
        RuntimeEvent::Cycle { monotonic_ms: 46 },
    ]);
    let outcome = Runtime::new(bundle(), ExternalAdapters::simulation(events))
        .expect("matching composition")
        .run();
    assert_eq!(outcome.final_feature_authority, FeatureAuthority::Fallback);
    assert!(outcome.effects.ends_with(&[
        RuntimeEffect::StopLeaseRenewal,
        RuntimeEffect::RequestFallback,
        RuntimeEffect::RecordAuthority(FeatureAuthority::Fallback),
    ]));
}
