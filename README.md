# Celerity

Celerity is a bounded vehicle-control runtime and dedicated duct-controller
firmware for an FD RX-7. It keeps one visible path from receive-only powertrain
observation through authority gates, deterministic or model-assisted policy,
leased controller commands, and immutable Run evidence.

This repository is pre-deployment software. It does not claim installed
hardware, electrical fail-silence, target timing, vehicle-trained model quality,
thermal-envelope validation, or road/track acceptance.

Nexus ECU upgrades can use the explicit
[Celerity GCAN telemetry profile](contracts/nexus-gcan-v1.md), with commissioned
sensor mappings and timing. The powertrain connection remains receive-only.

## Repository shape

- `crates/control-protocol`: `no_std` exact CAN v1 codec and golden vectors.
- `crates/control-core`: configuration, powertrain decoding, executor, controller
  emulator, Run, replay, ONNX inference, optimizer, and startup-only A/B model slots.
- `crates/vehicle-runtime`: Linux CAN adapters plus runtime, simulation/replay,
  and vehicle binaries.
- `crates/vehicle-diagnostics`: typed read-only status contract and bounded
  Unix-socket transport.
- `crates/vehicle-ui`: loopback-only Rust status observer, embedded local
  Svelte panel, and `celerity-ui` binary.
- `crates/sync`: authority-free Run/model reconciliation with SQLite.
- `firmware/duct-controller`: STM32G431 Embassy image and host conformance.
- `crates/home-service`: Rust home HTTP API, SQLite/filesystem archive, durable
  job supervision, model staging and bounded webhook delivery.
- `services/home`: Python ML data preparation, PyTorch training, ONNX export and
  evaluation, plus deterministic model fixtures.
- `contracts/celerity/v1/celerity.proto`: the one cross-language IDL. Rust,
  Python, and TypeScript bindings are committed and checked for regeneration.
- `contracts/controller-can-v1.md` and `config/schema-v1.md`: transport and
  startup-configuration adapters that retain their exact external formats.

## Development

Install the checked-in Rust toolchain, `uv`, and Node/npm. Run `npm ci` in
`crates/vehicle-ui/web`, then:

```sh
./scripts/check-protos
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features --all-targets
cargo test --locked --workspace --all-features --doc
cargo build --locked --workspace --all-targets --all-features
./scripts/check-rust-targets --portable
./scripts/check-python
./scripts/check-ui
```

When `contracts/celerity/v1/celerity.proto` changes, regenerate the committed
Rust, Python, and TypeScript bindings with `./scripts/generate-protos`. CI runs
the regeneration in a temporary tree and fails if any checked-in binding is
missing or stale.

CI runs the workspace Cargo checks through the shared `rust-check` action after
preparing the toolchain and platform dependencies. Formatting and Clippy use
the action's bundled policy alongside the workspace lint settings. Rust and
end-to-end, Python and artifacts, and UI checks run in parallel on both macOS
and Ubuntu. The required portable checks collect those results. Pushes to `main`
refresh caches for subsequent pull requests. Linux training uses CPU-only PyTorch
instead of downloading CUDA packages. A separate target job builds ARM64 services
and controller firmware once, in parallel with the host checks.
Locally, `check-rust-targets --portable` builds firmware and links ARM64 when its
cross-linker is available.

`check-e2e` runs the ignored cross-language acceptance test with the same workspace
features as the normal Rust tests. Cargo covers the Rust home service;
`check-python` covers numerical training and ONNX tooling;
install UI dependencies with `npm ci` before running `check-ui` locally.

Linux CI creates separate `vcan-powertrain` and `vcan-controller` interfaces
before the workspace checks. The Linux service job runs `./scripts/check-systemd`
against native release binaries, with its own build cache. Expected access denials
and startup retries are quiet; passing checks print explicit results. Locally,
`./scripts/check-linux` runs both target builds and the service checks. `vcan`
demonstrates software interface separation; it does not prove physical
listen-only behavior.

The local panel is built into `celerity-ui`; Node is a build/CI dependency,
not a vehicle runtime dependency. With the independently managed viewer
enabled on the vehicle host, open `http://127.0.0.1:8080/` locally.

Run the production runtime logic with synthetic evidence:

```sh
cargo run -p vehicle-runtime --bin celerity -- simulate config/examples/simulation.toml
cargo run -p vehicle-runtime --bin celerity -- replay .tmp/runs/healthy-startup --verify
```

See [architecture decisions](docs/adr/README.md),
[configuration](docs/configuration.md), [vehicle services](docs/service-install.md),
[home deployment](docs/home-deployment.md), and the
[first physical deployment checklist](docs/validation/first-deployment-checklist.md).
