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
    assert_eq!(slots.select_startup(None, None, "thermal-v1"), Ok(None));
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
        .select_startup(Some(&model_digest), None, "thermal-v1")
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
        .select_startup(Some(&good_digest), Some(&good_digest), "thermal-v1")
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
        slots.select_startup(Some("missing"), None, "thermal-v1"),
        Err(SlotError::NoValidSlot)
    );
}
