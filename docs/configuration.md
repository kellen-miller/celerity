# Configuration

`ValidatedBundle::load` is the runtime configuration boundary. Startup TOML is
strict: unknown fields, mode/composition mismatch, duplicate ownership, timing
violations, unsafe split/slew limits, and nonmonotonic policy tables fail before
external adapters are opened. Environment variables supply deployment paths and
secrets only; they do not change control behavior.

Use `config/examples/simulation.toml` or `config/examples/replay.toml` as the v1
shape. No live example is shipped: a live bundle must name the real logical
powertrain-RX and actuator-CAN roles, commissioned controller identity, concrete
interfaces, and platform watchdog behavior. The runtime does not assume `can0`,
`can1`, or Raspberry Pi hardware.

The sync section declares spool ownership, acknowledged retention count, the
home interface and expected default gateway, home API URL, and bearer-token
path. A link is "home" only when it is up and its default route uses that exact
gateway. HTTP success never grants vehicle authority.
