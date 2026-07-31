# Celerity runtime decision record

## Objective

Build Celerity as one continuous, hardware-free-verifiable delivery: a host-native Rust vehicle runtime and authority-free sync process; the fixed production v1 controller and Run contracts; an STM32G431 controller firmware and stateful emulator; deterministic, optimized, replay, and simulation control paths; immutable Run storage; the Python home training and artifact application on Kubernetes; automatic startup-only two-slot model lifecycle; generic Linux/systemd integration; a read-only local CLI; and a public SvelteKit architecture walkthrough. Dependency order is an implementation ordering mechanism only. It is not a phased release plan and creates no interim stopping gate.

Everything buildable and testable without physical vehicle hardware is in this work item. Only physical installation and calibration, target-HIL timing/electrical/thermal proof, and road evidence remain external.

## Provenance and repository state

The completed Wayfinder map is [Design the Celerity vehicle control runtime](https://github.com/kellen-miller/celerity/issues/1). It is closed, all child tickets are closed, and its `Not yet specified` section is empty. The resolved child tickets remain the provenance record rather than being copied wholesale here:

- [#2 installed-car inventory](https://github.com/kellen-miller/celerity/issues/2), [#3 CAN FD hardware](https://github.com/kellen-miller/celerity/issues/3), [#4 Haltech/CANTCU telemetry](https://github.com/kellen-miller/celerity/issues/4), and [#5 thermal model family](https://github.com/kellen-miller/celerity/issues/5).
- [#6 safety state machine](https://github.com/kellen-miller/celerity/issues/6), [#7 Run/experiment contract](https://github.com/kellen-miller/celerity/issues/7), [#8 controller conformance](https://github.com/kellen-miller/celerity/issues/8), [#9 model lifecycle](https://github.com/kellen-miller/celerity/issues/9), and [#10 runtime architecture](https://github.com/kellen-miller/celerity/issues/10).
- [#11 acceptance](https://github.com/kellen-miller/celerity/issues/11), [#12 hardware-free simulation/replay](https://github.com/kellen-miller/celerity/issues/12), [#13 deterministic policy](https://github.com/kellen-miller/celerity/issues/13), [#14 implementation languages](https://github.com/kellen-miller/celerity/issues/14), [#15 controller hardware](https://github.com/kellen-miller/celerity/issues/15), and [#16 duct mechanism](https://github.com/kellen-miller/celerity/issues/16).

Planning occurred only in `/Users/kellen/development/github/kellen-miller/celerity/.worktrees/celerity-runtime`, on branch `feat/celerity-runtime`, at base commit `a993404daef379ef8d8943d054d2b3ebddc23870` from `planning/duct-mechanism-contract`. The branch has no upstream.

The selected base contains only `.gitignore`, `README.md`, and `CONTEXT.md`. Read-only branch evidence was inspected from `feat/controller-conformance-prototype`, `research/actuator-controller-hardware`, `research/can-fd-hardware`, `research/haltech-cantcu-telemetry`, and `research/thermal-model-optimizer`. The controller prototype is throwaway evidence for state semantics; it is not production code and must not be merged or cherry-picked. Its provisional `radiator_fraction` vocabulary, symbolic messages, demo timeouts, and lack of exact wire layouts are replaced by the production v1 contract in this work.

## Confirmed decisions

### Product and authority boundary

Celerity observes the Haltech Elite 2500 and CANTCU-owned Classical CAN powertrain bus and never transmits on it. The live powertrain adapter exposes only receive operations and never gives feature or domain code a transmit-capable handle. Before binding, live startup queries the real CAN netdevice and requires it already be configured for 1 Mbit/s Classical CAN with listen-only enabled and automatic bus-off restart disabled; mismatch denies live startup. The runtime does not claim that opening a raw socket configures controller mode. `vcan` proves interface separation and zero application transmission only. Physical on-wire listen-only and the independent fail-silent tap remain HIL evidence. The isolated actuator network is ISO CAN FD at 500 kbit/s nominal and 2 Mbit/s data with bit-rate switching. Hostile nodes are outside the v1 threat model; v1 uses provisioned identity/capability, boot session, generation, lease epoch, monotonic sequence, and the CAN FD link CRC. A changed threat model requires breaking v2, not a translator.

The first mechanism command is `radiator_split_command`: normalized mechanical travel where `0.0` is the calibrated intercooler-side endpoint and `1.0` is the calibrated radiator-side endpoint. It is not measured airflow or measured position. The old `radiator_air_fraction` wording is superseded everywhere, including derived data and public documentation.

The first controller is NUCLEO-G431KB for bench/HIL and STM32G431KB plus native FDCAN and TCAN1044AV-Q1 on a protected installed carrier. Firmware uses stable Rust and current `embassy-stm32`. STM32G474 is only a measured resource-margin fallback. The controller applies PWM positional-servo output using two-point calibration, holds a powered controller-local radiator-protective fallback when authority is absent, and relies on a passive radiator-biased mechanism when power is absent. There is no external position feedback and no invented jam detector.

### Runtime lifecycle

The vehicle has two systemd-supervised host-native Rust processes. `celerityd` owns live authority. `celerity-sync` has no CAN or control authority. One deterministic single-writer event executor owns all mutable control state and makes side effects visible in the order: ingest ordered events, freeze a snapshot, evaluate shared health, reconcile feature health and command source, run eligible prediction/optimization, validate intents, decide authority, build leases/commands, record evidence, then perform output. Features are statically registered and cannot write CAN, renew leases, or grant themselves authority.

Every startup loads one complete immutable typed TOML bundle for exactly one `simulation`, `replay`, or `live` composition. The requested CLI mode must match the bundle. There are no overlays, hot reload, optional wiring, or usable live example configuration before physical commissioning. Simulation, replay, and live vary only at real external seams and execute identical configuration validation, state, feature, safety, model, optimizer, authority, and logging implementation.

Startup begins in controller-local fallback. System, feature, and node state are orthogonal and reconciled through typed conditions. Runtime and Command Leases are distinct. Each accepted `Command` refreshes the Command Lease to the configured Command Lease milliseconds. The controller evaluates Command Lease expiry on its local monotonic clock, independently of Runtime Lease validity; either lease expiring independently selects Controller-Local Fallback. Link restoration and process restart never restore authority; fresh controller truth, configuration reconciliation, leases, and a fresh command are required. Feature-local transient failures reject and continue through bounded reconciliation. Credibly unsafe or shared-trust failures latch global fallback. Controlled shutdown requests fallback, waits only to a configured bounded deadline, stops lease renewal regardless, marks/flushed Run evidence best-effort, notifies systemd, and exits.

Installed features are enabled whenever healthy; there is no driver mode. Observation is normal operation. Only an owner-armed immutable Experiment Plan may command transition-balanced repeated step-and-hold values from `0.10` through `0.90`. Controller acknowledgement plus configured settling defines holds without claiming physical position.

`celerityd.service` uses `Type=notify`, the systemd watchdog, bounded shutdown, and restart into unowned authority. It does not depend on `network-online.target`. Raspberry Pi is not encoded into runtime or deployment contracts. There is no vehicle container stack and no vehicle runtime software A/B mechanism; only the model has two slots.

### Fixed production v1 controller contract

Use standard 11-bit CAN identifiers. Address `0` is reserved; provisioned node addresses are `1..63`. A node-specific identifier is the listed base plus its address. All unlisted identifiers are reserved and ignored unless a later breaking contract version assigns them.

- `0x040 + node`: `FaultReport` (24 bytes).
- `0x080`: broadcast `DiscoveryProbe` (8 bytes); `0x081..0x0ff` are reserved.
- `0x100 + node`: `Command` (24 bytes).
- `0x140 + node`: `CommandAck` (32 bytes).
- `0x180 + node`: `RuntimeLease` (24 bytes).
- `0x1c0 + node`: `RuntimeLeaseAck` (24 bytes).
- `0x200 + node`: `Heartbeat` (32 bytes).
- `0x240 + node`: `FallbackRequest` (16 bytes).
- `0x280 + node`: `FallbackAck` (16 bytes).
- `0x2c0 + node`: `Configuration` (32 bytes).
- `0x300 + node`: `ConfigurationAck` (16 bytes).
- `0x340 + node`: `NodeAnnounce` (32 bytes).
- `0x380 + node`: `CapabilityReport` (24 bytes).
- `0x3c0..0x7ff` are reserved.

`DiscoveryProbe`, `NodeAnnounce`, and `CapabilityReport` are required v1 reconciliation messages, not dynamic enrolment or fleet discovery. The host uses them to compare the provisioned address/identity, controller boot session, lifecycle, protocol/capability generation, active configuration generation, and declared resource bounds before configuration or leases. Discovery grants no authority. It detects reboot and generation drift and preserves statically provisioned multi-node extensibility for later Celerity features without accepting unknown nodes or adding dynamic plugins.

All multi-byte integers are unsigned little-endian. All reserved bytes must be zero on encode and are rejected if nonzero on decode. `radiator_split_command` is encoded as basis points `0..10000`. Enum value `0` is invalid/reserved so zero-filled corruption cannot become a valid state. ISO CAN FD link-layer CRC is the integrity check; v1 deliberately adds no application MAC or redundant payload checksum.

The payload layouts, in byte-offset order, are fixed as follows. `DiscoveryProbe` carries protocol major `u8`, protocol minor `u8`, flags `u16`, and probe sequence `u32`. `NodeAnnounce` carries protocol major `u8`, protocol minor `u8`, lifecycle `u8`, state flags `u8`, boot session `u32`, provisioned identity `u64`, firmware generation `u32`, capability generation `u32`, configuration generation `u32`, and announce sequence `u32`. `CapabilityReport` carries boot session `u32`, capability generation `u32`, resource id `u32`, minimum command `u16`, maximum command `u16`, capability flags `u16`, maximum accepted command rate hertz `u16`, and four reserved zero bytes.

`Configuration` carries boot session `u32`, configuration generation `u32`, controller-local fallback command `u16`, PWM endpoint A microseconds `u16`, PWM endpoint B microseconds `u16`, direction `u8`, flags `u8`, Runtime Lease milliseconds `u16`, Command Lease milliseconds `u16`, heartbeat period milliseconds `u16`, acknowledgement deadline milliseconds `u16`, normal slew basis-points/second `u16`, protection slew basis-points/second `u16`, and configuration digest prefix `u32`. `ConfigurationAck` carries boot session `u32`, configuration generation `u32`, digest prefix `u32`, result `u8`, and three reserved bytes.

`RuntimeLease` carries boot session `u32`, configuration generation `u32`, epoch `u64`, renewal sequence `u32`, validity milliseconds `u16`, and two reserved bytes. `RuntimeLeaseAck` carries boot session `u32`, configuration generation `u32`, epoch `u64`, renewal sequence `u32`, result `u8`, and three reserved bytes. `Command` carries boot session `u32`, configuration generation `u32`, epoch `u64`, command sequence `u32`, `radiator_split_command` `u16`, flags `u8`, and one reserved byte. `CommandAck` carries boot session `u32`, configuration generation `u32`, epoch `u64`, command sequence `u32`, accepted command `u16`, mode `u8`, fault latch `u8`, result `u8`, output state `u8`, Command Lease remaining milliseconds `u16`, and acknowledgement sequence `u32`.

`Heartbeat` carries boot session `u32`, configuration generation `u32`, capability generation `u32`, heartbeat sequence `u32`, current epoch `u64` (zero means none), last accepted command sequence `u32`, accepted command `u16`, and packed state flags `u16`. `FaultReport` carries boot session `u32`, fault sequence `u32`, fault code `u16`, severity `u8`, flags `u8`, related epoch `u64`, and related command sequence `u32`. `FallbackRequest` carries boot session `u32`, epoch `u64`, and request sequence `u32`. `FallbackAck` carries boot session `u32`, request sequence `u32`, accepted command `u16`, mode `u8`, fault latch `u8`, result `u8`, and three reserved bytes.

Production source of truth is the `no_std` Rust codec in `crates/celerity-protocol`, the matching normative `contracts/controller-can-v1.md`, and checked-in byte-for-byte JSON golden vectors under `contracts/golden/controller-can-v1/`. Host runtime, controller emulator, and STM32 firmware all consume the codec. CI detects any disagreement. No provisional emulator protocol, aliases, translators, speculative messages, or compatibility path are permitted.

### Run, control, and model contracts

A Run is one uninterrupted `celerityd` process and monotonic epoch. Its canonical event stream is length-delimited Protobuf v1 inside append-only checksummed chunks. Each event has immutable Run identity, monotonically increasing sequence, run-local monotonic nanoseconds, optional source time, ingestion time, schema version, and a typed payload. Payloads cover raw CAN frames in both directions, decoded observations with units/quality/provenance, conditions, authority transitions, controller exchange, policy/model/optimizer evidence, Experiment events, time anchors, storage degradation, and lifecycle events. A sealed JSON manifest fixes configuration, model, firmware/protocol, decoder, derivation, build, and chunk digests. Incomplete Runs remain explicit and never enter training. Versioned Derivation Specs produce Parquet datasets without rewriting native-rate evidence.

Run storage uses a configured root and minimum-free-space threshold. A bounded writer queue keeps disk I/O off the control path. The runtime never auto-deletes on the control path. Threshold or write failure marks the active Run incomplete and raises best-effort `RunStorageDegraded`; safe control continues. `celerity-sync` alone may delete a locally acknowledged sealed Run under configured retention after the home service has acknowledged its exact digest.

The deterministic duct policy is coolant-first: one typed/versioned 3x3 coolant/IAT table, asymmetric hysteresis, and the shared command shaper used by optimized control. Thresholds, timing, bounds, and slew limits remain required typed commissioning configuration and are not invented defaults. Invalid configuration denies authority.

The learned model is a compact causal multi-output PyTorch temporal convolutional network that predicts future coolant and post-intercooler IAT trajectories from a recent state window plus each candidate split. The home application exports fixed-shape ONNX. `celerityd` uses synchronous, non-cancellable `tract` inference and a Rust finite-control-set optimizer; it does not add a worker or inference service merely to create a cancellation seam. Model eligibility requires startup load/warmup/benchmark evidence and a pre-inference remaining-cycle-budget gate against a measured configured ceiling. A surprising in-flight overrun cannot be interrupted: it is recorded and safely leads to lease expiry/Controller-Local Fallback and/or systemd watchdog restart. Native benchmarking is software evidence; target timing margin remains HIL evidence. Hard coolant envelope/target-band priority is lexicographically ahead of IAT and movement. Artifact/schema parity, finite outputs, the pre-inference budget gate, uncertainty, and OOD gates determine model eligibility. Any failure selects deterministic control when essential inputs/path remain healthy; otherwise controller-local fallback. Model absence is a required offline scenario, not a deferred subsystem.

`celerity-sync` detects the configured home network from Linux network state, uploads only complete Runs to the token-authenticated home API, and downloads desired artifacts while home. The home side is one Python application, one Kubernetes replica, one PVC, SQLite metadata, and filesystem Run/model storage. It decides training inclusion, derives data, retrains/evaluates, and stages a candidate automatically. The vehicle maintains exactly two model slots. A fully validated inactive artifact becomes eligible only at the next `celerityd` startup. There is no hot swap or shadow. Invalidity/demotion makes the known-good artifact desired for a later startup; if no eligible slot exists, deterministic control remains.

Exceptional model-lifecycle events use one configured outgoing HTTPS webhook with a small versioned JSON payload, durable SQLite event record, and bounded best-effort retry. Notification failure never blocks training, evaluation, promotion, demotion, rollback, or job completion. There is no provider framework.

### Local diagnostics, website, and developer contract

`celerityctl` is a read-only Unix-domain-socket client. It shows runtime health, configuration/model generations, controller/lease state, duct conditions, command source, and the recent fault. It has no mutation methods, authority, reset, configuration, experiment controls, TCP/HTTP listener, or driver UI.

The public Svelte/SvelteKit site is durable project documentation with a guided architecture walkthrough. It explains ownership, control flow, failure behavior, data/model lifecycle, validation, and physical boundaries. It is not an in-car UI. Its visual direction may reuse the dark, amber/cyan, technical style demonstrated by the reference prototype, but its implementation is new and describes production architecture rather than embedding throwaway simulation code.

Repository bootstrap establishes the language toolchain before feature work. Checked-in developer commands are the same commands CI invokes. Rust enforces rustfmt, Clippy with warnings denied, host builds/tests, `aarch64-unknown-linux-gnu` vehicle builds, and `thumbv7em-none-eabihf` firmware builds; checked-in editor settings configure rust-analyzer. Python uses `uv` exclusively for dependency/environment/lock/task execution and enforces Ruff lint and Ruff format plus tests. The site uses one formatter (Prettier with its Svelte plugin), one linter (ESLint with Svelte/TypeScript support), `svelte-check`, Vitest tests, and a production build. Lean checked-in scripts expose these commands; no generic task framework or redundant overlapping tools is introduced.

## Module, interface, seam, and adapter shape

The design uses deep modules: each interface hides meaningful policy/sequencing, and every seam has at least two real adapters rather than a test-only abstraction.

- `celerity-protocol` is a deep module whose interface is typed frame encode/decode and validation. It hides IDs, offsets, reserved bytes, ranges, and wire errors. Runtime, emulator, and firmware share it.
- `celerity-runtime` is the authority module. Its primary interface accepts a validated startup bundle plus concrete external adapters, consumes ordered events, and emits typed output effects. It hides lifecycle order, canonical state/history, supervisor/feature/node reconciliation, deterministic/model command selection, lease construction, Experiment execution, and Run evidence generation. Feature policy remains cohesive inside this module rather than split into shallow crates.
- The composition seam has exactly three adapters: live Linux, replay, and simulation. Each supplies the same real external roles: monotonic clock, raw powertrain source, controller transport, platform lifecycle/watchdog, and Run storage. Replay and simulation are not alternate policy implementations.
- The controller seam has live SocketCAN transport and the stateful emulator. The same conformance scenarios later drive firmware over CAN FD. Fault injection occurs by changing adapter input/transport behavior, never by mutating internal authority state.
- Run storage has the production filesystem implementation in temporary or configured roots. Tests use real temporary filesystems and constrained roots/queues rather than an in-memory repository seam.
- Model inference has one production `tract` implementation and real tiny ONNX fixtures. Tests do not add a fake predictor interface.
- Sync's external seams are Linux network state, filesystem spool, and HTTP. Tests use scripted network events, real temporary spool/SQLite state, and a tiny HTTP fixture.
- `celerityctl` crosses one versioned read-only UDS diagnostics interface. The runtime produces an immutable snapshot; the CLI cannot send commands.
- The home application owns HTTP, SQLite, filesystem, training subprocess, model evaluation, and webhook sequencing behind one cohesive application interface. Kubernetes only supervises it.

Required dependencies are constructor parameters at the composition root. No dependency-injection container, service locator, optional dependency that silently disables behavior, dynamic plugin, `helper`/`helpers`/`common`/`util`/`utils` package, test-only interface, `nolint`, or hidden detached task is allowed. Goroutines are irrelevant because this is Rust/Python/TypeScript; Rust threads/tasks, network requests, filesystem writes, retries, subprocesses, and waits must remain visibly named where their lifecycle is owned.

## TDD seams and validation expectations

Implementation uses failing behavior tests at the real interface before each slice. Contract-first tests lock every CAN ID, byte layout, reserved-byte rule, boundary value, and golden vector before runtime or firmware consumption. Runtime scenario tests drive raw encoded frames through controlled time and the stateful emulator and assert semantic traces. Replay tests compare conditions, authority, command source, commands, leases, and reason codes while excluding nondeterministic Run IDs and host latency. Small real PyTorch exports must match `tract` outputs within a declared tolerance. Run tests use real files, queue pressure, minimum-space failures, interrupted chunks, sealing/recovery, and retention ownership. Sync/home tests use real temporary SQLite/filesystem state and a tiny HTTP server. Site tests cover the guided walkthrough and links, then typecheck and build.

Portable tests run natively on macOS and Linux. Linux CI adds focused `vcan` tests for exact raw frames, channel separation, timestamps/filtering, and zero application transmission; it does not claim hardware controller-mode proof. CI cross-builds the vehicle binary for `aarch64-unknown-linux-gnu`; it does not use QEMU merely to test ARM. CI builds the firmware target, validates raw-frame fixtures, and checks a G431-specific FDCAN message-RAM allocation artifact, but native ARM timing, FDCAN electrical behavior, and physical fail-silent behavior remain target-HIL evidence.

Acceptance requires all checked-in developer checks to pass, deterministic scenario/replay results to repeat, systemd lifecycle tests to prove readiness/watchdog/bounded shutdown/restart without authority, the read-only CLI to show the specified snapshot, an end-to-end local Run upload/training/evaluation/stage/startup-activation exercise to pass, webhook failure to leave job state complete, and the public site to build and present the full walkthrough. The plan must keep one software acceptance matrix separate from the physical-only evidence matrix.

## CONTEXT and ADR disposition

`CONTEXT.md` is authoritative project language and remains concise. Implementation updates it only when a new canonical term is introduced, and all code/config/schema/site text must use its existing `Radiator Split Command`, `Radiator-Protective Position`, `Controller-Local Fallback`, and `Passive Fail Position` distinctions.

No ADR is needed for the already accepted mechanism terminology, which issue #16 marks locally reversible before deployment. The fixed v1 wire, Run, configuration, home, and model contracts are repository-owned normative documents under `contracts/`; changing a deployed external contract requires a new major version and a decision record at that time. Internal module choices remain in the living ExecPlan and code unless they become external compatibility commitments.

## Accepted risks and failure modes

- No hardware is currently treated as installed. The software can prove semantics, codec conformance, determinism, cross-builds, and integration, but cannot fabricate electrical, thermal, mechanical, timing-on-target, or road evidence.
- A single non-redundant actuator CAN FD bus can fail as a whole. Local powered fallback and passive mechanism bias bound the result; software does not claim fault tolerance the harness lacks.
- Open-loop PWM acknowledgement is not position feedback. A mechanical jam may be invisible. Physical repeatability and passive return are commissioning gates; insufficient behavior requires mechanism/feedback redesign.
- The lean trust model does not resist a hostile node. A changed threat model requires breaking v2 and explicit cryptographic design.
- Run storage degradation can lose training evidence. The runtime preserves safe control, marks the Run incomplete, never trains it, and reports the typed condition best-effort.
- Home API, Kubernetes, webhook, or network failure can delay upload/training/model availability. None can block or command live control; reconciliation is idempotent.
- Invalid/missing model artifacts remove optimized authority but not deterministic control. Startup validates the two slots without waiting for home.
- Exact policy thresholds, timing, resource limits, model horizons, evaluation tolerances, mechanism calibration, and usable live configuration are evidence-derived required inputs, not architecture defaults. Hardware-free fixtures use clearly marked synthetic test values only.

## Open questions and user judgments

There are no open user judgments blocking the complete hardware-free build. The remaining values and proof listed above are external commissioning evidence, not missing architecture decisions and not reasons to defer any buildable component.

## Source notes

This decision compiles the completed Wayfinder map and linked resolutions, the worktree `README.md` and `CONTEXT.md`, the supplied completed-delivery ledger, AGENTS instructions, and read-only evidence from the five named branches. Where older evidence conflicts, the supplied ledger and later issue #16 language win: `radiator_split_command` replaces `radiator_air_fraction`, the TCN replaces ARX for production, STM32G431 replaces the older Nano/MCP251863 node, model promotion is automatic startup-only without shadow, the website is required, and vehicle runtime software A/B is excluded.
