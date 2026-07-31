use std::{fs, path::PathBuf};

use celerity_runtime::{BundleError, StartupMode, ValidatedBundle};

fn fixture() -> String {
    fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/examples/simulation.toml"),
    )
    .expect("simulation fixture")
}

fn write_bundle(contents: &str, name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("celerity-config-{}", std::process::id()));
    fs::create_dir_all(&root).expect("temporary directory");
    let path = root.join(name);
    fs::write(&path, contents).expect("temporary bundle");
    path
}

#[test]
fn one_loader_accepts_complete_matching_bundle() {
    let bundle = ValidatedBundle::load(
        &write_bundle(&fixture(), "valid.toml"),
        StartupMode::Simulation,
    )
    .expect("valid simulation bundle");
    assert_eq!(bundle.generation(), 1);
    assert_eq!(bundle.mode(), StartupMode::Simulation);
    assert_eq!(bundle.controller_count(), 1);
}

#[test]
fn unknown_keys_and_mode_mismatch_are_denied() {
    let unknown = fixture().replace("cycle_ms = 20", "cycle_ms = 20\nsecret_mode = true");
    assert!(matches!(
        ValidatedBundle::load(
            &write_bundle(&unknown, "unknown.toml"),
            StartupMode::Simulation
        ),
        Err(BundleError::Parse(_))
    ));
    assert_eq!(
        ValidatedBundle::load(&write_bundle(&fixture(), "mode.toml"), StartupMode::Replay,),
        Err(BundleError::RequestedModeMismatch {
            requested: StartupMode::Replay,
            configured: StartupMode::Simulation,
        })
    );
}

#[test]
fn nonmonotonic_policy_and_duplicate_identity_are_denied() {
    let nonmonotonic = fixture().replace(
        "[[1000, 2000, 3000], [4000, 5000, 6000], [7000, 8000, 9000]]",
        "[[1000, 2000, 3000], [4000, 3000, 6000], [7000, 8000, 9000]]",
    );
    assert_eq!(
        ValidatedBundle::load(
            &write_bundle(&nonmonotonic, "policy.toml"),
            StartupMode::Simulation,
        ),
        Err(BundleError::NonmonotonicPolicy)
    );

    let duplicate = fixture().replace(
        "[duct]\n",
        "[controllers.second]\n\
         address = 2\n\
         identity = 1\n\
         configuration_generation = 1\n\
         capability_generation = 1\n\
         resource_id = 2\n\
         minimum_basis_points = 1000\n\
         maximum_basis_points = 9000\n\
         maximum_command_rate_hz = 50\n\
         fallback_basis_points = 9000\n\
         pwm_endpoint_a_us = 1000\n\
         pwm_endpoint_b_us = 2000\n\
         direction = 1\n\
         runtime_lease_ms = 100\n\
         command_lease_ms = 50\n\
         heartbeat_period_ms = 20\n\
         acknowledgement_deadline_ms = 20\n\
         normal_slew_basis_points_per_second = 1000\n\
         protection_slew_basis_points_per_second = 2000\n\
         digest_prefix = 2\n\n\
         [duct]\n",
    );
    assert_eq!(
        ValidatedBundle::load(
            &write_bundle(&duplicate, "duplicate.toml"),
            StartupMode::Simulation,
        ),
        Err(BundleError::DuplicateControllerIdentity(1))
    );
}

#[test]
fn composition_tag_must_match_mode() {
    let mismatched = fixture().replace("kind = \"simulation\"", "kind = \"replay\"");
    assert!(matches!(
        ValidatedBundle::load(
            &write_bundle(&mismatched, "composition.toml"),
            StartupMode::Simulation,
        ),
        Err(BundleError::Parse(_) | BundleError::CompositionModeMismatch)
    ));
}

#[test]
fn model_commands_must_fit_the_controller_envelope() {
    let outside_controller = fixture().replace(
        "command_lattice = [1000, 5000, 9000]",
        "command_lattice = [0, 5000, 9000]",
    );
    assert_eq!(
        ValidatedBundle::load(
            &write_bundle(&outside_controller, "model-envelope.toml"),
            StartupMode::Simulation,
        ),
        Err(BundleError::InvalidRuntimeTiming)
    );
}
