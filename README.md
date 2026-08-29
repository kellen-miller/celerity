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
  and diagnostics binaries.
- `crates/sync`: authority-free Run/model reconciliation with SQLite.
- `firmware/duct-controller`: STM32G431 Embassy image and host conformance.
- `services/home`: FastAPI, SQLite/filesystem archive, managed jobs, models, and
  bounded webhook delivery.
- `site`: static public architecture walkthrough, never a driver interface.
- `contracts`: fixed controller, Run, home API, webhook, configuration, and
  model-bundle v1 boundaries.

## Development

Install the checked-in Rust toolchain, `uv`, and Node/npm, then run:

```sh
./scripts/check-rust --portable
./scripts/check-python
./scripts/check-site
```

Linux CI additionally creates separate `vcan-powertrain` and
`vcan-controller` interfaces and runs `./scripts/check-linux`, including exact
systemd-unit lifecycle checks and the ARM64 link. `vcan` demonstrates software
interface separation; it does not prove physical listen-only behavior.

Run the production runtime logic with synthetic evidence:

```sh
cargo run -p vehicle-runtime --bin celerity -- simulate config/examples/simulation.toml
cargo run -p vehicle-runtime --bin celerity -- replay .tmp/runs/healthy-startup --verify
```

See [configuration](docs/configuration.md), [vehicle services](docs/service-install.md),
[home deployment](docs/home-deployment.md), and the
[first physical deployment checklist](docs/validation/first-deployment-checklist.md).
