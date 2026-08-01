# Build the Celerity vehicle control runtime end to end

This ExecPlan is a living document. The sections `Progress`, `Surprises & Discoveries`, `Decision Log`, and `Outcomes & Retrospective` must be kept current as implementation proceeds. Maintain this document in accordance with `.agent/PLANS.md` from the repository root.

This is one continuous implementation effort through every component that can be built and verified without physical vehicle hardware. The milestone headings below are non-stopping dependency milestones: they express the order in which later code depends on earlier code, not phases, releases, deferments, approval gates, or acceptable stopping points. Continue automatically from one milestone into the next until the full hardware-free acceptance surface passes. Physical installation/calibration, target-HIL electrical and timing proof, thermal proof, and road proof are the only external work.

## Purpose / Big Picture

After this work, the repository can build and exercise the real Celerity control lifecycle on a developer machine: start in controller-local fallback, ingest raw powertrain frames, reconcile a stateful controller, use deterministic duct protection while a real ONNX model is absent or ineligible, use `tract` prediction plus a finite-control-set optimizer when eligible, record an immutable Run, replay it deterministically, upload it through `celerity-sync`, train/evaluate/stage a model in the home application, and activate a validated model only at the next runtime startup. A reader can inspect the same architecture through a public SvelteKit walkthrough. Linux CI proves `vcan`, systemd lifecycle, ARM64 cross-build, and embedded firmware build/conformance without pretending that QEMU or simulation proves vehicle hardware.

The repository is greenfield. The main complexity risk is therefore not legacy code but accidental concept proliferation: separate simulation policy, duplicated wire types, shallow crates, optional wiring, hidden side effects, or multiple configuration paths. This plan concentrates complexity behind four deep modules: the fixed controller codec, the single-writer runtime, the Run format/writer, and the home application. Their small interfaces keep wire offsets, lifecycle sequencing, storage recovery, and training/job sequencing out of callers. Live, replay, and simulation adapters vary only at genuine external seams.

## Progress

- [x] (2026-07-31 17:45Z) Compiled the completed Wayfinder decisions and branch evidence into `.agent/work/celerity-runtime/decision.md`.
- [x] (2026-07-31 17:45Z) Added repository-owned `.agent/PLANS.md` and initialized planning metadata.
- [x] (2026-07-31 18:34Z) Established pinned Rust/uv/npm toolchains, developer checks, CI jobs, strict v1 contracts, examples, and repository skeleton.
- [x] (2026-07-31 19:52Z) Implemented and proved the controller codec, stateful emulator, and STM32G431 Embassy firmware against the same raw golden vectors.
- [x] (2026-07-31 19:53Z) Implemented the typed startup bundle, single-writer safety lifecycle, deterministic duct control, read-only diagnostics, and qualified live Linux CAN boundaries.
- [x] (2026-07-31 19:53Z) Implemented canonical Run storage, bounded Experiment Plans, simulation, replay, systemd integration, and hardware-free fault scenarios.
- [x] (2026-07-31 19:53Z) Implemented PyTorch TCN export/evaluation, ONNX/`tract` parity, Rust optimizer/model eligibility, and startup-only two-slot lifecycle.
- [x] (2026-07-31 20:04Z) Implemented authority-isolated sync, durable transfer journal, exact acknowledgement/retention, authenticated home API/job lifecycle, model staging, exceptional lifecycle notifications, and webhook-failure independence.
- [x] (2026-07-31 19:13Z) Added the one-replica PVC Helm deployment, public accessible SvelteKit walkthrough, operator/deployment/configuration docs, and explicit physical evidence checklist.
- [x] (2026-07-31 20:58Z) PR #17 head `87d5d49` passed the initial GitHub Actions run `30664026585`: Linux including ARM64 link/vCAN/systemd in 12m28s, macOS portable in 5m36s, and Ubuntu portable in 5m21s.
- [x] (2026-07-31 21:03Z) The sole normal closeout review found that the production daemon did not execute the runtime lifecycle and that model selection could not reach an actuator command; implementation was reopened rather than accepting component-only evidence.
- [x] (2026-07-31 22:32Z) Integrated the live CAN/controller/Run loop, ACK-backed model command and promotion evidence, canonical Run-to-training records, automatic home training, exact bundle staging/startup selection, startup-only tract inference, bounded experiments, persistent invalid-model recovery, and two-slot rollback preservation. The complete local Rust, Python, production e2e, artifact, Helm, and SvelteKit gates pass.
- [x] (2026-07-31 22:52Z) Fresh Linux CI exposed and native-Linux reproduction confirmed that accepted command acknowledgements did not refresh controller truth and that configured heartbeat timing was absent from emulator/firmware state. The runtime now treats a valid ACK as fresh exact-command truth, and both controller implementations use the accepted heartbeat period. Full host Rust and native-Linux Clippy/tests, including the production vCAN composition, pass.
- [x] (2026-08-01 00:10Z) PR #17 head `e55df1c` passed fresh GitHub Actions run `30673118789` across Linux vCAN/systemd/ARM64, Ubuntu portable, and macOS developer checks.
- [x] (2026-08-01 00:10Z) The sole Claude implementation adversarial review completed with eight findings (two high, four medium, two low). All were verified and resolved: separate ingest/observation time, fresh hardware-random boot sessions, explicit volatile controller configuration, fail-closed baseline evaluation, canonical Run admission validation, serialized durable home jobs, firmware/emulator equivalence, and constant-time token comparison. Host and native Linux vCAN validation pass on the corrected working tree.
- [ ] Commit and publish the dispositioned review fixes, pass fresh GitHub Actions on that exact head, update the final hardware-free evidence, and finish the Lavish implementation ownership loop.

## Surprises & Discoveries

---FRONTEND_IMPLEMENTATION_STATUS---
implementer: gpt-5.6-sol implementation agent
status: changed
changed_files: site/src/routes/+page.svelte, site/src/site.test.ts
docs_evidence: docs/validation/hardware-free-report.md and public evidence-boundary content
browser_evidence: in-app Chromium at 1440x900 and 390x844; screenshots inspected; no horizontal overflow; mobile grids collapsed; one h1/four h2; header/nav/main/footer present; seven native keyboard links plus skip link at tabIndex 0; explicit cyan focus ring; clicked and direct-reload hashes land 62px below sticky header; reduced-motion rule changes scroll behavior to auto; console clean; initial favicon 404 removed with inline favicon
validation: npm format, ESLint, svelte-check, 2 Vitest tests, static production build, rendered DOM/viewport/deep-link/console inspection
critical: 0
high: 0
medium: 0
low: 0

- Observation: The selected base commit contains only `.gitignore`, `README.md`, and `CONTEXT.md`; all code paths are new.
  Evidence: `git ls-tree -r --name-only a993404daef379ef8d8943d054d2b3ebddc23870` lists those three files.
- Observation: The controller prototype proves state semantics but explicitly does not settle CAN identifiers, byte encoding, bus scheduling, authentication, or production Rust module boundaries.
  Evidence: `git show feat/controller-conformance-prototype:prototypes/controller-conformance/README.md` labels it throwaway and names those unresolved items.
- Observation: Older research contains superseded choices: Nano/MCP251863, production ARX, `radiator_air_fraction`, shadow/human promotion, and vehicle software A/B.
  Evidence: the completed Wayfinder resolution and `decision.md` select STM32G431, TCN/ONNX/`tract`, `radiator_split_command`, automatic startup-only activation with no shadow, and model-only A/B.
