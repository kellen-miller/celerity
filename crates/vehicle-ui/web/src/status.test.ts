import { describe, expect, test } from "vitest";

import currentFixture from "../fixtures/status/current.json";
import producerStaleFixture from "../fixtures/status/producer-stale.json";
import unavailableFixture from "../fixtures/status/unavailable.json";
import unknownFixture from "../fixtures/status/unknown.json";
import {
  commandRatio,
  diagnosticStatusLabel,
  initialView,
  invalidResponseView,
  parseStatusEnvelope,
  temperatureAgeNow,
  unreachableView,
} from "./status";

describe("status contract", () => {
  test("parses Rust-generated envelopes and rejects schema drift", () => {
    expect(parseStatusEnvelope(currentFixture).state).toBe("current");
    expect(
      parseStatusEnvelope(unknownFixture).snapshot?.run_storage_health,
    ).toBe("unknown");
    expect(parseStatusEnvelope(unavailableFixture).snapshot).toBeNull();
    expect(() =>
      parseStatusEnvelope({ ...currentFixture, schema_version: 2 }),
    ).toThrow("status schema version");
    for (const state of ["current", "stale"] as const) {
      expect(() =>
        parseStatusEnvelope({
          ...currentFixture,
          state,
          last_success_age_ms: null,
        }),
      ).toThrow("last success age");
    }
    expect(() => parseStatusEnvelope("{not json")).toThrow("status envelope");
  });

  test("starts unavailable and distinguishes invalid and unreachable responses", () => {
    const initial = initialView(100);
    expect(initial.envelope.state).toBe("unavailable");
    expect(initial.reason).toBe("NOT YET OBSERVED");

    const accepted = {
      envelope: parseStatusEnvelope(currentFixture),
      receivedAtMs: 100,
      reason: null,
    } as const;
    const invalid = invalidResponseView(accepted, 400);
    expect(invalid.envelope.snapshot).toBeNull();
    expect(invalid.reason).toBe("INVALID STATUS RESPONSE");
    const unreachable = unreachableView(accepted, 1_100);
    expect(unreachable.envelope.last_success_age_ms).toBe(1_125);
    expect(unreachable.reason).toBe("LOCAL OBSERVER UNREACHABLE");
  });

  test("renders truthful end-to-end age, command ratio, and unknown reasons", () => {
    const view = {
      envelope: parseStatusEnvelope(currentFixture),
      receivedAtMs: 1_000,
      reason: null,
    } as const;
    const temperature = view.envelope.snapshot?.coolant_temperature;
    expect(temperature).not.toBeNull();
    expect(temperatureAgeNow(temperature!, view, 2_000)).toBe(1_165);

    const producerStaleView = {
      envelope: parseStatusEnvelope(producerStaleFixture),
      receivedAtMs: 2_000,
      reason: null,
    } as const;
    const producerStaleTemperature =
      producerStaleView.envelope.snapshot?.coolant_temperature;
    expect(producerStaleTemperature).not.toBeNull();
    expect(
      temperatureAgeNow(producerStaleTemperature!, producerStaleView, 2_000),
    ).toBe(600);
    expect(commandRatio(9_000)).toBe("0.90");
    expect(
      diagnosticStatusLabel({
        state: "unknown",
        reason: "no_outstanding_command",
      }),
    ).toBe("UNKNOWN — NO OUTSTANDING COMMAND");
  });
});
