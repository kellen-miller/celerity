use std::{
    fmt::Write as _,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use celerity_runtime::{ModelSlot, ModelSlots, SlotError};
use sha2::{Digest, Sha256};

fn fixture() -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../contracts/golden/model-v1/identity.onnx"),
    )
    .expect("fixture")
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("String write");
            output
        })
}

fn root() -> PathBuf {
    static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "celerity-slots-{}-{}",
        std::process::id(),
        NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn absence_is_deterministic_control_not_startup_failure() {
    let slots = ModelSlots::open(&root()).expect("slots");
    assert_eq!(slots.select_startup(None, None, "thermal-v1", 3), Ok(None));
}

#[test]
fn inactive_stage_activates_only_at_later_startup() {
    let root = root();
    let slots = ModelSlots::open(&root).expect("slots");
    let bytes = fixture();
    let model_digest = digest(&bytes);
    assert_eq!(
        slots.stage_inactive(ModelSlot::A, &bytes, &model_digest, "thermal-v1"),
        Ok(ModelSlot::B)
    );
    let selected = slots
        .select_startup(Some(&model_digest), None, "thermal-v1", 3)
        .expect("selection")
        .expect("model");
    assert_eq!(selected.slot, ModelSlot::B);
    assert_eq!(selected.digest, model_digest);
}

#[test]
fn corrupt_desired_falls_back_to_known_good() {
    let root = root();
    let slots = ModelSlots::open(&root).expect("slots");
    let good = fixture();
    let good_digest = digest(&good);
    slots
        .stage_inactive(ModelSlot::A, &good, &good_digest, "thermal-v1")
        .expect("stage B");
    slots
        .stage_inactive(ModelSlot::B, &good, &good_digest, "thermal-v1")
        .expect("stage A");
    fs::write(root.join("slot-b/model.onnx"), b"corrupt").expect("corrupt desired");
    let selected = slots
        .select_startup(Some(&good_digest), Some(&good_digest), "thermal-v1", 3)
        .expect("known-good selection")
        .expect("known good");
    assert_eq!(selected.slot, ModelSlot::A);
}

#[test]
fn interrupted_staging_never_becomes_selectable() {
    let root = root();
    let slots = ModelSlots::open(&root).expect("slots");
    fs::write(root.join("slot-b/model.onnx.staging"), fixture()).expect("interrupted stage");
    assert_eq!(
        slots.select_startup(Some("missing"), None, "thermal-v1", 3),
        Err(SlotError::NoValidSlot)
    );
}

#[test]
fn invalid_known_good_is_rejected_and_optional_startup_continues() {
    let root = root();
    let slots = ModelSlots::open(&root).expect("slots");
    let bytes = fixture();
    let model_digest = digest(&bytes);
    slots
        .stage_inactive(ModelSlot::A, &bytes, &model_digest, "thermal-v1")
        .expect("stage known-good");
    fs::write(root.join("known-good-digest"), format!("{model_digest}\n"))
        .expect("known-good marker");
    fs::write(root.join("slot-b/model.onnx"), b"corrupt").expect("corrupt graph");

    assert_eq!(slots.select_configured_startup("thermal-v1", 3), Ok(None));
    assert_eq!(
        fs::read_to_string(root.join("rejected-digests")).expect("rejection marker"),
        format!("{model_digest}\n")
    );
}

#[test]
fn malformed_desired_manifest_is_rejected_and_known_good_recovers() {
    let root = root();
    let slots = ModelSlots::open(&root).expect("slots");
    let bytes = fixture();
    let good_digest = digest(&bytes);
    slots
        .stage_inactive(ModelSlot::A, &bytes, &good_digest, "thermal-v1")
        .expect("stage known-good in B");
    fs::write(root.join("known-good-digest"), format!("{good_digest}\n"))
        .expect("known-good marker");

    let bad_digest = "b".repeat(64);
    let bad_slot = root.join("slot-a");
    fs::write(bad_slot.join("model.onnx"), &bytes).expect("bad desired graph");
    fs::write(bad_slot.join("manifest.json"), b"not-json").expect("bad desired manifest");
    fs::write(bad_slot.join("bundle-digest"), format!("{bad_digest}\n"))
        .expect("bad desired bundle marker");
    fs::write(root.join("desired-digest"), format!("{bad_digest}\n")).expect("desired marker");

    let selected = slots
        .select_configured_startup("thermal-v1", 3)
        .expect("recover selection")
        .expect("known-good model");
    assert_eq!(selected.digest, good_digest);
    assert_eq!(
        fs::read_to_string(root.join("rejected-digests")).expect("rejection marker"),
        format!("{bad_digest}\n")
    );
}
