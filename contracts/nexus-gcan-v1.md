# Celerity Nexus GCAN profile v1

This is a **Celerity-defined transmit profile to configure in NSP**, not a
factory Haltech broadcast or an exported NSP map. It supplies receive-only
thermal telemetry from an eligible Nexus ECU. Haltech documents GCAN custom
messages and logging streams, NSP 1.48+, ECU firmware 1.36+, and a limit of
30 frames/200 signals. Elite and Nexus Rebel are excluded.
([Haltech announcement](https://www.haltech.com/introducing-generic-can/),
checked 2026-10-06)

## Wire layout

Use four consecutive standard 11-bit IDs, `base_id` through `base_id + 3`.
Every frame is Classical CAN, DLC 8, and every 16-bit field is big-endian.
The commissioned `powertrain.haltech.mode = "nexus-gcan-v1"` selects the
version; frames do not contain a version byte. Changing this layout requires
a new profile version and decoder generation. IDs in examples are proposed,
not an allocation verified against the car.

| ID offset | Bytes | Signal | Encoding | Accepted range |
| --- | --- | --- | --- | --- |
| 0 | 0–1 | `coolant_temperature_c` | signed i16 / 10, °C | -50–300 |
| 0 | 2–3 | `intercooler_outlet_air_temperature_c` | signed i16 / 10, °C | -50–300 |
| 0 | 4–7 | Reserved | all zero | zero |
| 1 | 0–1 | `pre_intercooler_air_temperature_c` | signed i16 / 10, °C | -50–300 |
| 1 | 2–3 | `ambient_air_temperature_c` | signed i16 / 10, °C | -50–300 |
| 1 | 4–5 | `pre_intercooler_boost_pressure_kpa` | unsigned u16 / 10, kPa absolute | 0–1000 |
| 1 | 6–7 | `map_kpa_absolute` | unsigned u16 / 10, kPa absolute | 0–1000 |
| 2 | 0–1 | `oil_temperature_c` | signed i16 / 10, °C | -50–300 |
| 2 | 2–3 | `gearbox_oil_temperature_c` | signed i16 / 10, °C | -50–300 |
| 2 | 4–5 | `coolant_pressure_kpa_gauge` | signed i16 / 10, kPa gauge | -100–1000 |
| 2 | 6–7 | `oil_pressure_kpa_gauge` | signed i16 / 10, kPa gauge | -100–2000 |
| 3 | 0–1 | `engine_rpm` | unsigned u16, rpm | 0–20000 |
| 3 | 2–3 | `throttle_percent` | unsigned u16 / 10, % | 0–100 |
| 3 | 4–5 | `vehicle_speed_kph` | unsigned u16 / 10, km/h | 0–500 |
| 3 | 6 | `thermo_fan_1` | unsigned u8, off/on | 0/1 |
| 3 | 7 | `thermo_fan_2` | unsigned u8, off/on | 0/1 |

The decoder also emits `air_temperature_c` from offset 0 bytes 2–3, exclusively
as the existing `thermal-v1` model/safety ABI alias for the commissioned
intercooler **outlet** sensor. Never map a calculated or inlet temperature to
this field. The broad wire ranges reject malformed values; model input ranges
and the safety supervisor impose the tighter control limits.

Offset 0 is required for control. Offsets 1–3 are supplementary: omit a whole
frame when its sensors are not commissioned. Never fill an absent or failed
sensor with a plausible zero. Such values would pass range validation. The
ECU map must stop the affected frame or encode an out-of-range value on sensor
failure; prove this behavior in NSP/on the bench before live use. The profile
does not carry sensor-health flags or acquisition timestamps.

A bad DLC, reserved byte, boolean, or out-of-range field rejects the entire
frame, with no partial signal updates. Rejected frames do not refresh cached
values. Previously valid values age out at the configured runtime freshness
window; rejection does not immediately revoke still-fresh evidence. Raw
frames and rejection reasons remain in the canonical Run.

## Startup selection and timing

```toml
[powertrain]
decoder_generation = 2

[powertrain.haltech]
mode = "nexus-gcan-v1"
base_id = 1536 # proposed 0x600–0x603
maximum_period_ms = 100

[powertrain.cantcu]
mode = "disabled"
```

`maximum_period_ms` is the commissioned worst-case interval for every enabled
profile frame, including jitter. It must be positive, and both
`runtime.input_stale_after_ms` and `runtime.model_signals_stale_after_ms` must
exceed it. The example 100 ms is a proposed budget, not measured ECU behavior.
More frequent CAN transmission does not prove more frequent sensor acquisition.

Startup rejects standard-ID overflow, overlap with any decoded Haltech
Broadcast v2 ID, and overlap with the configured seven-frame CANTCU stream.
Check the complete installed network separately: software cannot know every
other accessory's IDs. CANTCU keeps its independently selected protocol.

In Nexus mode, factory Haltech broadcast frames remain raw evidence and cannot
overwrite GCAN signals. In `broadcast-v2` mode, GCAN frames remain opaque.
Record the ECU map, sensor locations/calibrations, selected IDs, decoder
generation, and measured timing together. Existing startup bundles must add
the explicit `powertrain.haltech` table; use `mode = "broadcast-v2"` to retain
their previous decoding and 200 ms period budget.

## NSP setup and bench acceptance

1. On an eligible ECU, update NSP/firmware and enable GCAN on the Haltech bus.
   Keep this Celerity profile on the existing 1 Mbit/s Classical CAN observation
   bus; other bus rates require a separate transport change.
2. Create transmit messages using the table, matching signedness, byte order,
   scaling, units, and physical channel selection. Configure zero reserved
   bytes. There is no supplied native NSP import file; verify the installed
   NSP can express each mapping before commissioning.
3. Capture the emitted bytes and compare them with independently known sensor
   readings and `crates/control-core/tests/nexus_gcan.rs`. Verify cold/negative
   temperatures, absolute versus gauge pressure, and fan states.
4. Measure frame gaps under normal load and ECU restart. Unplug each required
   sensor and stop the required stream; prove bad/missing evidence reaches
   controller-local fallback within the freshness budget. Plausible but frozen
   ECU values cannot be detected from CAN arrival times alone.
5. Confirm the tap remains hardware silent, IDs are collision-free, and CANTCU
   integration is unaffected. Software/vCAN tests do not establish this.

The live runtime records every decoded sample with its raw source sequence and
decoder generation, including supplementary channels absent from the active
model's input list. Additional channels do not change the current model's
input order or enable a newly trained model. Existing model activation remains
explicit. This profile adds no ECU commands or bridge to the separate CAN FD
actuator network.