- Observation: The macOS implementation host has no host systemd or ARM64 GNU linker, so local validation correctly omitted exact service lifecycle, `vcan`, and ARM64 link claims; the pinned Ubuntu CI job supplied that evidence.
  Evidence: PR #17 head `87d5d49`, GitHub Actions run `30664026585`, passed the full Linux job in 12m28s after the portable macOS and Ubuntu surfaces also passed.
- Observation: Current FastAPI/Starlette emits a deprecation warning for its compatibility `TestClient`; it does not affect the eleven passing Python tests and is upstream of this application.
  Evidence: `./scripts/check-python` passes with the warning originating from the installed `fastapi/testclient.py` compatibility import.
- Observation: A component-only authority test can pass while the production CAN loop still loses authority if evidence freshness is not refreshed by the real acknowledgement path.
  Evidence: GitHub Actions runs `30670359829`, `30670530044`, `30670650014`, and `30670753577` progressively reached the Linux vCAN test; native Linux reproduction showed ModelOptimized authority on cycle 1 followed by Controller-Local Fallback when the configuration acknowledgement aged out despite fresh accepted CommandAck frames.
- Observation: One timestamp cannot safely represent both when an event enters the executor and when its sensor values were observed.
  Evidence: The implementation review traced controller frames drained at the current cycle time followed by an input snapshot carrying an older observation time; the executor interpreted that valid sequence as time regression. `earlier_signal_observation_does_not_regress_ingest_time` now proves the two clocks remain distinct.
- Observation: An HTTP-level digest check is insufficient Run admission evidence because a valid digest can faithfully identify structurally invalid protobuf records.
  Evidence: The new canonical Run v1 decoder rejects malformed framing, unsupported schema major, missing sources, invalid payload cardinality, non-increasing sequence, and regressing monotonic time before a Run enters SQLite or the training corpus.
- Observation: A compiled safe PWM value and protocol configuration are different kinds of state; preloading the latter to obtain the former silently made a fresh controller authority-eligible before reconciliation.
  Evidence: The final consistency pass found firmware initialized generation 1 before receiving Configuration. Firmware now leaves `configuration_generation()` absent, preserves the already-powered compiled fallback PWM until Configuration arrives, and `unconfigured_controller_rejects_authority` proves leases and commands are denied.

## Decision Log

- Decision: Deliver all hardware-free work continuously; use milestone boundaries only for dependency ordering and verification.
  Rationale: The user explicitly rejected staged releases and deferment of any buildable subsystem.
  Date/Author: 2026-07-31 / planning agent.
- Decision: Make `crates/celerity-protocol`, `crates/celerity-runtime`, and the home application deep modules; keep runtime domain areas as modules, not symmetry-driven crates.
  Rationale: This hides wire mechanics, authority sequencing, and job/storage policy behind small interfaces and keeps the main lifecycle readable.
  Date/Author: 2026-07-31 / planning agent.
- Decision: Use checked-in shell entrypoints for language checks and invoke them unchanged from CI.
  Rationale: Rust, `uv`, and npm keep their native tools while CI and developers share one obvious command per language without adding a generic task framework.
  Date/Author: 2026-07-31 / planning agent.
- Decision: Use standard 11-bit CAN IDs with 64-address family ranges and fixed CAN FD payload sizes.
  Rationale: The scheme makes priority and ownership visible, supports the single-vehicle node count, and lets one `no_std` codec serve runtime, emulator, and firmware without translation.
  Date/Author: 2026-07-31 / planning agent.
- Decision: Do not introduce an interface solely for a fake predictor or in-memory Run repository.
  Rationale: Real ONNX fixtures and real temporary files exercise the production seams with less indirection and prevent tests from drifting into an alternate system.
  Date/Author: 2026-07-31 / planning agent.
- Decision: Package the home application as a split-template Helm chart and source its token and webhook URL through one ExternalSecret backed by an existing 1Password `ClusterSecretStore`.
  Rationale: Celerity retains ownership of its workload resources while infrastructure supplies environment-specific values and secret-store identity without static Kubernetes Secrets in this repository.
  Date/Author: 2026-07-31 / implementation agent.
- Decision: Count a validated CommandAck as fresh controller truth and drive controller heartbeats from the accepted Configuration period.
  Rationale: A CommandAck proves the exact boot session, generation, epoch, sequence, accepted command, and electrical application; ignoring that evidence caused false fallback, while hard-coded heartbeat timing contradicted the fixed CAN contract.
  Date/Author: 2026-07-31 / orchestrator.
- Decision: Keep executor ingest time and sensor observation time as separate fields.
  Rationale: Ingest time defines deterministic event ordering; observation time defines data staleness. Combining them made normal live I/O ordering look like a clock regression and could latch a false hard fault.
  Date/Author: 2026-08-01 / orchestrator.
- Decision: Make controller configuration explicitly volatile and generate a fresh nonzero boot session from hardware RNG while rejecting the immediately prior backup-domain value.
  Rationale: Reconciliation already requires configuration after every boot, so composing flash persistence would add a second lifecycle without improving authority recovery. The fresh session makes every reset visible and keeps the controller in local fallback until reconciliation completes.
  Date/Author: 2026-08-01 / orchestrator.
- Decision: Keep the pre-configuration safe PWM as an explicit board constant, independent from protocol configuration state.
  Rationale: The controller must establish a powered protective output before RNG and CAN are ready, but that electrical default must not manufacture a configuration generation or permit authority before host reconciliation.
  Date/Author: 2026-08-01 / orchestrator.
- Decision: Validate canonical Run structure at HTTP admission and serialize home training admission with one SQLite immediate transaction and one-active-job uniqueness rule.
  Rationale: Bad Runs should be rejected individually before corpus use, and duplicate/restarted home work should converge idempotently instead of racing or colliding on deterministic model digests.
  Date/Author: 2026-08-01 / orchestrator.

## Outcomes & Retrospective

The source implementation is complete on PR #17 and now exercises the controller protocol and equivalent firmware/emulator state machines, production live single-writer runtime and fault behavior, immutable and admission-validated Run/replay lifecycle, production ONNX/`tract` model path, authority-isolated synchronization, serialized and restart-aware home training/model lifecycle, bounded exceptional notifications, startup-only two-slot activation, systemd/vCAN/ARM64 Linux integration, and the public architecture walkthrough. The sole implementation adversarial review is complete and all eight findings are dispositioned. Workflow acceptance remains active until the dispositioned head passes fresh CI, final evidence names that exact head, and the implementation ownership session ends.

The home deployment is a versioned Helm chart with separate Deployment, Service, PVC, ConfigMap, and ExternalSecret templates. Its required store value references a pre-existing External Secrets Operator 1Password `ClusterSecretStore`; no static Kubernetes Secret is rendered. Chart lint, render, and package checks pass, but cluster installation remains a deployment action rather than simulated evidence. The remaining acceptance work is the explicitly external physical/HIL matrix: installed-bus decoding and fail-silence, electrical and target timing, servo/mechanism calibration, thermal/model envelope proof, and road/track validation.

## Context and Orientation

`CONTEXT.md` defines four terms that must remain distinct. `radiator_split_command` is normalized mechanical travel, not airflow and not position feedback. `Radiator-Protective Position` is the highest physically validated radiator-side position. `Controller-Local Fallback` is powered controller behavior when remote authority is absent. `Passive Fail Position` is mechanical behavior when controller power is absent. Remove the obsolete `radiator_air_fraction` wording from every new source, schema, fixture, UI, and document.

