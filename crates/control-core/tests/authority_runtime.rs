use std::path::PathBuf;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

use control_core::{
    CommandSource, ExternalAdapters, FeatureAuthority, Runtime, RuntimeEffect, RuntimeEvent,
    RuntimeModel, StartupMode, ValidatedBundle,
};

fn bundle() -> ValidatedBundle {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/examples/simulation.toml");
    ValidatedBundle::load(&source, StartupMode::Simulation).expect("valid fixture")
}

fn model_fixture() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("model fixture directory");
    let model_path = directory.path().join("identity.onnx");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/golden/model-v1/identity.onnx"),
        &model_path,
    )
    .expect("model fixture");
    fs::write(
        directory.path().join("manifest.json"),
        br#"{
  "calibration_error": 0.01,
  "input_ranges": [
    {"minimum": 60.0, "maximum": 120.0},
    {"minimum": 0.0, "maximum": 100.0}
  ],
  "normalization": [
    {"mean": 90.0, "scale": 10.0},
    {"mean": 40.0, "scale": 10.0}
  ]
}"#,
    )
    .expect("model manifest");
    (directory, model_path)
}

fn experiment_bundle() -> ValidatedBundle {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let source = fs::read_to_string(repository.join("config/examples/simulation.toml"))
        .expect("simulation bundle");
    let path = std::env::temp_dir().join(format!(
        "celerity-experiment-bundle-{}-{}.toml",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(
        &path,
        format!(
            "{source}\n[experiment]\nplan = \"{}\"\n",
            repository
                .join("fixtures/experiments/duct-sweep.toml")
                .display()
        ),
    )
    .expect("experiment bundle");
    ValidatedBundle::load(&path, StartupMode::Simulation).expect("valid experiment bundle")
}

fn healthy_events() -> Vec<RuntimeEvent> {
    vec![
        RuntimeEvent::InputSnapshot {
            monotonic_ms: 0,
            observed_monotonic_ms: 0,
            coolant_c: 95.0,
            iat_c: 50.0,
        },
        RuntimeEvent::ModelSignalSnapshot {
            monotonic_ms: 0,
            values: vec![95.0, 50.0],
        },
        RuntimeEvent::ControllerTruth {
            monotonic_ms: 1,
            boot_session: 7,
            configuration_generation: 3,
            identity_matches: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 2,
            remaining_cycle_ns: 1_000_000_000,
        },
        RuntimeEvent::CommandAcknowledged {
            monotonic_ms: 3,
            boot_session: 7,
            command_sequence: 1,
            accepted: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 4,
            remaining_cycle_ns: 1_000_000_000,
        },
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
fn startup_selected_model_reaches_the_production_command_effect() {
    let bundle = bundle();
    let (_model_directory, model_path) = model_fixture();
    let model = RuntimeModel::load(&model_path, &bundle).expect("startup model");
    let outcome = Runtime::new_with_model(
        bundle,
        ExternalAdapters::simulation(healthy_events()),
        model,
    )
    .expect("matching composition")
    .run();

    assert_eq!(outcome.command_source, CommandSource::ModelOptimized);
    assert_eq!(outcome.accepted_basis_points, Some(9_000));
    assert!(outcome.accepted_model_command_observed);
    assert!(outcome.effects.contains(&RuntimeEffect::SendCommand {
        sequence: 1,
        radiator_split_basis_points: 9_000,
    }));
}

#[test]
fn unacknowledged_model_command_is_not_acceptance_evidence() {
    let bundle = bundle();
    let (_model_directory, model_path) = model_fixture();
    let model = RuntimeModel::load(&model_path, &bundle).expect("startup model");
    let events = healthy_events().into_iter().take(4).collect();
    let outcome = Runtime::new_with_model(bundle, ExternalAdapters::simulation(events), model)
        .expect("matching composition")
        .run();

    assert_eq!(outcome.command_source, CommandSource::ModelOptimized);
    assert!(!outcome.accepted_model_command_observed);
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
fn earlier_signal_observation_does_not_regress_ingest_time() {
    let events = vec![
        RuntimeEvent::ControllerTruth {
            monotonic_ms: 2,
            boot_session: 7,
            configuration_generation: 3,
            identity_matches: true,
        },
        RuntimeEvent::InputSnapshot {
            monotonic_ms: 3,
            observed_monotonic_ms: 0,
            coolant_c: 95.0,
            iat_c: 50.0,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 4,
            remaining_cycle_ns: 1_000_000_000,
        },
    ];
    let outcome = Runtime::new(bundle(), ExternalAdapters::simulation(events))
        .expect("matching composition")
        .run();

    assert!(!outcome.hard_fault_latched);
    assert_eq!(outcome.final_feature_authority, FeatureAuthority::Arming);
}

#[test]
fn stale_input_revokes_authority_and_stops_renewal() {
    let mut events = healthy_events();
    events.push(RuntimeEvent::Cycle {
        monotonic_ms: 300,
        remaining_cycle_ns: 20_000_000,
    });
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
        RuntimeEvent::Cycle {
            monotonic_ms: 6,
            remaining_cycle_ns: 20_000_000,
        },
    ]);
    let outcome = Runtime::new(bundle(), ExternalAdapters::simulation(events))
        .expect("matching composition")
        .run();
    assert_eq!(outcome.final_feature_authority, FeatureAuthority::Arming);
    assert_ne!(outcome.accepted_basis_points, Some(5_000));
}

#[test]
fn hard_fault_latch_clears_after_reconciled_controller_reboot() {
    let mut events = healthy_events();
    events.extend([
        RuntimeEvent::SharedHardFault { monotonic_ms: 5 },
        RuntimeEvent::ControllerUnavailable { monotonic_ms: 6 },
        RuntimeEvent::ControllerTruth {
            monotonic_ms: 7,
            boot_session: 8,
            configuration_generation: 3,
            identity_matches: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 8,
            remaining_cycle_ns: 20_000_000,
        },
    ]);
    let outcome = Runtime::new(bundle(), ExternalAdapters::simulation(events))
        .expect("matching composition")
        .run();

    assert!(!outcome.hard_fault_latched);
    assert_eq!(outcome.final_feature_authority, FeatureAuthority::Arming);
}

#[test]
fn stale_command_acknowledgement_stops_lease_renewal() {
    let mut events = healthy_events();
    events.extend([
        RuntimeEvent::InputSnapshot {
            monotonic_ms: 44,
            observed_monotonic_ms: 44,
            coolant_c: 95.0,
            iat_c: 50.0,
        },
        RuntimeEvent::ControllerTruth {
            monotonic_ms: 45,
            boot_session: 7,
            configuration_generation: 3,
            identity_matches: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 46,
            remaining_cycle_ns: 20_000_000,
        },
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

#[test]
fn accepted_command_acknowledgement_refreshes_controller_truth() {
    let mut events = healthy_events();
    events.extend([
        RuntimeEvent::InputSnapshot {
            monotonic_ms: 42,
            observed_monotonic_ms: 42,
            coolant_c: 95.0,
            iat_c: 50.0,
        },
        RuntimeEvent::CommandAcknowledged {
            monotonic_ms: 43,
            boot_session: 7,
            command_sequence: 2,
            accepted: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 44,
            remaining_cycle_ns: 20_000_000,
        },
    ]);
    let outcome = Runtime::new(bundle(), ExternalAdapters::simulation(events))
        .expect("matching composition")
        .run();

    assert_eq!(outcome.final_feature_authority, FeatureAuthority::Active);
    assert_eq!(outcome.accepted_basis_points, Some(5_000));
}

#[test]
fn experiment_waits_for_authority_and_rejection_aborts_once() {
    let events = vec![
        RuntimeEvent::InputSnapshot {
            monotonic_ms: 0,
            observed_monotonic_ms: 0,
            coolant_c: 95.0,
            iat_c: 50.0,
        },
        RuntimeEvent::ControllerTruth {
            monotonic_ms: 0,
            boot_session: 7,
            configuration_generation: 3,
            identity_matches: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 1,
            remaining_cycle_ns: 1_000_000_000,
        },
        RuntimeEvent::CommandAcknowledged {
            monotonic_ms: 2,
            boot_session: 7,
            command_sequence: 1,
            accepted: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 3,
            remaining_cycle_ns: 1_000_000_000,
        },
        RuntimeEvent::CommandAcknowledged {
            monotonic_ms: 4,
            boot_session: 7,
            command_sequence: 2,
            accepted: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 5,
            remaining_cycle_ns: 1_000_000_000,
        },
        RuntimeEvent::CommandAcknowledged {
            monotonic_ms: 6,
            boot_session: 7,
            command_sequence: 3,
            accepted: false,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 7,
            remaining_cycle_ns: 1_000_000_000,
        },
        RuntimeEvent::CommandAcknowledged {
            monotonic_ms: 8,
            boot_session: 7,
            command_sequence: 4,
            accepted: true,
        },
        RuntimeEvent::Cycle {
            monotonic_ms: 9,
            remaining_cycle_ns: 1_000_000_000,
        },
    ];
    let outcome = Runtime::new(experiment_bundle(), ExternalAdapters::simulation(events))
        .expect("matching composition")
        .run();
    let experiment_evidence = outcome
        .effects
        .iter()
        .filter_map(|effect| match effect {
            RuntimeEffect::RecordExperiment(event) => Some(event.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        experiment_evidence,
        [
            r#"{"event":"start"}"#,
            r#"{"event":"step","radiator_split_basis_points":1000}"#,
            r#"{"event":"abort","reason":"AuthorityLost"}"#,
        ]
    );
    assert_eq!(outcome.command_source, CommandSource::Deterministic);
    assert_eq!(outcome.final_feature_authority, FeatureAuthority::Active);
}
