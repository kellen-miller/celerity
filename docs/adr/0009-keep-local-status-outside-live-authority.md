# 0009: Keep local status outside live authority

- Status: Accepted
- Date: 2026-09-01

## Context

The vehicle needs a local, operator-opened view of live status. Embedding an
HTTP server or UI assets in `celerityd` would join a presentation lifecycle to
live authority. Sending that view through home or sync would add an authority-
unrelated network path to a local status need.

## Decision

Run the Local Status Viewer as a separate, least-privilege `celerity-ui`
process. It may connect to celerityd's read-only diagnostics Unix socket through
a dedicated group and serve read-only status and static assets on loopback. It
has no CAN, controller, configuration, Run-storage, Home-token, or ambient
privileges, and celerityd neither depends on it nor changes its control behavior
when it fails.

Embedding HTTP or UI in `celerityd`, and any Home or sync path for this viewer,
are rejected. This complements [0002: Isolate vehicle authority and powertrain
observation](0002-isolate-authority-and-observation.md) and [0007: Run vehicle
services under systemd](0007-run-vehicle-services-under-systemd.md); it does not
supersede either decision.

## Consequences

The vehicle installation owns a separate observer identity, diagnostics-socket
permissions, and service lifecycle. Status failures remain visible to the
viewer, while observer or browser failure cannot delay, restart, or grant
authority to `celerityd`.