Haltech owns the rotary engine and CANTCU owns the transmission. Celerity only receives their existing Classical CAN traffic. It controls its own separate ISO CAN FD actuator bus at 500 kbit/s nominal and 2 Mbit/s data with bit-rate switching. Live powertrain code must be structurally unable to transmit: it gets an RX-only type with no send method. Before binding the RX-only raw socket, live startup queries and requires the real netdevice already be configured for 1 Mbit/s Classical CAN with `LISTEN-ONLY` and `restart-ms 0`; raw-socket open does not set controller mode. Linux `vcan` proves only zero application transmission and interface separation.

The vehicle process boundary is `celerityd` for authority and `celerity-sync` for home reconciliation. `celerityctl` is a read-only Unix-socket diagnostics client. The public site is documentation, not a driver UI. The home side is one Python application in one Kubernetes replica with a PVC, SQLite, and filesystem storage. Raspberry Pi is merely a candidate first Linux machine; use no Pi-specific runtime assumptions.

Create this repository shape. `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/config.toml`, `.vscode/settings.json`, and `.vscode/extensions.json` define Rust and rust-analyzer behavior. `scripts/check-rust`, `scripts/check-python`, `scripts/check-site`, and `scripts/check-linux` are small, readable checked-in developer commands. `.github/workflows/ci.yml` calls those scripts rather than copying their tool invocations.

`contracts/controller-can-v1.md` is the normative wire description, `contracts/run/v1/run.proto` is the canonical event schema, `contracts/home/v1/openapi.yaml` is the vehicle/home HTTP contract, and `contracts/golden/` contains byte-level and JSON examples. `config/schema-v1.md` and `config/examples/{simulation,replay}.toml` explain and exercise typed startup bundles; do not add a live example before commissioning. Rehome the durable accepted research evidence as `docs/research/{can-fd-hardware,haltech-cantcu-telemetry,thermal-model-optimizer,actuator-controller-hardware}.md` by extracting it read-only from the named branches, adding a supersession header where STM32G431, TCN, or later terminology replaced an older conclusion. This preserves primary-source evidence without merging branch history or presenting obsolete choices as current.

The minimal Cargo workspace has `crates/celerity-protocol`, `crates/celerity-runtime`, `crates/celerity`, `crates/celerity-sync`, and `firmware/duct-controller`. `celerity-protocol` is `no_std` and owns exact CAN encode/decode. `celerity-runtime` owns configuration types, ordered events, canonical vehicle state/history, System Supervisor, feature reconciliation, duct policy, model inference wrapper, optimizer, controller reconciliation, Experiment execution, Run writing, simulation/replay, and diagnostics snapshots. Keep these as cohesive Rust modules under `crates/celerity-runtime/src/`; do not turn each noun into a crate.

The `crates/celerity` package contains the thin `celerityd`, `celerity`, and `celerityctl` binaries plus concrete Linux adapters. `celerity simulate <scenario.toml>` and `celerity replay <run> --verify|--explore` use the production runtime. `crates/celerity-sync` is a separate binary and library with no dependency that exposes controller transport. `firmware/duct-controller` uses the shared protocol crate plus `embassy-stm32` for STM32G431 FDCAN, timers/PWM, flash, and watchdogs.

`services/home` is the `uv`-managed Python project. Put the cohesive application package under `services/home/src/celerity_home/` with HTTP routes, SQLite schema/migrations, Run admission/derivation, training/evaluation, artifact storage, job ownership, and webhook delivery in files named for those domain concepts. Do not create `utils.py`, `helpers.py`, or generic repository/service layers. `deploy/helm/celerity-home/` contains the single-replica Helm chart with its Deployment, Service, PVC, Secret/ConfigMap references, probes, and resource bounds. `site/` is the SvelteKit static public site.

## Fixed external contracts

The controller contract is v1 immediately; the emulator is not allowed a provisional language. Use standard IDs with node addresses `1..63` and address `0` reserved. The family allocation is `FaultReport=0x040+node`, `DiscoveryProbe=0x080`, `Command=0x100+node`, `CommandAck=0x140+node`, `RuntimeLease=0x180+node`, `RuntimeLeaseAck=0x1c0+node`, `Heartbeat=0x200+node`, `FallbackRequest=0x240+node`, `FallbackAck=0x280+node`, `Configuration=0x2c0+node`, `ConfigurationAck=0x300+node`, `NodeAnnounce=0x340+node`, and `CapabilityReport=0x380+node`. Reserve `0x081..0x0ff` and `0x3c0..0x7ff`. All frames use ISO CAN FD, standard IDs, fixed lengths, little-endian unsigned integers, zero reserved bytes, and basis points `0..10000` for split commands. Enum zero is invalid. The ISO CAN FD CRC is the v1 integrity mechanism; add no MAC, payload checksum, translator, alias, or speculative message.

The exact fixed payloads are:

- `DiscoveryProbe` 8 bytes: protocol major `u8`, minor `u8`, flags `u16`, probe sequence `u32`.
- `NodeAnnounce` 32 bytes: major `u8`, minor `u8`, lifecycle `u8`, flags `u8`, boot session `u32`, identity `u64`, firmware generation `u32`, capability generation `u32`, configuration generation `u32`, announce sequence `u32`.
- `CapabilityReport` 24 bytes: boot session `u32`, capability generation `u32`, resource id `u32`, minimum command `u16`, maximum command `u16`, capability flags `u16`, maximum accepted command rate hertz `u16`, four zero bytes.
- `Configuration` 32 bytes: boot session `u32`, configuration generation `u32`, fallback command `u16`, PWM endpoint A/B microseconds `u16` each, direction `u8`, flags `u8`, Runtime Lease/Command Lease/heartbeat/ack deadline milliseconds `u16` each, normal/protection slew basis-points per second `u16` each, digest prefix `u32`.
- `ConfigurationAck` 16 bytes: boot session `u32`, configuration generation `u32`, digest prefix `u32`, result `u8`, three zero bytes.
- `RuntimeLease` 24 bytes: boot session `u32`, configuration generation `u32`, epoch `u64`, renewal sequence `u32`, validity milliseconds `u16`, two zero bytes. `RuntimeLeaseAck` replaces the final four bytes with result `u8` and three zero bytes.
- `Command` 24 bytes: boot session `u32`, configuration generation `u32`, epoch `u64`, command sequence `u32`, split command `u16`, flags `u8`, one zero byte.
- `CommandAck` 32 bytes: boot session `u32`, configuration generation `u32`, epoch `u64`, command sequence `u32`, accepted command `u16`, mode `u8`, fault latch `u8`, result `u8`, output state `u8`, Command Lease remaining milliseconds `u16`, acknowledgement sequence `u32`.
- `Heartbeat` 32 bytes: boot session/configuration generation/capability generation/heartbeat sequence `u32` each, epoch `u64` with zero meaning none, last command sequence `u32`, accepted command `u16`, state flags `u16`.
- `FaultReport` 24 bytes: boot session `u32`, fault sequence `u32`, fault code `u16`, severity `u8`, flags `u8`, related epoch `u64`, related command sequence `u32`.
- `FallbackRequest` 16 bytes: boot session `u32`, epoch `u64`, request sequence `u32`. `FallbackAck` is boot session `u32`, request sequence `u32`, accepted command `u16`, mode `u8`, fault latch `u8`, result `u8`, three zero bytes.

