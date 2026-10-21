# 0008: Keep the home service operationally small

- Status: Accepted
- Date: 2026-08-29

## Context

The home system archives Runs, trains and evaluates models, stages artifacts,
and reports exceptional outcomes. It serves one vehicle and is outside the
live authority path. A distributed queue, object store, database service, or
model microservice would add failure and ownership boundaries before scale
requires them.

## Decision

Use one Rust HTTP application, one Kubernetes replica, one PVC, SQLite metadata,
and filesystem Run/model storage. Rust owns uploads, database migration, durable
job state, model staging and webhook delivery. Python owns numerical data
preparation, PyTorch training, ONNX export and evaluation through an immutable
request/result file contract. A detached Rust worker claims each queued job
atomically before launching Python and fences terminal writes by process identity.
Python inherits a filesystem admission lock across exec. Recovery retains the
active slot until numerical work exits, including supervisor death and spawn
races. SQLite owns durable job and notification state. If the desired model
changes while training runs, the candidate finishes without changing the desired
model. Jobs are serialized; retries and restart recovery are idempotent.

Package the workload as a Celerity-owned Helm chart. Infrastructure supplies
environment-specific values and an existing 1Password-backed
`ClusterSecretStore`; the chart renders an ExternalSecret rather than static
secret values. One configured HTTPS webhook reports exceptional model events,
and bounded delivery failure never changes job or model outcomes.

## Consequences

The deployment has one obvious persistence and process boundary and can be
recovered from its PVC. It intentionally does not scale horizontally; evidence
of real contention or availability requirements must precede a redesign.

A queue, object store, PostgreSQL, multiple replicas, provider framework, and
separate model service were rejected because the current single-vehicle load
does not justify their coordination cost.

Operational deployment remains documented in
[`docs/home-deployment.md`](../home-deployment.md).
