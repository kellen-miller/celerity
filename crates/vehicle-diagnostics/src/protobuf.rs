use super::*;

fn global_authority_to_proto(value: GlobalAuthority) -> ProtoGlobalAuthority {
    match value {
        GlobalAuthority::Fallback => ProtoGlobalAuthority::Fallback,
        GlobalAuthority::Active => ProtoGlobalAuthority::Active,
        GlobalAuthority::HardFault => ProtoGlobalAuthority::HardFault,
    }
}

fn global_authority_from_proto(value: i32) -> Result<GlobalAuthority, String> {
    match ProtoGlobalAuthority::try_from(value)
        .map_err(|_| "unknown global authority".to_owned())?
    {
        ProtoGlobalAuthority::Fallback => Ok(GlobalAuthority::Fallback),
        ProtoGlobalAuthority::Active => Ok(GlobalAuthority::Active),
        ProtoGlobalAuthority::HardFault => Ok(GlobalAuthority::HardFault),
        ProtoGlobalAuthority::Unspecified => Err("unspecified global authority".to_owned()),
    }
}

fn feature_authority_to_proto(value: FeatureAuthority) -> ProtoFeatureAuthority {
    match value {
        FeatureAuthority::Fallback => ProtoFeatureAuthority::Fallback,
        FeatureAuthority::Arming => ProtoFeatureAuthority::Arming,
        FeatureAuthority::Active => ProtoFeatureAuthority::Active,
    }
}

fn feature_authority_from_proto(value: i32) -> Result<FeatureAuthority, String> {
    match ProtoFeatureAuthority::try_from(value)
        .map_err(|_| "unknown feature authority".to_owned())?
    {
        ProtoFeatureAuthority::Fallback => Ok(FeatureAuthority::Fallback),
        ProtoFeatureAuthority::Arming => Ok(FeatureAuthority::Arming),
        ProtoFeatureAuthority::Active => Ok(FeatureAuthority::Active),
        ProtoFeatureAuthority::Unspecified => Err("unspecified feature authority".to_owned()),
    }
}

fn command_source_to_proto(value: CommandSource) -> ProtoCommandSource {
    match value {
        CommandSource::ControllerLocalFallback => ProtoCommandSource::ControllerLocalFallback,
        CommandSource::Deterministic => ProtoCommandSource::Deterministic,
        CommandSource::ModelOptimized => ProtoCommandSource::ModelOptimized,
        CommandSource::Experiment => ProtoCommandSource::Experiment,
    }
}

fn command_source_from_proto(value: i32) -> Result<CommandSource, String> {
    match ProtoCommandSource::try_from(value).map_err(|_| "unknown command source".to_owned())? {
        ProtoCommandSource::ControllerLocalFallback => Ok(CommandSource::ControllerLocalFallback),
        ProtoCommandSource::Deterministic => Ok(CommandSource::Deterministic),
        ProtoCommandSource::ModelOptimized => Ok(CommandSource::ModelOptimized),
        ProtoCommandSource::Experiment => Ok(CommandSource::Experiment),
        ProtoCommandSource::Unspecified => Err("unspecified command source".to_owned()),
    }
}

fn diagnostic_unknown_reason_to_proto(
    value: DiagnosticUnknownReason,
) -> ProtoDiagnosticUnknownReason {
    match value {
        DiagnosticUnknownReason::NeverObserved => ProtoDiagnosticUnknownReason::NeverObserved,
        DiagnosticUnknownReason::NotExpectedInFallback => {
            ProtoDiagnosticUnknownReason::NotExpectedInFallback
        }
        DiagnosticUnknownReason::NotRenewing => ProtoDiagnosticUnknownReason::NotRenewing,
        DiagnosticUnknownReason::NoOutstandingCommand => {
            ProtoDiagnosticUnknownReason::NoOutstandingCommand
        }
        DiagnosticUnknownReason::AwaitingEvidence => ProtoDiagnosticUnknownReason::AwaitingEvidence,
    }
}