Each accepted `Command` refreshes the Command Lease to exactly the configured Command Lease milliseconds. The controller measures that deadline with its own monotonic clock. Command Lease validity is independent of Runtime Lease validity, and expiration of either lease independently selects Controller-Local Fallback. A rejected, duplicate, stale, wrong-session, wrong-generation, or wrong-epoch command does not refresh the deadline. Make this rule normative in `contracts/controller-can-v1.md` and prove the specific case where the Command Lease expires while the Runtime Lease remains valid.

Keep `DiscoveryProbe`, `NodeAnnounce`, and `CapabilityReport`. They are not dynamic enrolment. They reconcile a statically provisioned node's address and identity, boot session, lifecycle, protocol/capability generation, active configuration generation, and resource bounds before configuration or leases. Discovery grants no authority and unknown identities are rejected. The handshake detects reboot and generation drift and supports later statically configured Celerity feature nodes without fleet machinery, dynamic plugins, or runtime acceptance of an unprovisioned controller.

The Run v1 schema is length-delimited Protobuf inside append-only chunks whose checksum covers exact stored bytes. `RunEvent` fixes field numbers `schema_major=1`, `schema_minor=2`, `run_id=3`, `sequence=4`, `monotonic_ns=5`, optional `source_monotonic_ns=6`, `ingestion_monotonic_ns=7`, `source=8`, and a payload `oneof` using fields `20..29`: raw CAN, signal observation, condition transition, authority transition, control decision, Experiment event, lifecycle event, storage event, time anchor, and annotation. Raw CAN preserves channel, direction, ID, standard/extended, FD/BRS/ESI flags, exact bytes, hardware receive timestamp when available, and overflow/drop counters. Signal observations preserve typed signal, value, canonical unit/reference, `Valid|Invalid|Unknown`, reason, source-event sequence, decoder generation, and age. Control decisions preserve command source, configuration/model generations, input-window reference, candidate predictions/rejections, chosen split, constraint margins, uncertainty/OOD evidence, latency, leases, and command/ack sequences. A Run manifest is sealed JSON with build, protocol, firmware, config, decoder, model, derivation, platform, clock-anchor, chunk sequence/range/checksum, completion, and predecessor identities. Any truncation, free-space threshold breach, queue overflow, or write failure makes `completion=incomplete`; incomplete Runs never train.

The startup TOML v1 has required top-level `schema_version`, `generation`, `mode`, `runtime`, `powertrain`, `controllers`, `duct`, `model`, `run_storage`, `diagnostics`, `sync`, and `composition`. `mode` is exactly one of `simulation`, `replay`, or `live`; `composition` is a tagged enum with exactly the matching subsection. A simulation composition names one scenario. Replay names one Run and verify/explore mode. Live names logical powertrain-RX and actuator-CAN roles, their concrete Linux interfaces, platform/watchdog behavior, and commissioned identities; no live example ships and no code assumes those roles enumerate as `can0`/`can1`. `sync` declares spool ownership, retention, home interface, expected default gateway, home API URL, and credential path. Linux route/link state says "home" only when that interface is up and its default route uses the configured gateway; periodic HTTP retry recovers missed notifications but does not redefine home presence. Configuration validation rejects unknown keys, missing required dependencies, duplicate actuator ownership, timing/ordering violations, invalid monotonic 3x3 policy tables, split/slew outside code absolute bounds, and any mode mismatch. Environment variables may supply only deployment paths and secrets, never control behavior. `ValidatedBundle::load` is the single parse/normalize/validate path used by all binaries; do not create separate create/update/generated normalization variants.

The home API is token-authenticated and vehicle-initiated. `POST /v1/reconcile` sends completed local Run digests plus active/staged/rejected model digests and returns missing Run digests and the desired model digest. `PUT /v1/runs/{run_digest}/chunks/{chunk_digest}` idempotently stores exact chunk bytes. `POST /v1/runs/{run_digest}/complete` validates the manifest and returns the acknowledged full Run digest. `GET /v1/models/{digest}` streams an immutable bundle. No route commands the vehicle, changes runtime configuration, or mutates authority. Exceptional webhook JSON is `{schema_version,event_id,event_type,occurred_at,job_id,artifact_digest,summary}`; delivery state and attempts are durable, bounded, and never control the owning job.

## Interfaces, modules, seams, and adapters

The `celerity-protocol` interface is typed `encode(frame, &mut [u8; 64]) -> EncodedFrame` and `decode(can_id, payload) -> Result<Frame, WireError>`. The module hides IDs, offsets, lengths, reserved-byte checks, ranges, enum decoding, and basis-point conversion. Golden-vector tests call only this interface. Runtime, emulator, and firmware depend directly on it, so deleting it would duplicate wire knowledge in three places.

The `celerity-runtime` external interface is one `Runtime::new(ValidatedBundle, ExternalAdapters)` constructor and one explicitly side-effecting `Runtime::run()` lifecycle. `ExternalAdapters` requires a monotonic clock, RX-only powertrain source, controller transport, platform lifecycle/watchdog, and configured Run filesystem. Construction fails if any required role is absent. Internally keep one `executor.rs` method whose readable cycle visibly performs observation application, snapshot freeze, shared-health gate, duct feature reconciliation, prediction/optimization, intent validation, authority/lease construction, evidence enqueue, and output. Do not split each line into a single-use helper merely to shorten the method.

The external seams are real because behavior varies there. The monotonic-clock seam has host and controlled adapters. Powertrain has live RX-only SocketCAN, Run replay, and scenario raw-frame encoder adapters. Controller transport has SocketCAN and the stateful in-process controller emulator. Platform lifecycle/watchdog has systemd and controlled adapters. Run storage always uses the production filesystem writer, with temporary/constrained roots in tests. Network detection in sync has Linux route/network state and scripted controlled adapters. Home HTTP uses the production client against the real home app or a tiny HTTP fixture. Fault injection changes boundary frames, timing, filesystem behavior, process lifetime, and HTTP behavior; there is no internal `force_authority` or test-only interface. An architecture test over `cargo metadata` also asserts that `celerity-sync` has no dependency path to `celerity-runtime`, `celerity-protocol`, or SocketCAN crates; it exchanges only immutable filesystem and HTTP artifacts.

The duct feature interface receives one immutable canonical snapshot plus current controller truth and returns typed conditions and one intent. Its implementation owns the deterministic policy, model eligibility, `tract` wrapper, optimizer, command-source selection, and Experiment override. The shared command shaper owns clamp-to-validated-range, deadband, normal/protection slew, handoff from last acknowledged command, and command cadence. Only the controller reconciler turns an accepted intent into leases and frames.

The deterministic policy is a coolant-first 3x3 coolant/IAT table with asymmetric entry/release thresholds. Protection entry occurs at the next cycle; release requires configured dwell. Both deterministic and optimized paths use the same shaper. Invalid/stale essential coolant or IAT removes Celerity authority. An exceeded thermal constraint while already at the maximum validated radiator allocation raises `ThermalConstraintViolated` and holds protection; Haltech retains engine protection.

The controller emulator is a stateful protocol peer, not an ACK mock. It owns provisioned identity, boot session, initialization/readiness, immutable configuration generation, Runtime Lease epoch/renewal, accepted-Command refresh of the independent Command Lease, output mode, accepted split, heartbeat, fault latch, lease expiry, reboot, and fresh-authority rules. Delay/drop/reorder/reject/reboot/fault are external scripted behaviors around this state machine. The same conformance cases feed encoded frames to emulator and firmware, including a Command Lease expiry while the Runtime Lease remains valid.

