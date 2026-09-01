export type PresentationState = "current" | "stale" | "unavailable";
export type GlobalAuthority = "fallback" | "active" | "hard_fault";
export type FeatureAuthority = "fallback" | "arming" | "active";
export type CommandSource =
  | "controller_local_fallback"
  | "deterministic"
  | "model_optimized"
  | "experiment";
export type DiagnosticUnknownReason =
  | "never_observed"
  | "not_expected_in_fallback"
  | "not_renewing"
  | "no_outstanding_command"
  | "awaiting_evidence";
export type DiagnosticStatus =
  | { state: "unknown"; reason: DiagnosticUnknownReason }
  | { state: "healthy" }
  | { state: "unhealthy" };
export type RunStorageHealth = "unknown" | "healthy" | "degraded";

export interface ObservedTemperature {
  degrees_celsius: number;
  observation_age_ms: number;
}

export interface AcceptedRadiatorSplitCommand {
  basis_points: number;
  source: CommandSource;
}

export interface DiagnosticsSnapshot {
  schema_version: 2;
  runtime_update_age_ms: number;
  runtime_update_stale_after_ms: number;
  global_authority: GlobalAuthority;
  feature_authority: FeatureAuthority;
  command_source: CommandSource;
  accepted_radiator_split_command: AcceptedRadiatorSplitCommand | null;
  coolant_temperature: ObservedTemperature | null;
  intake_air_temperature: ObservedTemperature | null;
  controller_runtime_lease_health: DiagnosticStatus;
  controller_command_ack_health: DiagnosticStatus;
  run_storage_health: RunStorageHealth;
}

export interface StatusEnvelope {
  schema_version: 1;
  state: PresentationState;
  consecutive_failures: number;
  last_success_age_ms: number | null;
  snapshot: DiagnosticsSnapshot | null;
}

export type UnavailableReason =
  | "NOT YET OBSERVED"
  | "DIAGNOSTICS UNAVAILABLE"
  | "INVALID STATUS RESPONSE"
  | "LOCAL OBSERVER UNREACHABLE";

export interface ViewerStatus {
  envelope: StatusEnvelope;
  receivedAtMs: number;
  reason: UnavailableReason | null;
}

const MAX_SAFE_MS = Number.MAX_SAFE_INTEGER;

