import {
  CommandSource as ProtoCommandSource,
  DiagnosticHealthState as ProtoDiagnosticHealthState,
  DiagnosticUnknownReason as ProtoDiagnosticUnknownReason,
  FeatureAuthority as ProtoFeatureAuthority,
  GlobalAuthority as ProtoGlobalAuthority,
  PresentationState as ProtoPresentationState,
  RunStorageHealth as ProtoRunStorageHealth,
  StatusEnvelope as ProtoStatusEnvelope,
  type AcceptedRadiatorSplitCommand as ProtoAcceptedRadiatorSplitCommandType,
  type DiagnosticsSnapshot as ProtoDiagnosticsSnapshotType,
  type DiagnosticStatus as ProtoDiagnosticStatusType,
  type ObservedTemperature as ProtoObservedTemperatureType,
  type StatusEnvelope as ProtoStatusEnvelopeType,
} from "./generated/celerity/v1/celerity";
import type {
  AcceptedRadiatorSplitCommand,
  CommandSource,
  DiagnosticStatus,
  DiagnosticUnknownReason,
  DiagnosticsSnapshot,
  FeatureAuthority,
  GlobalAuthority,
  ObservedTemperature,
  PresentationState,
  RunStorageHealth,
  StatusEnvelope,
} from "./status";
import { unsignedInteger } from "./status-validation";

export function parseProtoStatusEnvelope(input: Uint8Array): StatusEnvelope {
  const envelope = ProtoStatusEnvelope.decode(input);
  if (envelope.schema_version !== 1) {
    throw new Error("unexpected status schema version");
  }
  const state = presentationStateFromProto(envelope.state);
  const snapshot = envelope.snapshot
    ? snapshotFromProto(envelope.snapshot)
    : null;
  const last_success_age_ms =
    envelope.last_success_age_ms === undefined
      ? null
      : unsignedInteger(envelope.last_success_age_ms, "last success age");
  if (state === "unavailable" && snapshot !== null) {
    throw new Error("unavailable status cannot retain a snapshot");
  }
  if (state !== "unavailable" && snapshot === null) {
    throw new Error("current or stale status requires a snapshot");
  }
  if (state !== "unavailable" && last_success_age_ms === null) {
    throw new Error("current or stale status requires a last success age");
  }
  return {
    schema_version: 1,
    state,
    consecutive_failures: unsignedInteger(
      envelope.consecutive_failures,
      "consecutive failures",
    ),
    last_success_age_ms,
    snapshot,
  };
}

export function encodeProtoStatusEnvelope(input: StatusEnvelope): Uint8Array {
  return ProtoStatusEnvelope.encode(toProtoStatusEnvelope(input)).finish();
}

function presentationStateFromProto(
  value: ProtoPresentationState,
): PresentationState {
  switch (value) {
    case ProtoPresentationState.PRESENTATION_STATE_CURRENT:
      return "current";
    case ProtoPresentationState.PRESENTATION_STATE_STALE:
      return "stale";
    case ProtoPresentationState.PRESENTATION_STATE_UNAVAILABLE:
      return "unavailable";
    default:
      throw new Error("invalid presentation state");
  }
}

function snapshotFromProto(
  input: ProtoDiagnosticsSnapshotType,
): DiagnosticsSnapshot {
  if (input.schema_version !== 2) {
    throw new Error("unexpected diagnostics schema version");
  }
  const controller_runtime_lease_health = input.controller_runtime_lease_health;
  const controller_command_ack_health = input.controller_command_ack_health;
  if (!controller_runtime_lease_health || !controller_command_ack_health) {
    throw new Error("diagnostics snapshot is missing health status");
  }
  return {
    schema_version: 2,
    runtime_update_age_ms: unsignedInteger(
      input.runtime_update_age_ms,
      "runtime update age",
    ),
    runtime_update_stale_after_ms: unsignedInteger(
      input.runtime_update_stale_after_ms,
      "runtime stale threshold",
    ),
    global_authority: globalAuthorityFromProto(input.global_authority),
    feature_authority: featureAuthorityFromProto(input.feature_authority),
    command_source: commandSourceFromProto(input.command_source),
    accepted_radiator_split_command: input.accepted_radiator_split_command
      ? acceptedCommandFromProto(input.accepted_radiator_split_command)
      : null,
    coolant_temperature: input.coolant_temperature
      ? temperatureFromProto(input.coolant_temperature)
      : null,
    intake_air_temperature: input.intake_air_temperature
      ? temperatureFromProto(input.intake_air_temperature)
      : null,
    controller_runtime_lease_health: diagnosticStatusFromProto(
      controller_runtime_lease_health,
    ),
    controller_command_ack_health: diagnosticStatusFromProto(
      controller_command_ack_health,
    ),
    run_storage_health: runStorageHealthFromProto(input.run_storage_health),
  };
}