The Run writer owns a bounded queue and one writer worker. `enqueue` is non-blocking from the executor and returns an explicit accepted/degraded result. The worker creates checksummed segments, fsyncs at declared boundaries, seals manifests atomically, and recovers partial Runs as incomplete. It checks configured minimum free space before opening/rotating a chunk, never auto-deletes, and emits `RunStorageDegraded` best-effort through a reserved emergency path. Sync owns retention only after exact server acknowledgement.

The diagnostics module builds an immutable read-only snapshot. The Unix-socket server supports one versioned `GetStatus` request and no generic method name or mutation envelope. The snapshot contains runtime health, config/model generation, controller boot/config/lease truth, duct conditions and acknowledged split, command source, and recent fault. `celerityctl status` renders it and cannot arm, reset, stage, or write.

The model bundle contains ONNX bytes, SHA-256 digest, ABI/schema, signal order and units, normalization, sample period, history length, horizons, output order, residual/uncertainty calibration, OOD support, command lattice, compatibility, training lineage, and evaluation report. The TCN is causal and multi-output; use small residual Conv1d blocks with only past padding and make channel count/kernel/dilation/horizons part of the bounded versioned home recipe. Export fixed shapes through the current PyTorch exporter. Rust `tract` consumes only typed input windows and returns typed thermal trajectories synchronously; the call is non-cancellable. Startup loads, warms, and benchmarks the graph, then each cycle checks that its remaining budget exceeds the configured measured inference ceiling plus required post-inference work before entering `tract`. The optimizer enumerates the bundle/config candidate lattice; first reject any candidate whose coolant envelope violates the hard ceiling/target-band priority, then minimize IAT cost and actuator movement lexicographically. No feasible candidate revokes model eligibility and chooses deterministic protection. A surprising in-flight inference overrun is recorded and safely allowed to trip lease expiry/controller fallback and/or the systemd watchdog; do not add an inference worker, service, or cancellation interface to disguise this property.

The two-slot model store is a deep startup module. Sync may write only the inactive staging directory and atomically publish a staged manifest after digest/compatibility checks. `celerityd` startup independently validates both slots, golden-vector parity/self-test, configuration compatibility, and desired/known-good metadata; it selects at most one model before authority and never changes it until restart. Runtime invalidity revokes model authority immediately but does not hot-swap. Demotion makes known-good desired for a future startup. Absence or invalidity of both slots is a tested deterministic-control outcome.

The home application owns the job lifecycle in one place: admit only complete immutable Runs, apply a versioned Derivation Spec to Parquet, create whole-Run train/validation/test splits, train the causal TCN, export ONNX, compare PyTorch/`tract` golden outputs, evaluate protected suites, stage only a passing immutable bundle, record the job terminal state, and separately enqueue exceptional notification. Use `sqlite3` with explicit migrations rather than adding an ORM/repository layer. Start training as a managed subprocess whose PID, stdout/stderr paths, recipe, input digests, and terminal result are durable; app restart reconciles nonterminal jobs. Notification failure updates notification state only.

## Plan of Work

### Non-stopping dependency milestone 1: repository, toolchains, contracts, and checks

Create the workspace, language projects, deployment/site directories, contract files, lockfiles, and checked-in check scripts before feature code. Pin a stable Rust toolchain with `rustfmt`, `clippy`, `aarch64-unknown-linux-gnu`, and `thumbv7em-none-eabihf`; configure rust-analyzer in `.vscode` to load the workspace and embedded target without hiding host diagnostics. `scripts/check-rust` runs format check, Clippy with `-D warnings`, host builds/tests, ARM64 vehicle build, and embedded release build. It may accept a documented `--portable` switch that skips only Linux cross-linking when the required linker is unavailable locally; CI runs the full command. It must never use QEMU.

Create `services/home/pyproject.toml` and `uv.lock`; every Python install, lock, test, lint, format check, and command uses `uv`. `scripts/check-python` runs `uv sync --locked`, `uv run ruff format --check`, `uv run ruff check`, and `uv run pytest`. Create `site/package.json` and lockfile with Prettier plus `prettier-plugin-svelte`, ESLint plus Svelte/TypeScript support, `svelte-check`, Vitest, and a static production build. `scripts/check-site` uses `npm ci`, then the checked-in npm scripts for format, lint, type/Svelte check, tests, and build. Do not add Biome, a second formatter/linter, Just, Taskfile, Bazel, or another generic task layer.

Write the fixed CAN, Run, configuration, home API, model-manifest, and webhook contracts before consumers. Add golden vectors for minimum/maximum valid values, every message, wrong length, nonzero reserved bytes, unknown enums, wrong node ranges, stale epochs/sequences, and digest examples. Rehome the four branch research notes through `git show` plus deliberate edits, retaining their source links and clearly marking superseded Nano/MCP251863 and ARX conclusions. Add CI jobs on macOS and Linux for portable checks, plus Linux jobs that install the ARM64 linker, create `vcan` interfaces, and run `scripts/check-linux`. This milestone ends only when the empty skeleton and contract tests pass through the same commands developers run; continue directly to milestone 2.

### Non-stopping dependency milestone 2: controller codec, emulator, and firmware

Implement `celerity-protocol` manually with explicit byte offsets and checked conversions. Avoid derive magic that makes padding/endianness implicit. The codec validates CAN ID family/address, exact legal CAN FD length, protocol generation where present, zero reserved bytes, enum domains, split range, and frame-specific relationships. Generate golden vector JSON with an explicit repository command and test it in both directions; generated files are deterministic and a clean rerun changes nothing.

Implement the emulator inside the runtime test/support surface but using the production codec and production controller state semantics. Port only accepted semantics from the throwaway prototype: discovery grants nothing, configuration is immutable under authority, Runtime and Command Leases are distinct, each accepted Command refreshes only the Command Lease, ACK means electrically applied rather than measured position, link restoration grants nothing, reboot loses authority, and reconciliation precedes fresh leases. Add conformance cases for rejected commands not refreshing the lease and for Command Lease expiry selecting fallback while a Runtime Lease remains valid. Replace demo timeouts and `RadiatorFraction` names; fixture values are clearly synthetic.

Implement `firmware/duct-controller` as a cohesive Embassy program with visible initialization of safe PWM output, volatile configuration, FDCAN filters, independent Runtime/Command Lease timers, heartbeat/fault output, watchdogs, and transceiver standby. Lease expiry must be driven by local monotonic timers independent of message arrival. Every controller reset selects a fresh nonzero hardware-random boot session distinct from the prior backup-domain value, starts in Controller-Local Fallback, and requires discovery plus configuration reconciliation before any lease or command can be accepted; no partially persisted generation exists. Firmware uses linear two-point PWM interpolation and reports only electrical application. Retain flash/stack/link map output and a checked-in G431-specific FDCAN message-RAM budget artifact. The artifact names every 11-bit filter, Rx FIFO and dedicated Rx-buffer element count and payload size, Tx event FIFO and Tx buffer/queue element count and payload size, the resulting word offsets, and the total allocated 32-bit words; a host check recomputes the total and rejects overlap or a value beyond the G431 allocation before build acceptance. G431 resource failure is measured evidence before considering G474.

