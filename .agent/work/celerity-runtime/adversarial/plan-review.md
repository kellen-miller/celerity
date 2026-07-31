I have read all required files in full and independently verified the CAN FD contract arithmetic. I did not need to open the GitHub map or evidence branches to reach these findings — every claim below is grounded in `decision.md` and `execplan.md` directly. Here is my review.

# Celerity planning-boundary adversarial review

**Reviewer:** Anthropic Claude Opus (medium effort), cross-provider, single reviewer. Author: OpenAI `gpt-5.6-sol` (high).

## Verification performed (no defect)

I reconstructed the v1 CAN contract to answer Q1 concretely before hunting for defects:

- **ID allocation is collision-free and in-range.** Every family uses a 0x40 (64) stride with node range `1..63` (max offset `0x3f`), so each family exactly fills its block with no overlap: `FaultReport` `0x041..0x07f`, `DiscoveryProbe` `0x080`, reserved `0x081..0x0ff`, … `CapabilityReport` `0x381..0x3bf`, reserved `0x3c0..0x7ff`. Top ID `0x3bf` is within the 11-bit limit `0x7ff`. No two families collide; address `0` reserved is respected.
- **Every payload's fields sum to its declared length,** and every declared length (8/16/24/32) is a legal CAN FD DLC. I checked all thirteen messages byte-by-byte (`DiscoveryProbe`=8, `Configuration`=32, `Command`=24, `CommandAck`=32, `Heartbeat`=32, `FaultReport`=24, `NodeAnnounce`=32, `CapabilityReport`=24, `RuntimeLease`/`Ack`=24, `ConfigurationAck`=16, `FallbackRequest`/`Ack`=16). All balance exactly.
- Unaligned `u64` offsets (e.g., `FaultReport.related_epoch` at offset 12) are a non-issue because the plan mandates manual byte-offset codecs with explicit LE conversions (`execplan.md:135`), which the STM32G431 Cortex-M4 handles via byte assembly.

The contract is internally consistent and implementable. The findings below are the material gaps that survived scrutiny.

## Findings

### 1. HIGH — Command Lease renewal trigger is unspecified in the "fixed" v1 contract

**Evidence:** `decision.md:37` ("Runtime and Command Leases are distinct"); `Configuration` carries both `Runtime Lease milliseconds` and `Command Lease milliseconds` (`decision.md:66`); `CommandAck` reports `Command Lease remaining milliseconds` (`decision.md:68`). The contract defines explicit `RuntimeLease`/`RuntimeLeaseAck` messages (`0x180/0x1c0`) that renew the _Runtime_ Lease via `epoch` + `renewal sequence`, but **there is no message or rule that renews the Command Lease.** Neither `decision.md` nor `execplan.md` states the trigger.

**Impact:** The Command Lease is safety-critical (its expiry is what drops the controller to Controller-Local Fallback while the Runtime Lease may still be valid). A byte-level v1 contract that claims lease semantics "must remain explicit" (`decision.md:25`, hard constraint) leaves the single most consequential timer's renewal rule to inference. Two implementers (host vs. firmware, or runtime vs. emulator) can diverge on when the Command Lease resets, producing either premature fallback or dangerously stale command persistence — and the golden-vector suite cannot lock a rule that isn't written.

**Fix / check:** Add one normative sentence to `contracts/controller-can-v1.md` and `decision.md`: e.g. "Each accepted `Command` refreshes the Command Lease to the configured `Command Lease milliseconds`; Command Lease expiry is evaluated on the controller's local monotonic timer independent of Runtime Lease state and independently drives Controller-Local Fallback." Add an emulator conformance scenario asserting Command-Lease expiry with a still-valid Runtime Lease.

### 2. MEDIUM — SocketCAN listen-only _mode_ cannot be validated on `vcan`; acceptance implies it can

**Evidence:** Powertrain acceptance requires "SocketCAN listen-only setup to be tested, and an external `vcan-powertrain` sniffer to observe zero transmitted Celerity frames" (`execplan.md:255`); `execplan.md:61` "opens the SocketCAN interface in listen-only mode."

**Impact:** `vcan` (virtual CAN) does not implement CAN controller `ctrlmode` bits — `listenonly` is a real-hardware controller mode. The `vcan` sniffer test therefore proves only that the _application code path never calls send_; it cannot prove the listen-only _hardware configuration_ is applied or enforced. Treating "listen-only setup tested" as satisfied by `vcan` is a validation claim exceeding hardware-free evidence (Q8) — the actual electrical fail-silent guarantee remains HIL-only, which the plan elsewhere is careful about.

**Fix / check:** Split the claim: (a) software test asserts the live adapter _requests_ `CTRLMODE_LISTENONLY` and exposes an RX-only type with no send method (structural + config-level, portable); (b) explicitly move "listen-only mode enforced on the wire" to `first-deployment-checklist.md` as HIL evidence. Update `execplan.md:255` wording accordingly.

### 3. MEDIUM — Synchronous model inference sits on the single-writer control path; the "deadline" gate cannot abort an in-flight `tract` run

**Evidence:** The executor cycle runs "eligible prediction/optimization" inline between snapshot freeze and output (`decision.md:33`, `execplan.md:103`). Model eligibility uses a "deadline" gate (`decision.md:82`). `tract` inference is a synchronous, non-cancellable call.

**Impact:** A "deadline" eligibility gate can only _predictively skip_ inference based on a prior latency budget; it cannot interrupt a `tract` call already running inside the single-writer loop. A pathological inference time stalls the same thread that renews leases and pets the systemd watchdog, so the failure mode is a safe-but-disruptive Controller-Local Fallback (lease expiry) and/or watchdog restart, not a graceful skip. This degrades safely (good) but is unproven on target, where timing is HIL-only — so the architecture's real-time viability rests on hardware evidence the plan cannot produce.

