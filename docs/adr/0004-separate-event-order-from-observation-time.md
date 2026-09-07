# 0004: Separate event order from observation time

- Status: Accepted
- Date: 2026-08-29

## Context

Live I/O may ingest a fresh controller frame before a sensor snapshot whose
measurements were observed earlier. One timestamp cannot safely express both
executor order and sensor freshness: treating the older observation as a new
executor time can appear to regress time and latch a false fault.

## Decision

Every event carries current monotonic ingest time for deterministic executor
ordering. Sensor evidence separately retains observation time for freshness
and staleness decisions. Ingest time never derives from a source observation
timestamp.

## Consequences

The executor has one monotonic ordering domain while health logic can reason
about the actual age of measurements. Adapters and Run records must preserve
both values where source time exists.

Using one timestamp was rejected because valid I/O scheduling can then look
like clock regression or fresh data can be mistaken for old data.

The event fields remain normative in
[`contracts/celerity/v1/celerity.proto`](../../contracts/celerity/v1/celerity.proto).