fn diagnostic_unknown_reason_from_proto(value: i32) -> Result<DiagnosticUnknownReason, String> {
    match ProtoDiagnosticUnknownReason::try_from(value)
        .map_err(|_| "unknown diagnostics reason".to_owned())?
    {
        ProtoDiagnosticUnknownReason::NeverObserved => Ok(DiagnosticUnknownReason::NeverObserved),
        ProtoDiagnosticUnknownReason::NotExpectedInFallback => {
            Ok(DiagnosticUnknownReason::NotExpectedInFallback)
        }
        ProtoDiagnosticUnknownReason::NotRenewing => Ok(DiagnosticUnknownReason::NotRenewing),
        ProtoDiagnosticUnknownReason::NoOutstandingCommand => {
            Ok(DiagnosticUnknownReason::NoOutstandingCommand)
        }
        ProtoDiagnosticUnknownReason::AwaitingEvidence => {
            Ok(DiagnosticUnknownReason::AwaitingEvidence)
        }
        ProtoDiagnosticUnknownReason::Unspecified => {
            Err("unspecified diagnostics reason".to_owned())
        }
    }
}

fn diagnostic_status_to_proto(value: DiagnosticStatus) -> ProtoDiagnosticStatus {
    match value {
        DiagnosticStatus::Unknown(reason) => ProtoDiagnosticStatus {
            state: ProtoDiagnosticHealthState::Unknown as i32,
            reason: Some(diagnostic_unknown_reason_to_proto(reason) as i32),
        },
        DiagnosticStatus::Healthy => ProtoDiagnosticStatus {
            state: ProtoDiagnosticHealthState::Healthy as i32,
            reason: None,
        },
        DiagnosticStatus::Unhealthy => ProtoDiagnosticStatus {
            state: ProtoDiagnosticHealthState::Unhealthy as i32,
            reason: None,
        },
    }
}

fn diagnostic_status_from_proto(
    value: Option<&ProtoDiagnosticStatus>,
) -> Result<DiagnosticStatus, String> {
    let value = value.ok_or_else(|| "missing diagnostics status".to_owned())?;
    match ProtoDiagnosticHealthState::try_from(value.state)
        .map_err(|_| "unknown diagnostics health state".to_owned())?
    {
        ProtoDiagnosticHealthState::Unknown => {
            let reason = value
                .reason
                .ok_or_else(|| "unknown diagnostics status has no reason".to_owned())?;
            Ok(DiagnosticStatus::Unknown(
                diagnostic_unknown_reason_from_proto(reason)?,
            ))
        }
        ProtoDiagnosticHealthState::Healthy => Ok(DiagnosticStatus::Healthy),
        ProtoDiagnosticHealthState::Unhealthy => Ok(DiagnosticStatus::Unhealthy),
        ProtoDiagnosticHealthState::Unspecified => {
            Err("unspecified diagnostics health state".to_owned())
        }
    }
}

fn run_storage_health_to_proto(value: RunStorageHealth) -> ProtoRunStorageHealth {
    match value {
        RunStorageHealth::Unknown => ProtoRunStorageHealth::Unknown,
        RunStorageHealth::Healthy => ProtoRunStorageHealth::Healthy,
        RunStorageHealth::Degraded => ProtoRunStorageHealth::Degraded,
    }
}

fn run_storage_health_from_proto(value: i32) -> Result<RunStorageHealth, String> {
    match ProtoRunStorageHealth::try_from(value)
        .map_err(|_| "unknown Run storage health".to_owned())?
    {
        ProtoRunStorageHealth::Unknown => Ok(RunStorageHealth::Unknown),
        ProtoRunStorageHealth::Healthy => Ok(RunStorageHealth::Healthy),
        ProtoRunStorageHealth::Degraded => Ok(RunStorageHealth::Degraded),
        ProtoRunStorageHealth::Unspecified => Err("unspecified Run storage health".to_owned()),
    }
}
