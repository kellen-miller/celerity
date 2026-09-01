# Architecture decision records

These records preserve durable architectural choices and their rationale.
Normative wire formats, schemas, and configuration remain in `contracts/` and
`config/`; ADRs link to those sources instead of duplicating them.

- [0001: Name Rust crates by responsibility](0001-name-rust-crates-by-responsibility.md)
- [0002: Isolate vehicle authority and powertrain observation](0002-isolate-authority-and-observation.md)
- [0003: Reconcile controller state after every boot](0003-reconcile-controller-after-every-boot.md)
- [0004: Separate event order from observation time](0004-separate-event-order-from-observation-time.md)
- [0005: Keep Runs immutable and deletion acknowledgement-bound](0005-keep-runs-immutable.md)
- [0006: Activate learned models only at startup](0006-activate-models-only-at-startup.md)
- [0007: Run vehicle services directly under systemd](0007-run-vehicle-services-under-systemd.md)
- [0008: Keep the home service operationally small](0008-keep-the-home-service-small.md)
- [0009: Keep local status outside live authority](0009-keep-local-status-outside-live-authority.md)