Run the same raw-frame conformance fixtures through host codec, emulator, and firmware-native state machine tests. Cross-build the actual G431 image. Hardware FDCAN timing, transceiver standby, electrical fallback, PWM timing under load, and reset/power-cycle reconciliation remain explicitly HIL-only, but no software path is postponed.

### Non-stopping dependency milestone 3: runtime and deterministic authority path

Implement strict TOML deserialization with unknown-field denial and one shared normalization/validation path. `celerityd`, `simulate`, and `replay` all request a mode and load a complete bundle; mismatch is a startup error. Provide only simulation/replay examples. Validate identities/capabilities/ownership, timing relationships, policy monotonicity, absolute code bounds, Run thresholds, model ABI, and every required adapter before constructing `Runtime`.

Implement ordered canonical events, monotonic clock checks, bounded signal histories, immutable snapshots, expiring `True|False|Unknown` conditions, and the explicit executor cycle. Implement System Supervisor lifecycle `Starting|Running|Stopping`, global authority `Fallback|Permitted|HardFault`, feature authority `Fallback|Arming|Active`, circuit breaker `Closed|Open|HalfOpen`, and controller lifecycle/mode/fault truth. Keep recovery as bounded reconciliation activity rather than another authority state. A controller reboot, stale ACK, invalid time, queue overflow on essential controller traffic, or shared invariant failure stops lease renewal and requires fresh reconciliation.

Implement the deterministic 3x3 policy, asymmetric hysteresis, shared command shaper, and command-source hierarchy. Use one duct module and direct control flow; do not create one helper per table lookup, transition, or condition. Implement owner-armed immutable Experiment Plans with seeded transition-balanced repeated `0.10..0.90` step-and-hold blocks. Holds begin only after accepted command plus configured settling and abort Experiment classification on missing required evidence or storage degradation while normal control resumes.

Implement the live Linux composition with RX-only powertrain SocketCAN, actuator SocketCAN FD, systemd notification/watchdog, bounded queues, and filesystem Run storage. An image/provisioning boundary configures CAN netdevices; the runtime does not mutate controller `ctrlmode` when opening a raw socket. Before binding, live startup queries the real powertrain CAN netdevice and denies startup unless `ip -details`/netlink state confirms 1 Mbit/s Classical CAN, `LISTEN-ONLY`, and `restart-ms 0`. The RX-only adapter then subscribes to CAN error frames with `CAN_RAW_ERR_FILTER`, records receive overflow with `SO_RXQ_OVFL`, and requests `SO_TIMESTAMPING_NEW` raw-hardware receive timestamps with an explicit software-source fallback marker. Before binding the actuator socket, verify ISO CAN FD at 500 kbit/s / 2 Mbit/s with BRS and `restart-ms 0`; report bus/error counters and require deliberate requalification after bus-off. Powertrain decoding covers the documented standard-ID, big-endian Haltech ECU Broadcast CAN v2 stream and only a deliberately enabled collision-free CANTCU Default CAN Datastream; preserve all unknown raw frames without inventing meanings, and derive freshness from measured configuration rather than nominal vendor rates. Every network call, write, retry, wait, and worker lifecycle is visible at its owning entry point. Implement the read-only diagnostics socket and CLI.

### Non-stopping dependency milestone 4: Run storage, simulation, replay, and systemd proof

Implement the Protobuf event schema, exact-byte checksummed chunks, manifest sealing, predecessor links, monotonic sequence, time anchors, incomplete recovery, and versioned Derivation Spec reader. The control executor writes every raw bus frame, decoded observation, condition/authority transition, command-source decision, prediction/optimizer evidence required by mode, Experiment phase, command/ACK, fault, and storage condition. Queue/storage failure never blocks control, never auto-deletes, and makes the Run ineligible for training.

Implement small declarative TOML scenarios with typed actions, controlled time, raw powertrain frame encoding, emulator behavior, and semantic assertions. There is no scripting language or alternate policy. Provide `celerity simulate`, verification replay using the original artifacts/timing, and exploratory replay using recorded powertrain input plus newly generated commands. Both output a normal Run and meaningful exit status. Semantic comparison excludes only Run IDs, UTC anchors, and measured host latency; floating values use fixed tolerances.

Required scenarios cover healthy startup through deterministic/model authority; missing/stale/invalid/recovered inputs; model absence, OOD, invalid output, pre-inference budget denial, surprising in-flight overrun, and infeasibility; dropped/late/duplicate/rejected ACKs; Command Lease expiry while Runtime Lease remains valid; Runtime Lease expiry; CAN loss/restoration and controller reboot; graceful shutdown, abrupt runtime death, and fresh restart; incompatible identity/config/model; interrupted Experiment; and Run queue/free-space/write failure. Linux `vcan` exercises production application adapters, interface separation, exact frames, and zero application powertrain TX. It does not prove real netdevice controller mode or on-wire silence.

Add `deploy/systemd/celerityd.service` and `celerity-sync.service`. `celerityd` uses `Type=notify`, `NotifyAccess=main`, systemd watchdog, bounded stop, restart-rate limiting, local filesystem ordering, and no `network-online.target`. Run the lifecycle CI job on the pinned standard GitHub-hosted `ubuntu-24.04` VM, never `ubuntu-slim` and never a container. First fail if the host systemd is unavailable. Run `systemd-analyze verify` on the exact checked-in units. With passwordless `sudo`, install temporary exact units and fixture configuration under `/run/systemd/system`, run `daemon-reload`, and exercise readiness, watchdog, bounded stop, `SIGKILL`, automatic restart, and restart without authority. A shell trap must stop the temporary units, remove every installed file, and run `daemon-reload` even after failure. Vehicle runtime itself remains host-native and uncontainerized.

### Non-stopping dependency milestone 5: model, optimizer, and startup-only slots

In `services/home`, implement one causal multi-output TCN and deterministic training recipe. Make causal padding testable by proving future input perturbations cannot change earlier outputs. Train fixtures with fixed seeds, export fixed-shape ONNX, and emit bundle manifest/golden input-output vectors. Call a small checked-in Rust parity executable using the production `tract` wrapper from the Python evaluation job so promotion compares both runtimes rather than a Python ONNX substitute.

In Rust, build typed recent-state windows, normalization, synchronous `tract` loading/warmup, output validation, uncertainty/OOD eligibility, and finite-control-set optimization. Use purpose-built tiny real ONNX fixtures for all tests, including model absence/corruption/incompatibility/nonfinite output. At startup, benchmark the exact loaded graph enough to establish a configured measured ceiling; before each inference, compare remaining cycle budget against that ceiling plus post-inference work and skip to deterministic control if insufficient. Record both preventive skips and any surprising in-flight overrun. Benchmark natively as software evidence without claiming ARM timing, and require measured target margin at HIL. Candidate rejection and lexicographic selection tests prove coolant priority ahead of IAT and movement.

Implement exactly two model slots with atomic inactive staging and startup selection. Simulate initial absence, good activation, corrupt candidate rejection, runtime invalidity, evidence demotion, desired known-good rollback, interrupted download, and startup with neither slot valid. No code path may load or swap a graph after authority begins.

### Non-stopping dependency milestone 6: sync, home application, Kubernetes, and notification

Implement `celerity-sync` with Linux home-network detection, periodic recovery retry, spool inventory, idempotent chunk upload, exact-digest acknowledgement, bounded acknowledged retention, desired model download, inactive-slot staging, and durable SQLite transfer journal owned only by sync. Network availability wakes reconciliation but never defines correctness. Sync has no controller/runtime diagnostics mutation and cannot backpressure `celerityd`.

