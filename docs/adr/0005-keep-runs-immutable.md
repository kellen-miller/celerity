# 0005: Keep Runs immutable and deletion acknowledgement-bound

- Status: Accepted
- Date: 2026-08-29

## Context

Control evidence must remain reproducible without allowing storage, upload, or
training work to block safe vehicle control. A digest alone proves byte
identity, not that a Run contains structurally valid canonical events.

## Decision

A Run is one uninterrupted `celerityd` process and monotonic epoch. Store its
canonical events in append-only checksummed chunks and seal them with a
manifest. Interrupted or degraded Runs remain explicitly incomplete and are
excluded from training.

Storage pressure or write failure marks the Run incomplete while control
continues safely. The runtime never deletes Runs. Only `celerity-sync` may
delete a complete local Run after the home service acknowledges its exact
digest and retention policy permits deletion.

The home application validates canonical Run framing, schema, source and
payload cardinality, sequence, monotonic time, and signal validity both at HTTP
admission and corpus loading. Training admission is serialized durably so
retries and restarts converge instead of racing.

## Consequences

Evidence ownership and deletion are explicit, incomplete data cannot silently
enter training, and home/network failures cannot command or backpressure the
control loop. Local storage may retain data longer during outages.

Mutable Runs, runtime-owned retention, and digest-only admission were rejected
because they weaken reproducibility or couple control to external systems.

The canonical event schema remains normative in
[`contracts/run/v1/run.proto`](../../contracts/run/v1/run.proto), and the home
HTTP surface remains normative in
[`contracts/home/v1/openapi.yaml`](../../contracts/home/v1/openapi.yaml).
