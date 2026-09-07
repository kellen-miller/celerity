# 0006: Activate learned models only at startup

- Status: Accepted
- Date: 2026-08-29

## Context

Learned control can improve thermal decisions but must not make live authority
depend on the home service, network availability, a model swap race, or an
inference service. Model absence and invalidity are normal offline conditions.

## Decision

The home application trains a fixed-shape ONNX model. `celerityd` loads,
validates, warms, and benchmarks it through synchronous `tract` inference at
startup. A remaining-cycle-budget gate prevents starting inference without
enough measured budget; an unexpected in-flight overrun is recorded and fails
safe through lease expiry or watchdog recovery.

The vehicle keeps exactly two model slots. Sync may stage and validate the
inactive slot, but a model becomes active only during a later `celerityd`
startup. There is no live hot swap or shadow promotion path. Missing, corrupt,
incompatible, nonfinite, uncertain, out-of-distribution, or over-budget models
select deterministic control when essential inputs remain healthy; otherwise
the controller retains local fallback.

The optimizer prioritizes the hard coolant envelope ahead of intake-air
temperature and movement. Exact model shapes and compatibility remain in the
versioned model manifest rather than this ADR.

## Consequences

Model lifecycle is outside live authority, activation is atomic at a process
boundary, and deterministic control remains a complete operating path. Model
updates require a runtime restart before use.

Hot swapping, a separate inference service, vehicle software A/B, and a fake
predictor seam were rejected because they add lifecycle and failure paths
without improving the required safety boundary.

The artifact contract remains normative in the `ModelBundleManifest` message
in [`contracts/celerity/v1/celerity.proto`](../../contracts/celerity/v1/celerity.proto).