Implement the home FastAPI application using `uv`, Uvicorn, stdlib SQLite, filesystem/PVC storage, PyTorch/ONNX training dependencies, and explicit startup migrations. Authenticate every vehicle request with one manually rotatable token. Validate chunks and manifests before admitting Runs. Exclude incomplete/corrupt/nonmonotonic Runs; exclude bad windows with machine reasons. Build whole-Run splits and preserved evaluation suites, compare candidate to active, and automatically stage only a compatible passing artifact. No eligible data or no better candidate is a successful no-change job.

Implement one configured HTTPS webhook and durable events for training failure, candidate rejection, demotion, and rollback. Retries are bounded and best-effort. Tests prove a permanently failing webhook leaves model/job outcomes unchanged. Add a split-template Helm chart for the single-replica Deployment, PVC, Service, ConfigMap, probes, resource bounds, and security context. Render one ExternalSecret for the vehicle token and webhook URL; require an infrastructure-supplied name for the existing 1Password `ClusterSecretStore`. Do not render static Secret values or add a queue, object store, PostgreSQL, model microservice, or simulated cluster.

Exercise a complete local loop with tiny fixtures: seal Run, detect home, upload/resume/acknowledge, derive/train/evaluate, stage/download, restart runtime, activate candidate, reject corruption, and select known-good at a later startup. Use the production HTTP client/server and temporary filesystem/SQLite state.

### Non-stopping dependency milestone 7: public architecture walkthrough and documentation

Build a new SvelteKit static site under `site/` using the repository's dark technical visual language: near-black grid, warm amber for authority/control, cyan for observation/data, red for faults/physical boundaries, and accessible high-contrast typography. Do not copy the prototype simulator as product code. The guided walkthrough explains ownership boundaries, startup-to-command lifecycle, deterministic/model hierarchy, controller lease recovery, Run-to-home-to-model flow, included hardware-free evidence, physical-only evidence, and why the site is not a driver UI. Include keyboard/mobile navigation, reduced motion, semantic headings, and durable deep links.

Update `README.md`, `CONTEXT.md` only if terms need clarification, operator/developer docs, contract docs, configuration guide, service install guide, home deployment guide, validation checklist/report templates, and physical acceptance boundary. Documentation names the exact checked-in checks and expected behavior. The site and docs must never claim installed hardware, electrical proof, trained vehicle model quality, or road acceptance.

### Non-stopping dependency milestone 8: integrated evidence and honest boundary

Run every checked-in portable check on macOS and Linux CI, the full Linux/vcan/systemd job, ARM64 cross-build, embedded build and host conformance, local home lifecycle, and site build. Re-run generation/export commands and require a clean diff. Capture concise evidence in `docs/validation/hardware-free-report.md` keyed by commit, config fixtures, schema versions, firmware build, model fixture digest, and command output.

Maintain `docs/validation/first-deployment-checklist.md` as the single ordered future physical checklist, not a subsystem. It distinguishes HIL resource/timing/FDCAN/PWM/flash/watchdog evidence; electrical power, fail-silent tap, termination, transceiver, harness, and passive-return evidence; mechanism calibration/settling/backlash; thermal experiments; and installed/road/model-envelope proof. Mark these unexecuted without treating them as missing software.

## Concrete Steps

All commands run from `/Users/kellen/development/github/kellen-miller/celerity/.worktrees/celerity-runtime` unless the plan is later relocated; when implementing elsewhere, use the repository root containing this file.

After milestone 1 and after every meaningful slice, run the narrow affected tests and then the language check:

    ./scripts/check-rust
    ./scripts/check-python
    ./scripts/check-site

On Linux with `vcan`, the ARM64 cross-linker, and systemd test prerequisites, run:

    sudo modprobe vcan
    sudo ip link add dev vcan-powertrain type vcan 2>/dev/null || true
    sudo ip link add dev vcan-controller type vcan 2>/dev/null || true
    sudo ip link set up vcan-powertrain
    sudo ip link set up vcan-controller
    ./scripts/check-linux

The contract and fixture generation commands must be explicit checked-in binaries/scripts, for example:

    cargo run -p celerity-protocol --features std --bin write-golden-vectors -- contracts/golden/controller-can-v1
    uv run --project services/home python -m celerity_home.export_fixture --output contracts/golden/model-v1
    git diff --exit-code -- contracts/golden

Exercise the portable runtime:

    cargo run -p celerity --bin celerity -- simulate config/scenarios/healthy-startup.toml
    cargo run -p celerity --bin celerity -- replay fixtures/runs/healthy-startup --verify
    cargo run -p celerity --bin celerityctl -- status --socket .tmp/celerityd.sock

Expected output is concise and semantic: scenario/replay names, pass/fail, final supervisor/feature/controller authority, command source, accepted split, Run path/digest/completion, and typed failure reason. Do not snapshot ANSI prose as the test oracle.

Exercise the home loop through a checked-in integration test command used by CI:

    ./scripts/check-python
    uv run --project services/home pytest -m end_to_end -q

Build and inspect the public site:

    ./scripts/check-site
    npm --prefix site run dev -- --host 127.0.0.1

Before completion, run all four checked-in checks and verify repository cleanliness:

    ./scripts/check-rust
    ./scripts/check-python
    ./scripts/check-site
    ./scripts/check-linux
    git diff --check
    git status --short --branch

## Validation and Acceptance

Portable Rust acceptance is rustfmt-clean, Clippy-clean with warnings denied, all host tests/builds passing on macOS and Linux, exact protocol golden vectors passing, `aarch64-unknown-linux-gnu` vehicle binaries cross-building, and the `thumbv7em-none-eabihf` firmware release image building with retained flash/stack/link-map and FDCAN message-RAM budget evidence. The message-RAM check covers standard filters, Rx FIFO/buffer element counts and payload sizing, Tx event/buffer elements, offsets, overlap, and total words against the G431-specific allocation. Rust-analyzer opens the host workspace and embedded project from checked-in editor settings. No QEMU result is required or accepted as ARM timing proof.

Python acceptance is a locked `uv` environment, Ruff format/lint clean, all tests passing, deterministic TCN fixture export, PyTorch/`tract` parity within the contract tolerance, whole-Run data splitting, automatic pass/no-change/reject jobs, durable restart recovery, and webhook failure independence. No Python environment or object crosses the vehicle authority path.

Site acceptance is Prettier clean, ESLint clean, `svelte-check` clean, Vitest passing, static production build successful, and browser tests at desktop/mobile widths covering every guided step, keyboard operation, deep links, and absence of driver controls. The site visibly labels project documentation and physical evidence boundaries.

Runtime acceptance is behavior, not merely types. Repeating each logical-time scenario produces the same semantic decisions and command sequence. Startup begins in fallback; model absence reaches deterministic authority only after shared/input/controller gates; model eligibility reaches optimized control after healthy dwell and the pre-inference budget gate; stale input or ACK removes the appropriate authority; CAN restoration does not restore leases; controller reboot changes boot session and requires reconciliation; each accepted Command refreshes the independent Command Lease; rejected commands do not; Command Lease expiry selects Controller-Local Fallback even while Runtime Lease remains valid; global hard faults latch; Run storage failure marks incomplete while safe control continues; restart creates a new Run/epoch and restores no authority; shutdown is bounded and either lease expiry remains final. A recorded in-flight `tract` overrun may cause fallback/watchdog restart and is not misreported as a cancellable graceful skip.

