use std::{fs, path::PathBuf};

use vehicle_diagnostics::{
    AcceptedRadiatorSplitCommand, CommandSource, DiagnosticStatus, DiagnosticUnknownReason,
    DiagnosticsSnapshot, FeatureAuthority, GlobalAuthority, ObservedTemperature, RunStorageHealth,
};
use vehicle_ui::{PresentationState, StatusEnvelope};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args_os().nth(1).map_or_else(
        || PathBuf::from("crates/vehicle-ui/web/fixtures/status"),
        PathBuf::from,
    );
    fs::create_dir_all(&output)?;

    let current_snapshot = DiagnosticsSnapshot {
        schema_version: 2,
        runtime_update_age_ms: 20,
        runtime_update_stale_after_ms: 100,
        global_authority: GlobalAuthority::Active,
        feature_authority: FeatureAuthority::Active,
        command_source: CommandSource::ModelOptimized,
        accepted_radiator_split_command: Some(AcceptedRadiatorSplitCommand {
            basis_points: 9_000,
            source: CommandSource::ModelOptimized,
        }),
        coolant_temperature: Some(ObservedTemperature {
            degrees_celsius: 100.0,
            observation_age_ms: 20,
        }),
        intake_air_temperature: Some(ObservedTemperature {
            degrees_celsius: 40.0,
            observation_age_ms: 25,
        }),
        controller_runtime_lease_health: DiagnosticStatus::Healthy,
        controller_command_ack_health: DiagnosticStatus::Healthy,
        run_storage_health: RunStorageHealth::Healthy,
    };
    write(
        &output,
        "current.json",
        &envelope(
            PresentationState::Current,
            0,
            Some(125),
            Some(current_snapshot.clone()),
        ),
    )?;
    write(
        &output,
        "stale.json",
        &envelope(
            PresentationState::Stale,
            2,
            Some(1_400),
            Some(current_snapshot.clone()),
        ),
    )?;
    let mut producer_stale = current_snapshot;
    producer_stale.runtime_update_age_ms = 500;
    write(
        &output,
        "producer-stale.json",
        &envelope(PresentationState::Stale, 0, Some(80), Some(producer_stale)),
    )?;
    write(
        &output,
        "unavailable.json",
        &envelope(PresentationState::Unavailable, 3, Some(4_300), None),
    )?;
    write(
        &output,
        "unknown.json",
        &envelope(
            PresentationState::Current,
            0,
            Some(40),
            Some(DiagnosticsSnapshot {
                schema_version: 2,
                runtime_update_age_ms: 15,
                runtime_update_stale_after_ms: 100,
                global_authority: GlobalAuthority::Fallback,
                feature_authority: FeatureAuthority::Fallback,
                command_source: CommandSource::ControllerLocalFallback,
                accepted_radiator_split_command: None,
                coolant_temperature: None,
                intake_air_temperature: None,
                controller_runtime_lease_health: DiagnosticStatus::Unknown(
                    DiagnosticUnknownReason::NotExpectedInFallback,
                ),
                controller_command_ack_health: DiagnosticStatus::Unknown(
                    DiagnosticUnknownReason::NoOutstandingCommand,
                ),
                run_storage_health: RunStorageHealth::Unknown,
            }),
        ),
    )?;
    Ok(())
}

fn envelope(
    state: PresentationState,
    consecutive_failures: u32,
    last_success_age_ms: Option<u64>,
    snapshot: Option<DiagnosticsSnapshot>,
) -> StatusEnvelope {
    StatusEnvelope {
        schema_version: 1,
        state,
        consecutive_failures,
        last_success_age_ms,
        snapshot,
    }
}

fn write(
    output: &std::path::Path,
    name: &str,
    envelope: &StatusEnvelope,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = serde_json::to_vec_pretty(envelope)?;
    bytes.push(b'\n');
    fs::write(output.join(name), bytes)?;
    Ok(())
}