**Fix / check:** Specify the deadline gate as a _pre-inference_ budget check with an explicit host-measured per-cycle inference ceiling; document that overrun degrades to fail-safe fallback rather than graceful skip; and record the native latency budget as an acceptance number (already benchmarked per `execplan.md:167`) with a stated margin the target must beat at HIL.

### 4. MEDIUM — Discovery/announce/capability handshake is machinery beyond the one confirmed node

**Evidence:** v1 defines broadcast `DiscoveryProbe`, `NodeAnnounce`, and `CapabilityReport` plus a 63-node address space (`decision.md:48-59`), while immediate confirmed scope is a single provisioned duct controller with identity/capability already carried in `Configuration` and `CapabilityReport`. The owner "strongly prefers minimal complexity" and the plan bars "fleet/enterprise machinery" (`decision.md`, hard constraints).

**Impact:** For a single controller whose identity and command bounds are provisioned config, a runtime discovery/announce protocol is concept proliferation the plan itself warns against (`execplan.md:11`). It adds three message types, two more monotonic sequences (`probe`, `announce`), and emulator/firmware state for a capability the confirmed v1 never exercises.

**Fix / check:** Either (a) justify each of the three discovery messages against a concrete confirmed v1 need in `decision.md`, or (b) drop the runtime discovery/announce handshake — keep the address-family ID scheme and provisioned identity/capability — and reintroduce discovery only if a multi-node bus is actually confirmed. This removes machinery without deleting any confirmed control behavior (Q9).

### 5. MEDIUM — systemd `Type=notify`/watchdog lifecycle test has no concrete CI execution mechanism

**Evidence:** `execplan.md:161` requires a lifecycle test proving readiness/watchdog/bounded shutdown/restart, run "in a disposable user/systemd test environment or container with systemd as init only for service verification." Named platforms are macOS + Linux CI (`decision.md`, quality gates).

**Impact:** Standard Linux CI runners do not provide systemd as PID 1, and `systemd --user` with a working notify/watchdog D-Bus session is not reliably present on hosted runners. The requirement is stated as executable-from-greenfield (Q6) but the _how_ is hand-waved, so this acceptance gate may be un-runnable as written — turning a claimed hardware-free proof into an unexecuted one.

**Fix / check:** Name the concrete mechanism (e.g., privileged container with `systemd` as init, or a documented `systemd --user` runner setup), and if neither is viable in the target CI, reclassify watchdog/notify enforcement as manual/HIL evidence rather than a portable acceptance check.

### 6. LOW — STM32G431 FDCAN message-RAM budget for 13 message types is unquantified

**Evidence:** 13 standard IDs up to 32-byte payloads on one `FDCAN1` instance (`decision.md:48-70`); resource-margin fallback to G474 is gated on "map/size output" (`decision.md:29`, `execplan.md:139`).

**Impact:** G431's FDCAN shares a small (~1 KB) message RAM across Rx FIFOs, filter lists, and Tx buffers. With 13 receive identifiers and 32-byte element sizes, the filter/FIFO layout must be shown to fit; the plan's retained "size evidence" focuses on flash/stack, not message RAM specifically. This is a concrete feasibility datum for Q1's "implementable on the selected controller."

**Fix / check:** Add FDCAN message-RAM element-count/size budgeting to the CI-retained size evidence, so G431 message-RAM exhaustion (if any) surfaces as measured evidence before considering G474.

## Areas assessed clean

- **Q2 (transmit leak):** No plan language authorizes powertrain TX. RX-only type with no send method, listen-only open, `cargo metadata` architecture test proving `celerity-sync` has no path to protocol/runtime/SocketCAN (`execplan.md:105`). No transmit-capable handle is leaked to feature code.
- **Q3 (open-loop truth):** ACK consistently means "electrically applied," never measured position (`decision.md:29,68`, `execplan.md:137`); `CommandAck.output_state` and enum-zero-invalid rules are sound; controller lease expiry is local-timer driven; reboot changes `boot session` and grants no authority.
- **Q4 (model lifecycle):** Startup-only two-slot activation, "no code path may load or swap a graph after authority begins" (`execplan.md:169`), incomplete Runs never train, real ONNX/`tract` fixtures with no fake predictor. Mid-drive activation is structurally prevented.
- **Q5 (omissions):** Every immediate-scope subsystem maps to a milestone; none is deferred or subtly gated — milestones are correctly framed as dependency ordering only.
- **Q7 (deep seams):** Every seam has ≥2 real adapters; emulator uses production codec + production state and is exercised by shipped `simulate` mode (not test-only); no in-memory Run repo or fake predictor interface.

---ADVERSARIAL_REVIEW_STATUS---
AUTHOR_PROVIDER: OpenAI
REVIEWER_PROVIDER: Anthropic
REVIEWER_MODEL: Claude Opus
INDEPENDENCE: cross_provider
ISSUES_FOUND: 6
CRITICAL_COUNT: 0
HIGH_COUNT: 1
MEDIUM_COUNT: 4
LOW_COUNT: 1
CONFIDENCE: MEDIUM
BLOCKING: false
SUMMARY: Contract arithmetic verified sound; one HIGH gap (Command Lease renewal rule undefined) plus four MEDIUM refinements (vcan listen-only over-claim, inference-on-control-path deadline semantics, discovery machinery, systemd CI feasibility) — all resolvable in-plan, none invalidating the approach.
---END_ADVERSARIAL_REVIEW_STATUS---
