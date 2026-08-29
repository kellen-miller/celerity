# 0001: Name Rust crates by responsibility

- Status: Accepted
- Date: 2026-08-29

## Context

Celerity is the overall system and product name. Repeating it across package
names obscured the role of each package and made the system boundary look like
a collection of branded subsystems. Splitting every runtime concept into a
crate would create shallow interfaces and hide the control lifecycle.

## Decision

Use responsibility-oriented Rust package names:

- `control-protocol` owns the exact shared CAN codec.
- `control-core` owns configuration, decoded observations, ordered state,
  policy, authority, model execution, Runs, simulation, and replay.
- `vehicle-runtime` owns Linux adapters and executable composition.
- `sync` owns authority-free home reconciliation.
- `duct-controller` owns controller firmware.

Keep `celerity`, `celerityd`, `celerityctl`, and `celerity-sync` as user-facing
executable names. Keep cohesive runtime concepts as modules inside
`control-core` until a real ownership boundary requires another crate.

## Consequences

Package names communicate ownership without repeating the system name. Binary
and service interfaces remain stable. Dependency direction is explicit:
protocol and core are reusable below vehicle composition, while sync has no
control or CAN dependency path.

The alternatives were brand-prefixed package names, one large package, and
many noun-sized crates. They were rejected because they respectively obscure
role, mix platform composition with policy, or create shallow indirection.
