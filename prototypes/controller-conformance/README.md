# Controller conformance protocol prototype

> THROWAWAY PROTOTYPE — this is discussion material, not production Celerity
> code or a firmware implementation.

## Question

Does a controller protocol built around provisioned identity, boot-scoped
discovery, versioned configuration, separate Runtime and Command Leases,
monotonic sequences, explicit acknowledgements, heartbeats, and controller-local
fallback behave correctly through startup, message loss, invalid commands,
controller reboot, configuration changes, hard faults, and controlled shutdown?

The prototype models a Vehicle Compute Node and one duct controller. Its pure
state machine uses symbolic CAN FD messages while the terminal shell lets a
human drive difficult transitions. The intentionally accelerated timeouts make
lease behavior easy to exercise by hand; they are not proposed vehicle timing
values.

The prototype does not settle CAN identifiers, byte offsets, encoding, bus
scheduling, authentication, or production Rust module boundaries. Those should
follow only after the state and message semantics feel right.

## Run

From the repository root:

```sh
cargo run --manifest-path prototypes/controller-conformance/Cargo.toml
```

Type one action key and press Enter. A useful first walkthrough is:

1. `g` — attempt authority before discovery and see it denied locally.
2. `d`, then `i`, then `d` — discover the initializing node, complete its
   startup self-test, and refresh observed state.
3. `g`, then `3` — acquire a Runtime Lease and command 30% radiator air.
4. `l`, then `T` — lose the link and advance beyond the Command Lease.
5. `l`, `d`, `g`, `7` — reconcile and recover at 70%.
6. `s` or `e` — inject a stale-sequence or wrong-epoch command.
7. `f`, then `b`, `i` — observe a hard fault, power cycle, and successful
   self-test clearing the ignition-session latch.

## Candidate message families

| Message | Direction | Purpose |
| --- | --- | --- |
| `DiscoveryProbe` | Compute → broadcast | Ask provisioned nodes to announce; grants no authority |
| `NodeAnnounce` | Node → compute | Identity, boot session, protocol, firmware, capability and configuration generations |
| `CapabilityReport` | Node → compute | Declared actuator capability and bounds |
| `Configuration` / `ConfigurationAck` | Compute ↔ node | Activate an immutable generation only outside authority |
| `RuntimeLease` / `RuntimeLeaseAck` | Compute ↔ node | Prove current global permission with boot, epoch, and renewal ordering |
| `Command` / `CommandAck` | Compute ↔ node | Carry and confirm a current-epoch, monotonic actuator setpoint |
| `Heartbeat` | Node → compute | Report readiness, mode, fault latch, lease freshness, and last accepted command |
| `FaultReport` | Node → compute | Report attributable transient or latched controller faults |
| `FallbackRequest` / `FallbackAck` | Compute ↔ node | Request fallback during controlled shutdown without replacing lease expiry |

Acknowledgement means that the controller accepted and electrically applied a
setpoint. It never claims measured physical duct position.
