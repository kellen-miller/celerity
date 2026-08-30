# Startup bundle schema v1

Every process startup loads one immutable TOML bundle through the same strict
parse, normalization, and validation path. Required top-level keys are
`schema_version`, `generation`, `mode`, `runtime`, `powertrain`, `controllers`,
`duct`, `model`, `run_storage`, `diagnostics`, `sync`, and `composition`.
Unknown keys and missing dependencies are errors.

`mode` is exactly `simulation`, `replay`, or `live`, and the tagged
`composition` must match the requested CLI mode. Simulation names one scenario.
Replay names one Run and `verify` or `explore`. Live names commissioned logical
roles and Linux interfaces; this repository deliberately ships no usable live
example before commissioning.

Validation rejects duplicate actuator ownership, invalid identities,
nonmonotonic 3x3 policy tables, split or slew values beyond absolute code
bounds, timing/ordering violations, insufficient Run free-space thresholds,
model ABI mismatch, and mode mismatch. Environment variables may supply only
deployment paths and secrets, never control behavior.

The required `powertrain.cantcu` table is tagged by `mode`. `disabled` accepts
no base ID. `default` requires the commissioned decimal base ID for CANTCU's
seven-frame Default CAN Datastream. Validation rejects standard-ID overflow or
overlap with every Haltech Broadcast v2 ID decoded by this generation.

The `sync` section declares spool ownership, retention, home interface,
expected default gateway, home API URL, and credential path. Home presence means
the configured Linux interface is up and its default route uses the configured
gateway. Periodic HTTP retry recovers missed notifications but does not redefine
home presence.
