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
        "[controllers.second]\naddress = 2\nidentity = 1\n\n[duct]\n",
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
