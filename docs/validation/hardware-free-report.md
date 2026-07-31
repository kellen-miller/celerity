# Hardware-free validation report

Date: 2026-07-31
Base commit: `a993404`
Branch: `feat/celerity-runtime`
Validated PR/head: #17 / `87d5d49`
GitHub Actions run: `30664026585`
Configuration fixtures: `config/examples/simulation.toml`, `config/examples/replay.toml`
Contract schemas: controller CAN v1, Run v1, home v1, model bundle v1

## Executed locally on macOS

- Controller protocol: 13 production frame types plus deterministic min/max
  golden vectors; encode/decode and rejection tests pass. The firmware-native
  test consumes the same raw vectors and drives Configuration, Runtime Lease,
  Command, and independent lease expiry through the embedded state machine.
- Runtime/emulator/firmware host behavior: distinct runtime and command leases,
  rejected-command non-refresh, reboot reconciliation, deterministic logical
  time, stale fallback, Run sealing/recovery/replay, model optimizer, and A/B
  slot corruption/recovery tests pass.
- Embedded: the real Embassy executor initializes protected PWM, exact FDCAN
  filters, 500 kbit/s / 2 Mbit/s+BRS timing, transceiver standby, and the
  independent watchdog. The STM32G431 `thumbv7em-none-eabihf` release image
  links; the checked
  message-RAM layout uses 181 of 256 words with 75 spare. This is build/resource
  evidence, not target runtime evidence.
- Model fixture: the deterministic real ONNX identity graph exports and loads
  with `tract`; generation is checked for byte stability.
- Live-edge software: strict powertrain and actuator netdevice qualification,
  raw-hardware/software-fallback timestamp selection, CAN error subscription,
  receive-overflow enablement, separate CAN FD+BRS controller transport,
  published Haltech big-endian decoding, opt-in collision-checked CANTCU
  little-endian decoding, and opaque unknown-frame preservation pass. The
  Linux-only implementation is also cross-target Clippy-clean locally.
- Sync/home: Linux route parsing fixtures, authority-isolation metadata,
  content-addressed transfer/acknowledged retention, exact Run admission,
  managed job completion/no-change/rejection, orphaned-job restart recovery,
  authenticated operator jobs, durable training-failure/candidate-rejection/
  demotion/rollback events, required HTTPS webhook delivery, and webhook-failure
  independence pass. The production HTTP exercise starts
  the real ASGI app, uploads and acknowledges a complete Run, trains/evaluates
  the TCN, downloads the exact desired digest, stages it atomically, and then
  executes inactive-slot activation/corruption/rollback checks.
- Site: Prettier, ESLint, `svelte-check`, Vitest, and static production build pass.

Commands:

```sh
./scripts/check-rust --portable
./scripts/check-python
./scripts/check-site
cargo run -p celerity-protocol --features std --bin write-golden-vectors -- contracts/golden/controller-can-v1
uv run --project services/home python -m celerity_home.export_fixture --output contracts/golden/model-v1
./scripts/check-artifacts
./scripts/check-e2e
CC_x86_64_unknown_linux_gnu=clang \
  CFLAGS_x86_64_unknown_linux_gnu=--target=x86_64-unknown-linux-gnu \
  cargo clippy --target x86_64-unknown-linux-gnu -p celerity \
  --all-targets -- -D warnings
git diff --check
```

These commands execute 48 portable Rust tests, the production HTTP exercise,
eleven Python tests, two site tests, artifact byte-stability checks, the G431
release link, and the static site build. The ARM64 cross-link and
`./scripts/check-linux` were not executed locally because the macOS host lacks
the Linux linker, `vcan`, and host systemd. The site build reports three
low-severity npm dependency advisories; the checked formatter, linter, type
checker, tests, and build pass.

## Executed in GitHub Actions

PR #17 head `87d5d49` passed run `30664026585` in all three jobs:

- Linux completed in 12m28s, including the actual ARM64 GNU link, production
  vCAN receive-only/interface-separation test, exact systemd unit verification,
  readiness/watchdog/bounded-stop/SIGKILL-restart lifecycle, and trap cleanup.
- macOS portable completed in 5m36s.
- Ubuntu portable completed in 5m21s.

Both portable jobs also linted, rendered, and packaged
`deploy/helm/celerity-home`. The chart owns separate Deployment, Service, PVC,
ConfigMap, and ExternalSecret templates. The ExternalSecret requires an
infrastructure-supplied name for a pre-existing External Secrets Operator
1Password `ClusterSecretStore` and materializes only the vehicle token and HTTPS
webhook URL. No static Kubernetes Secret is rendered. The chart was not
installed in a cluster, so no cluster rollout claim is made.

## Explicitly unclaimed

Live Haltech/CANTCU decode validation on this car; physical zero-drive
fail-silence; FDCAN electrical behavior; controller/servo timing under target
load; power/EMC/thermal behavior; passive return; mechanism travel, settling,
and backlash; real model accuracy and thermal envelope; target ARM deadlines;
installation; road acceptance; and track acceptance.
