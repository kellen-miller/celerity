# Hardware-free validation report

- Date: 2026-08-01
- Base commit: `a993404`
- Branch: `feat/celerity-runtime`
- Validated implementation PR/head: #17 / `47283e6`
- GitHub Actions run: `30675238377`
- Configuration fixtures: `config/examples/simulation.toml`, `config/examples/replay.toml`
- Contract schemas: controller CAN v1, Run v1, home v1, model bundle v1

## Support contract

Linux is the sole vehicle runtime and deployment target. The macOS portable job
is developer feedback for platform-independent logic because development may
happen from a Mac; it does not claim macOS CAN, systemd, firmware, or vehicle
support. No Raspberry Pi assumption exists in the runtime contract.

## Executed locally

The checked-in developer gates passed on macOS:

```sh
./scripts/check-rust --portable
./scripts/check-python
./scripts/check-e2e
./scripts/check-site
./scripts/check-artifacts
git diff --check
```

Those commands executed 63 portable Rust tests with one intentionally ignored
Linux-only production HTTP test, 16 Python tests, two site tests, a real
production HTTP/TCN/ONNX/model-slot exercise, deterministic artifact checks,
the STM32G431 embedded release build, Helm lint/render/package checks, and the
static SvelteKit build. Rustfmt and Clippy passed with warnings denied; Ruff,
Prettier, ESLint, and `svelte-check` passed. The generated controller vectors
and model fixture reproduced without a checked-in diff.

A privileged Linux/amd64 container with `vcan-powertrain` and
`vcan-controller` also passed:

```sh
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
```

The Linux-only tests proved that the production receive surface did not
transmit to either vCAN interface and that real kernel sockets carried a model
command through controller acknowledgement while sealing the Run. This is
application/interface-separation evidence only; `vcan` does not prove physical
listen-only wiring or on-wire fail-silence.

## Behavior proved

- The fixed CAN FD codec, controller emulator, and firmware-native state use
  the same messages and byte-compare every golden vector. A direct equivalence
  test compares configuration, Runtime Lease, Command Lease, accepted command,
  mode, split, and expiry behavior.
- Firmware establishes the compiled powered radiator-protective PWM before
  waiting for hardware RNG. It selects a fresh nonzero boot session distinct
  from the immediately prior TAMP backup-domain value, remains logically
  unconfigured, and rejects leases and commands until volatile Configuration
  reconciliation succeeds.
- The live single-writer path distinguishes event-ingest monotonic time from
  sensor-observation time. Older-but-valid observations drive staleness without
  appearing to regress executor order or latching a false hard fault.
- Accepted `CommandAck` frames refresh exact controller truth, and controller
  emulator/firmware heartbeats use the configured period. Command and Runtime
  leases expire independently and either loss restores controller-local
  fallback.
- Startup-only model selection validates digest, ABI, shape, self-test, OOD,
  uncertainty, and cycle budget. A real ONNX model reaches the Rust `tract`
  predictor, finite-control-set optimizer, actuator Command, accepted ACK, Run
  seal, and acknowledgement-gated promotion evidence.
- Immutable Run admission verifies the manifest/chunk digests and canonical
  Run v1 protobuf framing, schema major, source, exactly one typed payload,
  strict sequence order, monotonic time, UTF-8, and finite signal values before
  inserting the Run or exposing it to training.
- An absent desired baseline is the intentional bootstrap case. A configured
  desired baseline that is missing, unstaged, incompatible, unreadable, or
  nonfinite fails candidate evaluation closed.
- The home application uses SQLite WAL, a five-second busy timeout, an
  immediate transaction for active-job admission, and a partial unique index
  for one queued/running job. PID ownership includes Linux process start time;
  deterministic duplicate model results converge to `no_change`; notification
  writes are idempotent and webhook failure cannot alter job results.
- `celerity-sync` remains authority-isolated, uploads exact complete Runs,
  retains data until exact acknowledgement, stages only the inactive slot, and
  preserves the only known-good rollback artifact.
- Simulation, replay, experiment bounds, Run recovery, model absence,
  corruption, later-startup activation, and known-good rollback execute through
  the production policy and storage paths rather than alternate mocks.

## Executed in GitHub Actions

PR #17 implementation head `47283e6` passed GitHub Actions run
`30675238377` in all three jobs:

- Linux passed in 2m14s. `./scripts/check-linux` included the actual ARM64 GNU
  link, embedded G431 build, production vCAN receive-only and live-composition
  tests, exact systemd unit verification, readiness/watchdog/bounded-stop/
  `SIGKILL` restart behavior, and trap cleanup.
- macOS portable passed in 3m54s as the non-authoritative developer check.
- Ubuntu portable passed in 4m03s, including the language gates, vCAN-enabled
  portable Rust surface, home end-to-end lifecycle, site, Helm, and generated
  artifact checks.

The macOS job emitted an unrelated Homebrew tap-trust annotation for an
installed runner tap; every Celerity step passed.

## Deployment artifact

`deploy/helm/celerity-home` owns separate Deployment, Service, PVC, ConfigMap,
and ExternalSecret templates for one application replica with filesystem and
SQLite storage. The ExternalSecret refers to an infrastructure-supplied,
pre-existing External Secrets Operator 1Password `ClusterSecretStore` and
materializes only the vehicle token and HTTPS webhook URL. No static
Kubernetes Secret is rendered. Chart lint, render, and package checks passed;
the chart was not installed because deployment requires separate authority.

## Review evidence

The one normal closeout review found the original missing production runtime
and model-command composition; those blocking gaps were implemented before
this validation. The one cross-provider implementation adversarial review used
Anthropic Claude Opus and produced eight findings: two high, four medium, and
two low. All eight are resolved. Their durable architectural outcomes are
preserved in the [architecture decision records](../adr/README.md), including
controller boot reconciliation, event time, immutable Runs, and the home
service; exact behavior remains enforced by normative contracts and conformance
tests. Local review transcripts and orchestration metadata remain under the
ignored `.agent/` directory, not in repository documentation. Neither completed
review boundary was reopened.

## Explicitly unclaimed

This report does not claim live Haltech/CANTCU decode validation on this car;
physical zero-drive fail-silence; FDCAN electrical behavior; controller, servo,
or model timing under target load; protected power, grounding, EMC, or thermal
behavior; passive radiator-biased return; mechanism travel, settling,
backlash, or obstruction behavior; real model accuracy or safe thermal
envelope; installation; road acceptance; or track acceptance. Those remain the
unexecuted physical/HIL/commissioning items in
`docs/validation/first-deployment-checklist.md`.
