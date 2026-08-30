# 0007: Run vehicle services directly under systemd

- Status: Accepted
- Date: 2026-08-29

## Context

The vehicle needs observable process ownership, watchdog recovery, bounded
shutdown, and restart into unowned authority. Network availability must not be
a prerequisite for safe local control. A vehicle container stack or a
board-specific runtime would add another lifecycle without solving these
requirements.

## Decision

Run two host-native Rust processes under systemd:

- `celerityd` owns live authority, uses readiness notification and the systemd
  watchdog, stops within a bounded deadline, and restarts into fallback.
- `celerity-sync` owns home reconciliation and can restart independently.

The control service depends on local filesystems, not
`network-online.target`. Configuration and paths target generic Linux rather
than a Raspberry Pi or another specific board. The vehicle has no container
stack and no runtime software A/B mechanism.

## Consequences

Process supervision and side effects remain visible to operators, loss of home
connectivity cannot block control startup, and every restart requires fresh
controller reconciliation. Installation owns host binaries, systemd units,
configuration, and permissions directly.

Containers, network-gated startup, Pi-specific assumptions, and vehicle
software A/B were rejected because they add lifecycle states without a current
control requirement.

Operational installation remains documented in
[`docs/service-install.md`](../service-install.md).
