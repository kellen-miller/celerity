use std::{fs, path::PathBuf};

use control_core::{
    EnqueueResult, ExternalAdapters, RunRecord, RunWriter, Runtime, SimulationScenario,
    StartupMode, ValidatedBundle, replay_events,
};

fn repository_path(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

#[test]
fn sealed_simulation_replays_through_identical_policy() {
    let scenario =
        SimulationScenario::load(&repository_path("fixtures/scenarios/healthy-startup.toml"))
            .expect("scenario");
    let simulation_bundle = ValidatedBundle::load(
        &repository_path("config/examples/simulation.toml"),
        StartupMode::Simulation,
    )
    .expect("simulation config");
    let expected = Runtime::new(
        simulation_bundle,
        ExternalAdapters::simulation(scenario.events().to_vec()),
    )
    .expect("simulation composition")
    .run();

    let root = std::env::temp_dir().join(format!("celerity-replay-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).expect("clean fixture");
    }
    let writer = RunWriter::start(&root, "healthy", 16, 0).expect("writer");
    for (index, event) in scenario.events().iter().enumerate() {
        let sequence = u64::try_from(index + 1).expect("small fixture");
        assert_eq!(
            writer.enqueue(RunRecord::runtime_event(sequence, event).expect("serialize event")),
            EnqueueResult::Accepted,
        );
    }
    writer.seal().expect("seal run");

    let replay_bundle = ValidatedBundle::load(
        &repository_path("config/examples/replay.toml"),
        StartupMode::Replay,
    )
    .expect("replay config");
    let replayed = Runtime::new(
        replay_bundle,
        ExternalAdapters::replay(replay_events(&root.join("healthy")).expect("replay events")),
    )
    .expect("replay composition")
    .run();
    assert_eq!(replayed, expected);
}

#[test]
fn replay_rejects_tampered_chunk() {
    let root = std::env::temp_dir().join(format!("celerity-tamper-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).expect("clean fixture");
    }
    let writer = RunWriter::start(&root, "tampered", 2, 0).expect("writer");
    let event = control_core::RuntimeEvent::Cycle {
        monotonic_ms: 1,
        remaining_cycle_ns: 20_000_000,
    };
    assert_eq!(
        writer.enqueue(RunRecord::runtime_event(1, &event).expect("event")),
        EnqueueResult::Accepted
    );
    writer.seal().expect("seal");
    fs::write(root.join("tampered/events.chunk"), b"tampered").expect("tamper chunk");
    assert!(replay_events(&root.join("tampered")).is_err());
}
