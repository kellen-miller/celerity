use std::{fmt::Write as _, fs, path::PathBuf};

use control_core::{Completion, EnqueueResult, RunRecord, RunWriter, recover_incomplete_runs};
use prost::Message;
use sha2::{Digest, Sha256};

fn root(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("celerity-{name}-{}", std::process::id()));
    if path.exists() {
        fs::remove_dir_all(&path).expect("remove prior fixture");
    }
    fs::create_dir_all(&path).expect("fixture root");
    path
}

#[test]
fn sealed_run_has_exact_byte_checksum_and_monotonic_manifest() {
    let root = root("sealed-run");
    let writer = RunWriter::start(&root, "run-1", 8, 0).expect("start writer");
    assert_eq!(
        writer.enqueue(RunRecord::lifecycle(1, 10, "starting")),
        EnqueueResult::Accepted
    );
    assert_eq!(
        writer.enqueue(RunRecord::lifecycle(2, 20, "running")),
        EnqueueResult::Accepted
    );
    let manifest = writer.seal().expect("seal run");
    assert_eq!(manifest.completion, Completion::Complete);
    assert_eq!(
        (manifest.first_sequence, manifest.last_sequence),
        (Some(1), Some(2))
    );
    let chunk = fs::read(root.join("run-1/events.chunk")).expect("sealed chunk");
    let expected =
        Sha256::digest(&chunk)
            .iter()
            .fold(String::with_capacity(64), |mut output, byte| {
                write!(output, "{byte:02x}").expect("writing to String cannot fail");
                output
            });
    assert_eq!(manifest.chunk_sha256, expected);
    assert!(!root.join("run-1/events.chunk.partial").exists());
}

#[test]
fn large_run_rotates_into_verified_chunks() {
    let root = root("rotated-run");
    let writer = RunWriter::start(&root, "run-rotated", 32, 0).expect("start writer");
    let payload = "x".repeat(1024 * 1024);
    for sequence in 1..=9 {
        assert_eq!(
            writer.enqueue(RunRecord::evidence(sequence, sequence, "bulk", &payload)),
            EnqueueResult::Accepted
        );
    }

    let manifest = writer.seal().expect("seal rotated run");
    assert_eq!(manifest.completion, Completion::Complete);
    assert!(manifest.chunks.len() >= 2);
    assert!(
        manifest
            .chunks
            .iter()
            .all(|chunk| root.join("run-rotated").join(&chunk.file).is_file())
    );
}

#[test]
fn nonmonotonic_sequence_seals_explicitly_incomplete() {
    let root = root("nonmonotonic-run");
    let writer = RunWriter::start(&root, "run-2", 8, 0).expect("start writer");
    assert_eq!(
        writer.enqueue(RunRecord::lifecycle(2, 20, "running")),
        EnqueueResult::Accepted
    );
    assert_eq!(
        writer.enqueue(RunRecord::lifecycle(1, 30, "regression")),
        EnqueueResult::Accepted
    );
    let manifest = writer.seal().expect("seal incomplete run");
    assert_eq!(manifest.completion, Completion::Incomplete);
    assert_eq!(
        manifest.incomplete_reason.as_deref(),
        Some("nonmonotonic_sequence")
    );
}

#[test]
fn impossible_free_space_threshold_degrades_without_blocking() {
    let root = root("space-run");
    let writer = RunWriter::start(&root, "run-3", 8, u64::MAX).expect("start degraded writer");
    assert_eq!(
        writer.enqueue(RunRecord::lifecycle(1, 10, "starting")),
        EnqueueResult::Degraded
    );
    let manifest = writer.seal().expect("seal degraded run");
    assert_eq!(manifest.completion, Completion::Incomplete);
    assert_eq!(
        manifest.incomplete_reason.as_deref(),
        Some("minimum_free_space")
    );
}

#[test]
fn interrupted_partial_run_is_recovered_without_becoming_complete() {
    let root = root("recover-run");
    let run = root.join("run-4");
    fs::create_dir_all(&run).expect("run directory");
    fs::write(run.join("events.chunk.partial"), b"exact partial bytes").expect("partial chunk");
    let recovered = recover_incomplete_runs(&root).expect("recover runs");
    assert_eq!(recovered, vec![run.join("manifest.pb")]);
    let manifest = celerity_proto::celerity::v1::RunManifest::decode(
        fs::read(&recovered[0]).expect("manifest").as_slice(),
    )
    .expect("protobuf");
    assert_eq!(
        manifest.completion,
        celerity_proto::celerity::v1::Completion::Incomplete as i32
    );
    assert_eq!(manifest.incomplete_reason.as_deref(), Some("interrupted"));
    assert!(run.join("events.chunk.partial").exists());
}
