import { parseJsonStatusEnvelope } from "./status-json";
import {
  encodeProtoStatusEnvelope,
  parseProtoStatusEnvelope,
} from "./status-protobuf";
import { assertNever, saturatingSum } from "./status-validation";

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

export function parseStatusEnvelope(input: unknown): StatusEnvelope {
  if (input instanceof Uint8Array) {
    return parseProtoStatusEnvelope(input);
  }
  return parseJsonStatusEnvelope(input);
}

export function encodeStatusEnvelope(input: unknown): Uint8Array {
  return encodeProtoStatusEnvelope(parseStatusEnvelope(input));
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
      headers: { Accept: "application/x-protobuf" },
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
    const envelope = parseStatusEnvelope(
      new Uint8Array(await response.arrayBuffer()),
    );
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
