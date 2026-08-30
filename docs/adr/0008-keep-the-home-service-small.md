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

Use one FastAPI application, one Kubernetes replica, one PVC, SQLite metadata,
and filesystem Run/model storage. SQLite owns durable job and notification
state. Jobs are serialized where shared state requires it; retries and restart
recovery are idempotent.

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