export function parseStatusEnvelope(input: unknown): StatusEnvelope {
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

export function initialView(now: number): ViewerStatus {
  return unavailableView(null, now, "NOT YET OBSERVED");
}

export function invalidResponseView(
  previous: ViewerStatus,
  now: number,
): ViewerStatus {
  return unavailableView(previous, now, "INVALID STATUS RESPONSE");
}

export function unreachableView(
  previous: ViewerStatus,
  now: number,
): ViewerStatus {
  return unavailableView(previous, now, "LOCAL OBSERVER UNREACHABLE");
}

export async function loadStatus(
  previous: ViewerStatus,
): Promise<ViewerStatus> {
  let response: Response;
  try {
    response = await fetch("/v1/status", {
      method: "GET",
      headers: { Accept: "application/json" },
      cache: "no-store",
    });
  } catch {
    return unreachableView(previous, performance.now());
  }
  const receivedAtMs = performance.now();
  if (!response.ok) {
    return invalidResponseView(previous, receivedAtMs);
  }
  try {
    const envelope = parseStatusEnvelope(await response.json());
    return {
      envelope,
      receivedAtMs,
      reason:
        envelope.state === "unavailable"
          ? envelope.last_success_age_ms === null
            ? "NOT YET OBSERVED"
            : "DIAGNOSTICS UNAVAILABLE"
          : null,
    };
  } catch {
    return invalidResponseView(previous, receivedAtMs);
  }
}

export function temperatureAgeNow(
  temperature: ObservedTemperature,
  view: ViewerStatus,
  now: number,
): number {
  return saturatingSum([
    temperature.observation_age_ms,
    view.envelope.snapshot?.runtime_update_age_ms ?? 0,
    view.envelope.last_success_age_ms ?? 0,
    Math.max(0, Math.floor(now - view.receivedAtMs)),
  ]);
}

export function commandRatio(basisPoints: number): string {
  return (basisPoints / 10_000).toFixed(2);
}

export function globalAuthorityLabel(value: GlobalAuthority): string {
  switch (value) {
    case "fallback":
      return "FALLBACK";
    case "active":
      return "ACTIVE";
    case "hard_fault":
      return "HARD FAULT";
    default:
      return assertNever(value);
  }
}

export function featureAuthorityLabel(value: FeatureAuthority): string {
  switch (value) {
    case "fallback":
      return "FALLBACK";
    case "arming":
      return "ARMING";
    case "active":
      return "ACTIVE";
    default:
      return assertNever(value);
  }
}

export function commandSourceLabel(value: CommandSource): string {
  switch (value) {
    case "controller_local_fallback":
      return "CONTROLLER-LOCAL FALLBACK";
    case "deterministic":
      return "DETERMINISTIC";
    case "model_optimized":
      return "MODEL OPTIMIZED";
    case "experiment":
      return "EXPERIMENT";
    default:
      return assertNever(value);
  }
}

export function diagnosticStatusLabel(value: DiagnosticStatus): string {
  switch (value.state) {
    case "healthy":
      return "HEALTHY";
    case "unhealthy":
      return "UNHEALTHY";
    case "unknown":
      return `UNKNOWN — ${unknownReasonLabel(value.reason)}`;
    default:
      return assertNever(value);
  }
}

export function runStorageLabel(value: RunStorageHealth): string {
  switch (value) {
    case "unknown":
      return "UNKNOWN";
    case "healthy":
      return "HEALTHY";
    case "degraded":
      return "DEGRADED";
    default:
      return assertNever(value);
  }
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

function unavailableView(
  previous: ViewerStatus | null,
  now: number,
  reason: UnavailableReason,
): ViewerStatus {
  const previousAge = previous?.envelope.last_success_age_ms;
  const last_success_age_ms =
    previous === null || previousAge === null || previousAge === undefined
      ? null
      : saturatingSum([
          previousAge,
          Math.max(0, Math.floor(now - previous.receivedAtMs)),
        ]);
  return {
    envelope: {
      schema_version: 1,
      state: "unavailable",
      consecutive_failures: previous?.envelope.consecutive_failures ?? 0,
      last_success_age_ms,
      snapshot: null,
    },
    receivedAtMs: now,
    reason,
  };
}

function unknownReasonLabel(value: DiagnosticUnknownReason): string {
  switch (value) {
    case "never_observed":
      return "NEVER OBSERVED";
    case "not_expected_in_fallback":
      return "NOT EXPECTED IN FALLBACK";
    case "not_renewing":
      return "NOT RENEWING";
    case "no_outstanding_command":
      return "NO OUTSTANDING COMMAND";
    case "awaiting_evidence":
      return "AWAITING EVIDENCE";
    default:
      return assertNever(value);
  }
}

function unsignedInteger(input: unknown, name: string): number {
  if (
    typeof input !== "number" ||
    !Number.isSafeInteger(input) ||
    input < 0 ||
    input > MAX_SAFE_MS
  ) {
    throw new Error(`${name} must be an unsigned safe integer`);
  }
  return input;
}

function nullableUnsignedInteger(input: unknown, name: string): number | null {
  return input === null ? null : unsignedInteger(input, name);
}

function stringUnion<const Values extends readonly string[]>(
  input: unknown,
  values: Values,
  name: string,
): Values[number] {
  if (typeof input !== "string" || !values.includes(input)) {
    throw new Error(`${name} is outside the contract`);
  }
  return input;
}

function exactKeys(input: Record<string, unknown>, expected: string[]): void {
  const actual = Object.keys(input).sort();
  const wanted = [...expected].sort();
  if (
    actual.length !== wanted.length ||
    actual.some((key, index) => key !== wanted[index])
  ) {
    throw new Error("status object fields are outside the contract");
  }
}

function isRecord(input: unknown): input is Record<string, unknown> {
  return typeof input === "object" && input !== null && !Array.isArray(input);
}

function saturatingSum(values: number[]): number {
  return values.reduce(
    (total, value) => Math.min(MAX_SAFE_MS, total + value),
    0,
  );
}

function assertNever(value: never): never {
  throw new Error(`unhandled status value: ${String(value)}`);
}
