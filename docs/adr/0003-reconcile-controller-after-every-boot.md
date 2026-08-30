# 0003: Reconcile controller state after every boot

- Status: Accepted
- Date: 2026-08-29

## Context

A controller restart must not silently restore host authority. Persisting
protocol configuration in controller flash would add another configuration
lifecycle while still requiring the host to prove controller identity and
state. The controller must nevertheless establish a protective powered output
before CAN and host reconciliation are available.

## Decision

Controller protocol configuration is volatile. Every reset:

1. establishes the board's protective PWM output independently of protocol
   configuration;
2. creates a fresh nonzero boot session from hardware randomness and rejects
   the immediately prior retained value;
3. starts in Controller-Local Fallback; and
4. requires discovery and complete configuration reconciliation before leases
   or commands can be accepted.

Runtime and Command Leases remain independent. An accepted command refreshes
only the Command Lease. Expiry of either lease selects Controller-Local
Fallback. Link restoration or process restart never restores authority by
itself.

## Consequences

Every reboot is externally visible, stale messages cannot revive authority,
and the protective electrical default does not manufacture a configuration
generation. The host must reconcile after every controller restart.

Flash-backed protocol configuration and automatic authority restoration were
rejected because they create hidden state and weaken reboot recovery semantics.

Message layouts and lease fields remain normative in
[`contracts/controller-can-v1.md`](../../contracts/controller-can-v1.md).
