import type {
  AcceptedRadiatorSplitCommand,
  DiagnosticStatus,
  DiagnosticsSnapshot,
  ObservedTemperature,
  StatusEnvelope,
  CommandSource,
} from "./status";
import {
  exactKeys,
  isRecord,
  nullableUnsignedInteger,
  stringUnion,
  unsignedInteger,
} from "./status-validation";

export function parseJsonStatusEnvelope(input: unknown): StatusEnvelope {
  if (!isRecord(input)) {
    throw new Error("status envelope must be an object");
  }
  exactKeys(input, [
    "schema_version",
    "state",
    "consecutive_failures",
    "last_success_age_ms",
    "snapshot",
  ]);
  if (input.schema_version !== 1) {
    throw new Error("unexpected status schema version");
  }
  const state = stringUnion(
    input.state,
    ["current", "stale", "unavailable"],
    "presentation state",
  );
  const consecutive_failures = unsignedInteger(
    input.consecutive_failures,
    "consecutive failures",
  );
  const last_success_age_ms = nullableUnsignedInteger(
    input.last_success_age_ms,
    "last success age",
  );
  const snapshot =
    input.snapshot === null ? null : parseSnapshot(input.snapshot);
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
    consecutive_failures,
    last_success_age_ms,
    snapshot,
  };
}

function parseSnapshot(input: unknown): DiagnosticsSnapshot {
  if (!isRecord(input)) {
    throw new Error("diagnostics snapshot must be an object");
  }
  exactKeys(input, [
    "schema_version",
    "runtime_update_age_ms",
    "runtime_update_stale_after_ms",
    "global_authority",
    "feature_authority",
    "command_source",
    "accepted_radiator_split_command",
    "coolant_temperature",
    "intake_air_temperature",
    "controller_runtime_lease_health",
    "controller_command_ack_health",
    "run_storage_health",
  ]);
  if (input.schema_version !== 2) {
    throw new Error("unexpected diagnostics schema version");
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
    global_authority: stringUnion(
      input.global_authority,
      ["fallback", "active", "hard_fault"],
      "global authority",
    ),
    feature_authority: stringUnion(
      input.feature_authority,
      ["fallback", "arming", "active"],
      "feature authority",
    ),
    command_source: parseCommandSource(input.command_source),
    accepted_radiator_split_command:
      input.accepted_radiator_split_command === null
        ? null
        : parseAcceptedCommand(input.accepted_radiator_split_command),
    coolant_temperature:
      input.coolant_temperature === null
        ? null
        : parseTemperature(input.coolant_temperature),
    intake_air_temperature:
      input.intake_air_temperature === null
        ? null
        : parseTemperature(input.intake_air_temperature),
    controller_runtime_lease_health: parseDiagnosticStatus(
      input.controller_runtime_lease_health,
    ),
    controller_command_ack_health: parseDiagnosticStatus(
      input.controller_command_ack_health,
    ),
    run_storage_health: stringUnion(
      input.run_storage_health,
      ["unknown", "healthy", "degraded"],
      "Run storage health",
    ),
  };
}

function parseAcceptedCommand(input: unknown): AcceptedRadiatorSplitCommand {
  if (!isRecord(input)) {
    throw new Error("accepted command must be an object");
  }
  exactKeys(input, ["basis_points", "source"]);
  const basis_points = unsignedInteger(
    input.basis_points,
    "accepted command basis points",
  );
  if (basis_points > 10_000) {
    throw new Error("accepted command basis points exceed contract");
  }
  return { basis_points, source: parseCommandSource(input.source) };
}

function parseTemperature(input: unknown): ObservedTemperature {
  if (!isRecord(input)) {
    throw new Error("observed temperature must be an object");
  }
  exactKeys(input, ["degrees_celsius", "observation_age_ms"]);
  if (
    typeof input.degrees_celsius !== "number" ||
    !Number.isFinite(input.degrees_celsius)
  ) {
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

function parseDiagnosticStatus(input: unknown): DiagnosticStatus {
  if (!isRecord(input)) {
    throw new Error("diagnostic health must be an object");
  }
  if (input.state === "unknown") {
    exactKeys(input, ["state", "reason"]);
    return {
      state: "unknown",
      reason: stringUnion(
        input.reason,
        [
          "never_observed",
          "not_expected_in_fallback",
          "not_renewing",
          "no_outstanding_command",
          "awaiting_evidence",
        ],
        "unknown reason",
      ),
    };
  }
  exactKeys(input, ["state"]);
  return {
    state: stringUnion(
      input.state,
      ["healthy", "unhealthy"],
      "diagnostic health",
    ),
  };
}

function parseCommandSource(input: unknown): CommandSource {
  return stringUnion(
    input,
    [
      "controller_local_fallback",
      "deterministic",
      "model_optimized",
      "experiment",
    ],
    "command source",
  );
}