Powertrain acceptance requires all production types and imports to expose no send path. `vcan-powertrain` proves interface separation and zero application transmission only. Hardware-free tests feed representative queried netdevice attributes to the live-startup verifier and require mismatch rejection, but `vcan` is never credited for hardware `ctrlmode` proof. In the live/HIL environment, the actual netdevice must report 1 Mbit/s Classical CAN, `LISTEN-ONLY`, and `restart-ms 0` before binding. Physical on-wire silence and independent fail-silent enforcement remain HIL checklist evidence. Controller acceptance requires every v1 ID/layout/golden vector to pass through host codec, emulator, and firmware state logic, including accepted-Command lease refresh, Command Lease expiry under a still-valid Runtime Lease, Runtime Lease expiry, bad session/generation/sequence, reserved bytes, reboot, and recovery.

Run acceptance requires native-rate raw evidence, monotonic order, all relevant control/fault events, exact-byte checksums, atomic complete manifests, recoverable incomplete Runs, no auto-deletion, training exclusion for incomplete Runs, and deletion only after home digest acknowledgement. Simulation/replay must use real runtime policy and production encoding; exploratory replay must state that it is not counterfactual plant truth.

Linux integration acceptance requires the pinned `ubuntu-24.04` VM job to fail when host systemd is unavailable, pass `systemd-analyze verify` on the checked-in units, install exact temporary units under `/run/systemd/system`, and prove readiness/watchdog, bounded shutdown, restart limits, `SIGKILL` recovery, no `network-online.target`, and restart into fallback. Cleanup runs from a trap and removes the units before daemon reload. Focused `vcan` tests prove only production application behavior and interface separation. Generic Linux paths and configuration contain no Raspberry Pi assumption.

End-to-end acceptance is one local command/test that demonstrates complete Run upload, exact acknowledgement, derivation, TCN training/evaluation, ONNX bundle stage, vehicle download, inactive-slot validation, next-startup activation, corrupt rejection, later known-good rollback, and durable nonblocking notification failure. The Helm chart lints/renders and describes one replica/PVC/SQLite/filesystem without simulating a cluster.

The hardware-free report may claim all of the above only with command evidence. It must explicitly leave unclaimed: live Haltech/CANTCU decode validation on this car, physical zero-drive fail-silent tap, FDCAN electrical behavior, controller/servo timing under target load, protected power/EMC/thermal behavior, passive radiator-biased return, mechanism travel/settling/backlash, actual model accuracy/thermal envelope, target ARM deadlines, installation, and road/track acceptance.

## Idempotence and Recovery

All code generation and model fixture export must be deterministic; rerunning them either produces identical bytes or fails with an actionable version drift. Contract migrations are additive only within an undeployed draft; once v1 is used outside the repository, incompatible changes require v2 and no translator. Do not preserve provisional code merely to ease migration because nothing is deployed.

Run writing is append-only. On interruption, retain chunks and recover the Run as incomplete; never rewrite it complete. Sync upload is content-addressed and idempotent, so retry the same chunk/digest after network loss. Server acknowledgement precedes any local retention deletion. Home jobs record inputs and terminal result; on restart reconcile durable state and retry only idempotent work. Model staging writes to an inactive temporary path, verifies, fsyncs, and atomically publishes; discard an incomplete temp path without touching active/known-good.

Runtime startup failure leaves the controller in local fallback and reports a typed reason. A runtime restart creates a new Run, boot epoch, conditions, leases, and history. Configuration/model/controller mismatch never falls through to a default. A pre-inference budget failure selects deterministic control without entering `tract`; a surprising in-flight overrun cannot be interrupted, is recorded, and safely yields lease expiry/Controller-Local Fallback and/or watchdog restart. Controlled shutdown always stops lease renewal even if logging, fallback acknowledgement, or systemd notification fails.

Firmware configuration is deliberately volatile: every reset selects a fresh boot session, establishes the safe powered fallback, and requires discovery plus full configuration reconciliation before accepting authority. A failed embedded build does not justify switching to G474 until flash, stack, link map, runtime, or the explicit G431 FDCAN message-RAM budget identifies resource margin as the cause. Physical tests are never replaced with simulated success.

## Artifacts and Notes

The completed Wayfinder map and tickets remain linked from `.agent/work/celerity-runtime/decision.md`. Do not copy ticket transcripts into product docs. Reference evidence branches with `git show`; do not merge or cherry-pick them wholesale. Specific accepted facts may be reimplemented or rewritten into durable contracts/docs with provenance.

The controller prototype at commit `bc4419b` is reference-only. Its interactive lifecycle demonstrates useful semantics, but its symbolic messages, helper layering, demo timing, and old `RadiatorFraction` language do not establish production shape. The new public site may reuse visual direction, not source or simulated product behavior.

Small public fixtures must contain synthetic or deliberately scrubbed data. Full vehicle Runs remain private home archive data; do not add Git LFS or an external CI dataset service initially.

Implementation source notes for these corrected checks are the current [Linux SocketCAN documentation](https://docs.kernel.org/networking/can.html), which exposes `LISTEN-ONLY`, `restart-ms`, controller-state inspection, error filters, and the virtual local CAN boundary; the [GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners), which identifies standard Ubuntu labels as VMs and Linux VM `sudo` as passwordless; the [STM32G4 reference manual](https://www.st.com/resource/en/reference_manual/dm00355726-stm32g4-series-reference-manual-stmicroelectronics.pdf); and ST's [FDCAN application note](https://www.st.com/resource/en/application_note/dm00625700-fdcan-peripheral-on-stm32-devices-stmicroelectronics.pdf), which requires explicit allocation of filters, receive sections, transmit sections, element sizes, offsets, and total message-RAM words.

## Revision Note

2026-07-31: Initial ExecPlan created from the completed Wayfinder map, selected worktree evidence, resolved delivery ledger, repository AGENTS instructions, and the explicit requirement to establish and enforce all language toolchains in CI before feature implementation.

2026-07-31: One codebase-design-guided improvement pass made the Linux CAN edge faithful to the research branches (`SO_TIMESTAMPING_NEW`, error/overflow evidence, deliberate bus-off recovery, Haltech endianness), preserved accepted research without merging obsolete branch choices, added the missing typed sync/home-network configuration, made one normalization path explicit, and added an enforceable authority-isolation test for `celerity-sync`.

2026-07-31: The sole cross-provider planning adversarial review produced six non-blocking findings. The plan accepted the Command Lease refresh/independent-expiry rule, corrected `vcan` claims and real-netdevice startup verification, made synchronous `tract` deadline/overrun behavior explicit without redesign, specified the `ubuntu-24.04` systemd CI mechanism, and added a G431 FDCAN message-RAM budget. It retained discovery/announce/capability with an explicit reconciliation rationale because issue #8 already accepted those semantics. These are contract clarifications that preserve the approved intent; revision-1 approval remains valid and no new approval is required.

2026-08-01: The sole cross-provider implementation adversarial review found eight verified issues. The implementation now separates event ingest and signal observation time, generates reboot-visible firmware boot sessions, treats controller configuration as intentionally volatile, fails closed on an unreadable desired baseline, validates Run v1 structure at admission, serializes and recovers home jobs durably, compares firmware/emulator behavior, byte-checks firmware golden responses, and compares bearer tokens in constant time. The corrected host and Linux vCAN matrices pass; fresh CI and the Lavish ownership loop remain.
