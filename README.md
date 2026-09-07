# Celerity

Celerity is a bounded vehicle-control runtime and dedicated duct-controller
firmware for an FD RX-7. It keeps one visible path from receive-only powertrain
observation through authority gates, deterministic or model-assisted policy,
leased controller commands, and immutable Run evidence.

This repository is pre-deployment software. It does not claim installed
hardware, electrical fail-silence, target timing, vehicle-trained model quality,
thermal-envelope validation, or road/track acceptance.

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
- `services/home`: FastAPI, SQLite/filesystem archive, managed jobs, models, and
  bounded webhook delivery.
- `contracts/celerity/v1/celerity.proto`: the one cross-language IDL. Rust,
  Python, and TypeScript bindings are committed and checked for regeneration.
- `contracts/controller-can-v1.md` and `config/schema-v1.md`: transport and
  startup-configuration adapters that retain their exact external formats.

## Development

Install the checked-in Rust toolchain, `uv`, and Node/npm. Run `npm ci` in
`crates/vehicle-ui/web`, then:

```sh
./scripts/check-protos
./scripts/check-rust --portable
./scripts/check-python
./scripts/check-ui
```

When `contracts/celerity/v1/celerity.proto` changes, regenerate the committed
Rust, Python, and TypeScript bindings with `./scripts/generate-protos`. CI runs
the regeneration in a temporary tree and fails if any checked-in binding is
missing or stale.

Linux CI additionally creates separate `vcan-powertrain` and
`vcan-controller` interfaces and runs `./scripts/check-linux`, including exact
systemd-unit lifecycle checks and the ARM64 link. `vcan` demonstrates software
interface separation; it does not prove physical listen-only behavior.

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