function acceptedCommandFromProto(
  input: ProtoAcceptedRadiatorSplitCommandType,
): AcceptedRadiatorSplitCommand {
  if (
    !Number.isSafeInteger(input.basis_points) ||
    input.basis_points < 0 ||
    input.basis_points > 10_000
  ) {
    throw new Error("accepted command basis points exceed contract");
  }
  return {
    basis_points: input.basis_points,
    source: commandSourceFromProto(input.source),
  };
}

function temperatureFromProto(
  input: ProtoObservedTemperatureType,
): ObservedTemperature {
  if (!Number.isFinite(input.degrees_celsius)) {
    throw new Error("temperature must be finite");
  }
  return {
    degrees_celsius: input.degrees_celsius,
    observation_age_ms: unsignedInteger(
      input.observation_age_ms,
      "temperature age",
    ),
  };
}

function diagnosticStatusFromProto(
  input: ProtoDiagnosticStatusType,
): DiagnosticStatus {
  switch (input.state) {
    case ProtoDiagnosticHealthState.DIAGNOSTIC_HEALTH_STATE_HEALTHY:
      return { state: "healthy" };
    case ProtoDiagnosticHealthState.DIAGNOSTIC_HEALTH_STATE_UNHEALTHY:
      return { state: "unhealthy" };
    case ProtoDiagnosticHealthState.DIAGNOSTIC_HEALTH_STATE_UNKNOWN:
      if (input.reason === undefined) {
        throw new Error("unknown diagnostic status has no reason");
      }
      return {
        state: "unknown",
        reason: diagnosticUnknownReasonFromProto(input.reason),
      };
    default:
      throw new Error("invalid diagnostic health state");
  }
}

function globalAuthorityFromProto(
  value: ProtoGlobalAuthority,
): GlobalAuthority {
  switch (value) {
    case ProtoGlobalAuthority.GLOBAL_AUTHORITY_FALLBACK:
      return "fallback";
    case ProtoGlobalAuthority.GLOBAL_AUTHORITY_ACTIVE:
      return "active";
    case ProtoGlobalAuthority.GLOBAL_AUTHORITY_HARD_FAULT:
      return "hard_fault";
    default:
      throw new Error("invalid global authority");
  }
}

function featureAuthorityFromProto(
  value: ProtoFeatureAuthority,
): FeatureAuthority {
  switch (value) {
    case ProtoFeatureAuthority.FEATURE_AUTHORITY_FALLBACK:
      return "fallback";
    case ProtoFeatureAuthority.FEATURE_AUTHORITY_ARMING:
      return "arming";
    case ProtoFeatureAuthority.FEATURE_AUTHORITY_ACTIVE:
      return "active";
    default:
      throw new Error("invalid feature authority");
  }
}

function commandSourceFromProto(value: ProtoCommandSource): CommandSource {
  switch (value) {
    case ProtoCommandSource.COMMAND_SOURCE_CONTROLLER_LOCAL_FALLBACK:
      return "controller_local_fallback";
    case ProtoCommandSource.COMMAND_SOURCE_DETERMINISTIC:
      return "deterministic";
    case ProtoCommandSource.COMMAND_SOURCE_MODEL_OPTIMIZED:
      return "model_optimized";
    case ProtoCommandSource.COMMAND_SOURCE_EXPERIMENT:
      return "experiment";
    default:
      throw new Error("invalid command source");
  }
}

function diagnosticUnknownReasonFromProto(
  value: ProtoDiagnosticUnknownReason,
): DiagnosticUnknownReason {
  switch (value) {
    case ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_NEVER_OBSERVED:
      return "never_observed";
    case ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_NOT_EXPECTED_IN_FALLBACK:
      return "not_expected_in_fallback";
    case ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_NOT_RENEWING:
      return "not_renewing";
    case ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_NO_OUTSTANDING_COMMAND:
      return "no_outstanding_command";
    case ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_AWAITING_EVIDENCE:
      return "awaiting_evidence";
    default:
      throw new Error("invalid diagnostic unknown reason");
  }
}

function runStorageHealthFromProto(
  value: ProtoRunStorageHealth,
): RunStorageHealth {
  switch (value) {
    case ProtoRunStorageHealth.RUN_STORAGE_HEALTH_UNKNOWN:
      return "unknown";
    case ProtoRunStorageHealth.RUN_STORAGE_HEALTH_HEALTHY:
      return "healthy";
    case ProtoRunStorageHealth.RUN_STORAGE_HEALTH_DEGRADED:
      return "degraded";
    default:
      throw new Error("invalid Run storage health");
  }
}

