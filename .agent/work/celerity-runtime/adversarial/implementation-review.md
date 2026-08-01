# Celerity implementation-boundary adversarial review

Provider: Anthropic Claude Opus (cross-provider review)

## Verification performed

- Traced the live cycle through CAN discovery, configuration, leases, command, acknowledgement, authority selection, model command, promotion evidence, and Run sealing.
- Confirmed the powertrain surface is RX-only and exposes no transmission path.
- Confirmed promotion to known-good requires a real model-sourced accepted command acknowledgement in a complete Run.
- Confirmed Command and Runtime leases expire independently in the emulator and firmware.
- Confirmed two-slot staging, rollback digest handling, and incomplete/degraded Run sealing.

## Findings

### 1. HIGH - Intra-cycle event ordering can latch a spurious HardFault

`crates/celerity-runtime/src/executor.rs` rejects any event timestamp below the
last ingested timestamp. Controller frames use their drain time, but the
subsequent `InputSnapshot` uses the earlier powertrain observation time. A
controller frame and input snapshot in one live cycle can therefore appear to
regress time and permanently latch a hard fault.

Separate ingest ordering time from signal observation time. The latter should
only drive staleness.

### 2. HIGH - Firmware boot session is a compile-time constant

`firmware/duct-controller/src/main.rs` defines `BOOT_SESSION` as `1`. A
controller restart is consequently invisible to the daemon, which prevents
the required fresh discovery and configuration reconciliation before authority
is restored.

Derive the boot session at startup from a non-repeating persisted boot counter.

### 3. MEDIUM - Firmware configuration persistence is not composed

Accepted configuration is only applied in RAM. The existing two-record
power-loss helpers are never used by the firmware main loop, so commissioned
configuration is lost on reboot.

Compose the two-record persistence boundary into firmware configuration
acceptance and boot recovery, or explicitly make configuration volatile and
force reconciliation on every reset.

### 4. MEDIUM - Home evaluation fails open when the baseline is unreadable

An absent or incompatible baseline leaves `baseline_mse` as `None`, which is
treated as an automatic candidate improvement. A baseline read failure should
fail evaluation closed; only the intentional no-baseline bootstrap case may
stage a candidate without comparison.

### 5. MEDIUM - Run admission does not enforce record structure or monotonicity

Run completion checks manifest and chunk digests but does not decode the
protobuf stream or validate monotonic event ordering. One corrupt admitted Run
can later fail the whole training job instead of being excluded with a machine
reason.

Validate each Run at admission or corpus load and exclude only the offending
Run.

### 6. MEDIUM - Home job durability and concurrency are not fully idempotent

PID reuse can leave a recovered job marked as running, deterministic duplicate
models collide on a primary key, active-job admission is a check-then-insert
race, and SQLite connections have no busy timeout or WAL setup.

Track process ownership across restarts, make deterministic model insertion
idempotent, serialize active-job admission, and configure SQLite WAL plus a
busy timeout.

### 7. LOW - Firmware and emulator lack an equivalence check

End-to-end tests drive the emulator, while firmware conformance tests only
decode golden vectors. Existing emitted-field differences demonstrate that
the two state machines can drift without failing CI.

Compare their externally observable state and emitted bytes for identical
input sequences, and byte-compare firmware responses with the golden vectors.

### 8. LOW - Vehicle-token comparison is not constant-time

The home service compares the Authorization value with normal string equality.
Use `hmac.compare_digest`.

## Areas checked clean

- RX-only powertrain boundary and separate CAN FD actuator-bus qualification.
- Startup-only model selection, digest and ABI validation, ONNX self-test,
  rejection persistence, and acknowledgement-gated promotion.
- Independent controller lease semantics and strict command freshness.
- Nonblocking Run enqueue, degraded/incomplete sealing, atomic manifest rename,
  and partial-Run recovery.
- Deterministic lexicographic optimizer behavior.
- Internally consistent message-RAM budget.

```text
---ADVERSARIAL_REVIEW_STATUS---
AUTHOR_PROVIDER: OpenAI
REVIEWER_PROVIDER: Anthropic
REVIEWER_MODEL: Claude Opus
INDEPENDENCE: cross_provider
ISSUES_FOUND: 8
CRITICAL_COUNT: 0
HIGH_COUNT: 2
MEDIUM_COUNT: 4
LOW_COUNT: 2
CONFIDENCE: HIGH
BLOCKING: true
SUMMARY: Two code-confirmed live-path defects plus home durability and firmware conformance gaps require disposition.
---END_ADVERSARIAL_REVIEW_STATUS---
```
