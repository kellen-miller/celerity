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

The required `powertrain.haltech` table selects the ECU telemetry profile.
Existing bundles must add `mode = "broadcast-v2"` to retain fixed Haltech
decoding. For an eligible Nexus configured with Celerity's custom stream, use
`mode = "nexus-gcan-v1"`, a commissioned `base_id`, and `maximum_period_ms`.
Both input/model freshness windows must exceed the selected profile's period
budget (200 ms for Broadcast v2). Nexus IDs must avoid decoded Haltech IDs and
the configured CANTCU range. Only the selected ECU profile produces signals.

See [the Nexus wire and NSP setup contract](../contracts/nexus-gcan-v1.md) and
`config/examples/nexus-gcan-simulation.toml`. This example validates the bundle
shape; its synthetic scenario does not exercise actual ECU CAN transmission.

```toml
[powertrain.haltech]
mode = "nexus-gcan-v1"
base_id = 1536
maximum_period_ms = 100
```

The required `powertrain.cantcu` table makes CANTCU reception explicit. Use
`mode = "disabled"` when its Default CAN Datastream is not commissioned. Use
`mode = "default"` with the measured decimal `base_id` only after confirming
the seven-frame range is present and collision-free; bundle loading rejects
standard-ID overflow and overlap with decoded Haltech Broadcast v2 IDs.

```toml
[powertrain.cantcu]
mode = "default"
base_id = 1280
```

The sync section declares spool ownership, acknowledged retention count, the
home interface and expected default gateway, home API URL, and bearer-token
path. A link is "home" only when it is up and its default route uses that exact
gateway. HTTP success never grants vehicle authority.
