# 0002: Isolate vehicle authority and powertrain observation

- Status: Accepted
- Date: 2026-08-29

## Context

Celerity must observe the Haltech- and CANTCU-owned powertrain network without
becoming capable of commanding it. Duct actuation needs a separate authority
path with explicit failure behavior. Home synchronization must never acquire
vehicle-control authority.

## Decision

Use separate powertrain-observation and actuator networks. The live powertrain
adapter exposes receive operations only and accepts a real interface only when
Linux reports 1 Mbit/s Classical CAN, listen-only mode, and automatic bus-off
restart disabled. Duct commands use an isolated CAN FD controller network.

Only `celerityd` may own live control authority. `celerity-sync` has no CAN or
controller dependency. One deterministic executor in `control-core` orders
state changes and produces effects; adapters perform those effects visibly at
the composition boundary.

Haltech decoding is part of the configured observation path. CANTCU decoding
is opt-in with an explicit base identifier. Startup rejects invalid ranges and
collisions with supported Haltech broadcast identifiers. Unknown frames remain raw
evidence.

## Consequences

Feature and synchronization code cannot transmit on the powertrain bus or
grant itself authority. Linux `vcan` can prove application-level interface
separation and absence of application transmission, but physical on-wire
silence and the independent fail-silent tap remain HIL evidence.

A shared CAN network, transmit-capable observation socket, and shared runtime
and sync process were rejected because each broadens the authority boundary.

The exact controller wire format remains normative in
[`contracts/controller-can-v1.md`](../../contracts/controller-can-v1.md).
