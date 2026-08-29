use std::{env, io, path::PathBuf};

use control_core::{
    EnqueueResult, ExternalAdapters, RunRecord, RunWriter, Runtime, SimulationScenario,
    StartupMode, ValidatedBundle, replay_events,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    match arguments.as_slice() {
        [command, bundle_path] if command == "simulate" => simulate(bundle_path),
        [command, run_path, behavior] if command == "replay" && behavior == "--verify" => {
            replay(run_path)
        }
        _ => Err(io::Error::other(
            "usage: celerity simulate <bundle.toml> | celerity replay <run> --verify",
        )
        .into()),
    }
}

fn simulate(bundle_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let bundle = ValidatedBundle::load(
        PathBuf::from(bundle_path).as_path(),
        StartupMode::Simulation,
    )
    .map_err(debug_error)?;
    let scenario_path = bundle
        .simulation_scenario()
        .ok_or_else(|| io::Error::other("simulation composition missing scenario"))?
        .to_owned();
    let run_root = bundle.run_storage_root().to_path_buf();
    let scenario =
        SimulationScenario::load(PathBuf::from(scenario_path).as_path()).map_err(debug_error)?;
    let run_id = scenario.name().to_owned();
    let writer = RunWriter::start(&run_root, &run_id, 64, 0)?;
    for (index, event) in scenario.events().iter().enumerate() {
        let sequence = u64::try_from(index + 1)?;
        if writer.enqueue(RunRecord::runtime_event(sequence, event)?) != EnqueueResult::Accepted {
            return Err(io::Error::other("Run writer degraded during simulation").into());
        }
    }
    let outcome = Runtime::new(bundle, ExternalAdapters::simulation(scenario.into_events()))
        .map_err(debug_error)?
        .run();
    let manifest = writer.seal()?;
    println!(
        "scenario={} pass authority={:?} source={:?} accepted={:?} run={}/{} digest={} completion={:?}",
        run_id,
        outcome.final_feature_authority,
        outcome.command_source,
        outcome.accepted_basis_points,
        run_root.display(),
        run_id,
        manifest.chunk_sha256,
        manifest.completion,
    );
    Ok(())
}

fn replay(run_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = env::var("CELERITY_REPLAY_CONFIG")
        .unwrap_or_else(|_| "config/examples/replay.toml".to_owned());
    let bundle = ValidatedBundle::load(PathBuf::from(config_path).as_path(), StartupMode::Replay)
        .map_err(debug_error)?;
    let events = replay_events(PathBuf::from(run_path).as_path())?;
    let outcome = Runtime::new(bundle, ExternalAdapters::replay(events))
        .map_err(debug_error)?
        .run();
    println!(
        "replay={} verified authority={:?} source={:?} accepted={:?}",
        run_path,
        outcome.final_feature_authority,
        outcome.command_source,
        outcome.accepted_basis_points,
    );
    Ok(())
}

fn debug_error(error: impl std::fmt::Debug) -> io::Error {
    io::Error::other(format!("{error:?}"))
}