function toProtoStatusEnvelope(input: StatusEnvelope): ProtoStatusEnvelopeType {
  return {
    schema_version: input.schema_version,
    state: {
      current: ProtoPresentationState.PRESENTATION_STATE_CURRENT,
      stale: ProtoPresentationState.PRESENTATION_STATE_STALE,
      unavailable: ProtoPresentationState.PRESENTATION_STATE_UNAVAILABLE,
    }[input.state],
    consecutive_failures: input.consecutive_failures,
    last_success_age_ms: input.last_success_age_ms ?? undefined,
    snapshot: input.snapshot ? toProtoSnapshot(input.snapshot) : undefined,
  };
}

function toProtoSnapshot(
  input: DiagnosticsSnapshot,
): ProtoDiagnosticsSnapshotType {
  return {
    schema_version: input.schema_version,
    runtime_update_age_ms: input.runtime_update_age_ms,
    runtime_update_stale_after_ms: input.runtime_update_stale_after_ms,
    global_authority: {
      fallback: ProtoGlobalAuthority.GLOBAL_AUTHORITY_FALLBACK,
      active: ProtoGlobalAuthority.GLOBAL_AUTHORITY_ACTIVE,
      hard_fault: ProtoGlobalAuthority.GLOBAL_AUTHORITY_HARD_FAULT,
    }[input.global_authority],
    feature_authority: {
      fallback: ProtoFeatureAuthority.FEATURE_AUTHORITY_FALLBACK,
      arming: ProtoFeatureAuthority.FEATURE_AUTHORITY_ARMING,
      active: ProtoFeatureAuthority.FEATURE_AUTHORITY_ACTIVE,
    }[input.feature_authority],
    command_source: toProtoCommandSource(input.command_source),
    accepted_radiator_split_command: input.accepted_radiator_split_command
      ? {
          basis_points: input.accepted_radiator_split_command.basis_points,
          source: toProtoCommandSource(
            input.accepted_radiator_split_command.source,
          ),
        }
      : undefined,
    coolant_temperature: input.coolant_temperature
      ? { ...input.coolant_temperature }
      : undefined,
    intake_air_temperature: input.intake_air_temperature
      ? { ...input.intake_air_temperature }
      : undefined,
    controller_runtime_lease_health: toProtoDiagnosticStatus(
      input.controller_runtime_lease_health,
    ),
    controller_command_ack_health: toProtoDiagnosticStatus(
      input.controller_command_ack_health,
    ),
    run_storage_health: {
      unknown: ProtoRunStorageHealth.RUN_STORAGE_HEALTH_UNKNOWN,
      healthy: ProtoRunStorageHealth.RUN_STORAGE_HEALTH_HEALTHY,
      degraded: ProtoRunStorageHealth.RUN_STORAGE_HEALTH_DEGRADED,
    }[input.run_storage_health],
  };
}

function toProtoCommandSource(value: CommandSource): ProtoCommandSource {
  return {
    controller_local_fallback:
      ProtoCommandSource.COMMAND_SOURCE_CONTROLLER_LOCAL_FALLBACK,
    deterministic: ProtoCommandSource.COMMAND_SOURCE_DETERMINISTIC,
    model_optimized: ProtoCommandSource.COMMAND_SOURCE_MODEL_OPTIMIZED,
    experiment: ProtoCommandSource.COMMAND_SOURCE_EXPERIMENT,
  }[value];
}

function toProtoDiagnosticStatus(
  input: DiagnosticStatus,
): ProtoDiagnosticStatusType {
  return input.state === "healthy"
    ? { state: ProtoDiagnosticHealthState.DIAGNOSTIC_HEALTH_STATE_HEALTHY }
    : input.state === "unhealthy"
      ? { state: ProtoDiagnosticHealthState.DIAGNOSTIC_HEALTH_STATE_UNHEALTHY }
      : {
          state: ProtoDiagnosticHealthState.DIAGNOSTIC_HEALTH_STATE_UNKNOWN,
          reason: {
            never_observed:
              ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_NEVER_OBSERVED,
            not_expected_in_fallback:
              ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_NOT_EXPECTED_IN_FALLBACK,
            not_renewing:
              ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_NOT_RENEWING,
            no_outstanding_command:
              ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_NO_OUTSTANDING_COMMAND,
            awaiting_evidence:
              ProtoDiagnosticUnknownReason.DIAGNOSTIC_UNKNOWN_REASON_AWAITING_EVIDENCE,
          }[input.reason],
        };
}
