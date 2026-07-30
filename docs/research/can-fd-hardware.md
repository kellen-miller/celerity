# CAN FD hardware and harness baseline

Resolved 2026-07-30 for
[Establish the CAN FD hardware and harness baseline](https://github.com/kellen-miller/celerity/issues/3).

## Decision

Use an
[SK Pang PiCAN FD Duo Isolated, SKU RSP-PICANDUOFD-ISO](https://www.skpang.co.uk/products/pican-fd-duo-isolated-for-raspberry-pi)
on the Raspberry Pi. It provides two independently isolated MCP2518FD channels,
each with an ADM3055E CAN FD transceiver, a switchable 120-ohm termination, a
four-way pluggable terminal, and SocketCAN presentation as `can0` and `can1`.
The manufacturer rates the board for 1 Mbit/s arbitration and 8 Mbit/s data and
currently offers it for sale. Its
[user guide](https://cdn.shopify.com/s/files/1/0563/2029/5107/files/PICAN_FD_DUO_ISO_UGB_10.pdf?v=1669555662)
uses the mainline `mcp251xfd` device-tree overlay for both channels.

Assign and label the channels by physical connector, then prove the mapping on
every image:

- `can0` / J1 is the existing Haltech/CANTCU Classical CAN receive tap. Leave
  JP1 termination open. Configure the MCP2518FD in Classical CAN listen-only
  mode at the measured installed-bus rate. In addition, permanently assert the
  channel's ADM3055E `SILENT` input so a boot, kernel, or configuration failure
  cannot cause a dominant bit on that bus.
- `can1` / J2 is Celerity's separate control network. Leave JP2 termination
  open and terminate in the harness. Configure ISO CAN FD at 500 kbit/s
  arbitration and 2 Mbit/s data, with bit-rate switching enabled. Every
  Celerity application frame is CAN FD from the first deployment; there is no
  Classical-CAN custom-bus phase or compatibility mode.

The published
[Rev B schematic](https://cdn.shopify.com/s/files/1/0563/2029/5107/files/pican_fd_duo_iso_rev_B.pdf?v=1669497811)
shows J1 on SPI chip select 0, its ADM3055E `SILENT` input routed through solder
bridge SB3 to GPIO23, a TVS device at CANH/CANL, and a jumper-selectable
120-ohm resistor. For Celerity, do not depend on a GPIO default: isolate SB3
from the GPIO and hard-strap `SILENT` high to the 3.3 V `VIO` domain with a
documented, inspected rework and a fail-safe pull-up. Analog Devices specifies
that
[ADM3055E silent mode ignores TXD while retaining listen-only reception](https://www.analog.com/en/products/adm3055e.html).
The software `listen-only` setting remains required because it also stops the
CAN controller from acknowledging frames or emitting error frames.

This board protects the Pi from bus ground faults; it does not qualify the
Pi's power input for a car. Power the Pi from a separate, fused automotive
front end and 5 V regulator sized for the selected Pi, not from an unqualified
12 V HAT input.

For the first Nano actuator carrier, use the
[Microchip MCP251863](https://www.microchip.com/en-us/product/MCP251863), an
external SPI MCP2518FD controller and integrated ATA6563 CAN FD transceiver.
Run `VDD`, `VIO`, and transceiver `VCC` at regulated 5 V for the classic Nano
and use a 40 MHz crystal. Microchip specifies direct 2.7 V-to-5.5 V MCU
interfacing, 1 Mbit/s arbitration, 5 Mbit/s data, SPI CRC, ECC-protected
message RAM, a 32-bit timestamp counter, bus-health diagnostics, dominant
timeout, automotive bus transient protection, and bus disconnection while
unpowered in the
[MCP251863 data sheet](https://ww1.microchip.com/downloads/aemDocuments/documents/APID/ProductDocuments/DataSheets/MCP251863-Data-Sheet-DS20006624A.pdf).
The part is in production and is recommended for automotive designs.

The maintained
[ACAN2517FD Arduino driver at commit 74e5b29](https://github.com/pierremolinaro/acan2517FD/tree/74e5b29fdbba16caeddac1d2535f49a24bb8b42c)
supports MCP251863, 40 MHz oscillators, and all Arduino architectures. Its
[ATmega328P/Uno example](https://github.com/pierremolinaro/acan2517FD/blob/74e5b29fdbba16caeddac1d2535f49a24bb8b42c/examples/LoopBackDemoArduinoUno/LoopBackDemoArduinoUno.ino)
explicitly reduces the driver TX and RX software FIFOs to one entry because
the MCU has only 2 KiB of RAM. The classic
[Arduino Nano is the same 16 MHz ATmega328-class, 5 V board](https://docs.arduino.cc/resources/datasheets/A000005-datasheet.pdf).
This makes the Nano feasible for the first bounded actuator, but the complete
firmware must pass SRAM, stack-margin, interrupt-latency, and sustained-bus-load
tests. It is not the long-term controller contract.

## Alternatives considered

### Raspberry Pi interfaces

| Interface | Evidence | Assessment |
| --- | --- | --- |
| PiCAN FD Duo Isolated | Two MCP2518FD controllers and independently isolated ADM3055E transceivers; reinforced isolation; SocketCAN; 1/8 Mbit/s; pluggable terminals and termination jumpers. The product page currently accepts orders. | **Baseline.** The only compared board that combines two disclosed controller/transceiver channels, independent isolation, mainline SocketCAN, and accessible per-channel hardware `SILENT`. It still needs a protected external Pi supply and the can0 fail-silent rework. |
| [PiCAN FD Duo with RTC](https://www.skpang.co.uk/products/pican-fd-duo-board-with-real-time-clock-for-raspberry-pi-3) | Two MCP2518FD/MCP2562FD channels, SocketCAN, 1/8 Mbit/s, optional SMPS, currently offered. The vendor warns that its SMPS version is not suitable for Raspberry Pi 4. | Electrically capable and less expensive, but the channels share the Pi ground and the offered power option is not a general current-Pi vehicle-power answer. Keep only as a bench spare. |
| [Waveshare 2-CH CAN FD HAT](https://www.waveshare.com/product/2-ch-can-fd-hat.htm) | Two MCP2518FD channels, claimed 5 kV isolation, SM24CANB TVS protection, switchable termination, 8-to-28 V input, official-driver configuration, and current direct sale. | Not baseline. The published material does not identify the CAN transceiver or publish automotive transient/reverse-battery qualification for the board's 8-to-28 V input, and it does not document a boot-safe per-channel silent strap. Its [wiki](https://www.waveshare.com/wiki/2-CH_CAN_FD_HAT) also documents a CAN FD failure with one Raspberry Pi OS 6.6.51 image, making exact image validation necessary. |

The MCP2518FD is an in-production ISO 11898-1:2015 controller with Classical
CAN and CAN FD support, ECC message RAM, SPI CRC, diagnostics, and timestamps
([data sheet](https://ww1.microchip.com/downloads/en/DeviceDoc/External-CAN-FD-Controller-with-SPI-Interface-DS20006027B.pdf)).
Linux's mainline
[`mcp251xfd` driver](https://github.com/torvalds/linux/blob/11028ab62899e4191e074ee364c712b77823a9c4/drivers/net/can/spi/mcp251xfd/mcp251xfd-core.c)
identifies MCP2518FD, implements `LISTENONLY` and bus-error reporting, and the
driver's
[ethtool implementation advertises hardware timestamps](https://github.com/torvalds/linux/blob/11028ab62899e4191e074ee364c712b77823a9c4/drivers/net/can/spi/mcp251xfd/mcp251xfd-ethtool.c#L128).
This is mainline support, not a vendor character device or SLCAN tunnel.

### Nano-side controller/transceiver families

| Family | Strengths | Decision |
| --- | --- | --- |
| MCP251863 | One external SPI part integrates the MCP2518FD controller and automotive ATA6563 transceiver; direct 5 V Nano logic; 5 Mbit/s data; CRC/ECC, timestamp, diagnostics, dominant timeout, shorts/thermal/transient protection, and unpowered high impedance. ACAN2517FD explicitly supports it. | **Baseline.** Fewest parts and the strongest demonstrated AVR firmware path. |
| MCP2518FD + [MCP2562FD](https://ww1.microchip.com/downloads/en/DeviceDoc/20005284A.pdf) | Same controller and Arduino driver, with a separate transceiver optimized for 2/5/8 Mbit/s, 1.8-to-5.5 V `VIO`, -58 V to +58 V bus-pin absolute range, ISO 7637 transient ratings, dominant timeout, and unpowered bus disconnect. Both parts are established and the Pi uses the same controller family. | Approved fallback if MCP251863 cannot be procured or routed. It costs more board area and components but does not change firmware or the bus contract. |
| [TI TCAN4550-Q1](https://www.ti.com/product/TCAN4550-Q1) | Active AEC-Q100 5.5-to-30 V CAN FD system-basis chip; integrated M_CAN controller/transceiver, 3.3/5 V I/O, up to 8 Mbit/s, +/-58 V bus-fault protection, watchdog, failsafe modes, and unpowered high impedance. Linux has a mainline [`tcan4x5x` driver](https://github.com/torvalds/linux/blob/11028ab62899e4191e074ee364c712b77823a9c4/drivers/net/can/m_can/tcan4x5x-core.c). | Do not use for the first Nano. No primary source found an Arduino Nano/AVR driver comparable to ACAN2517FD; porting the M_CAN driver would consume the first actuator's risk budget. TI also states that its [SPI has no CRC](https://www.ti.com/lit/fs/sllu312a/sllu312a.pdf). Reconsider with a later MCU carrier and an owned driver. |

## Custom-network electrical contract

### Bit timing and traffic

Use ISO CAN FD at 500 kbit/s nominal arbitration and 2 Mbit/s data with BRS.
This 4:1 setting is supported by every selected controller and transceiver and
keeps substantially more topology margin than their 5-to-8 Mbit/s maxima.
CAN FD necessarily arbitrates at the nominal rate before switching the data
phase; that is not a Classical-CAN transition.

Linux configuration, after proving physical connector-to-interface mapping:

```sh
ip link set can0 down
ip link set can0 type can bitrate <measured-rate> listen-only on \
  berr-reporting on restart-ms 0
ip link set can0 up

ip link set can1 down
ip link set can1 type can bitrate 500000 dbitrate 2000000 fd on \
  berr-reporting on restart-ms 0
ip link set can1 up
```

The kernel documents separate arbitration/data rates, ISO CAN FD as the
default, `fd on`, error states/counters, and manual or automatic bus-off
recovery in
[SocketCAN](https://www.kernel.org/doc/html/latest/networking/can.html).
Keep `restart-ms 0`: Celerity must observe bus-off, revoke model authority,
place actuators in their bounded local fallback, and deliberately requalify
the interface before recovery. Do not silently restore control after a timer.

### Harness topology and termination

- Build a linear 120-ohm-characteristic-impedance twisted-pair trunk. The Pi
  and the farthest controller are the two ends. Every intermediate controller
  has an in/out pass-through connection; no star, ring, dangling service lead,
  or unterminated branch is allowed.
- Keep CANH/CANL paired through every connector and to the transceiver. Budget
  no cable stub longer than 150 mm and no connector-to-transceiver PCB path
  longer than 50 mm. Treat those as Celerity design limits, not a promise that
  an arbitrary 150 mm stub will pass; the installed harness must pass scope and
  load testing.
- Put exactly one 120-ohm termination at each physical end. Prefer split
  termination (two 60.4-ohm 1% resistors with the midpoint AC-coupled to
  `CAN_REF`) in sealed, replaceable endpoint plugs. With power off and nodes
  connected, CANH-to-CANL should measure about 60 ohms. Do not enable either
  HAT termination when harness plugs supply the two endpoints.
- Keep can0's HAT termination disabled. Before and after attaching the tap,
  measure the installed Haltech/CANTCU network and prove the tap has not
  changed its termination. Only an installed-car finding that the Pi is an
  actual unterminated physical endpoint can change that rule.

CAN in Automation recommends a line topology with impedance-matched
termination at both ends and notes that attainable CAN FD data rate depends on
topology and transceivers
([network design](https://can-cia.org/can-knowledge/designing-a-can-network)).
Its CAN FD bit-timing guidance recommends a linear, end-terminated network,
short total length and limited node count even when 5-Mbit/s-qualified
transceivers are used at 2 Mbit/s
([guidance](https://www.can-cia.org/fileadmin/cia/documents/publications/cnlm/march_2018/18-1_p28_recommendation_for_the_canfd_bit-timing_holger_zeltwanger_cia.pdf)).

### Connectors and conductors

Use a sealed, polarized four-position TE DEUTSCH DTM connection at each
custom-network controller:

1. `CAN_L`
2. `CAN_H`
3. `CAN_REF` / controller-electronics ground
4. `SW_FUSED_12V_NODE`

Use the
[DTM04-4P-E004](https://www.te.com/en/product-DTM04-4P-E004.html) and
[DTM06-4S-E004](https://www.te.com/en/product-DTM06-4S-E004.html) housing
family with the specified size-20 contacts and wedgelocks. TE lists these
parts as active, sealed, polarized, -55 degrees C to 125 degrees C,
7.5 A/contact, and IP68/IP6K9K, but not currently available direct from TE.
Confirm authorized-distributor stock, contacts, seals, wedgelocks, and the
correct crimp tool before freezing the harness BOM.

Each node has one pin-side input and one socket-side pass-through so an
energized disconnected trunk does not expose a powered male contact. Pin 4
powers controller electronics only. Actuator or motor current uses a separate
connector, fuse, ground return, and driver so switching current does not flow
through the CAN trunk. Maintain the CAN pair twist to the contact backshell and
strain-relieve the cable.

The Pi HAT remains inside its enclosure. Use short locked terminal pigtails
from J1/J2 to sealed, labeled bulkhead or inline DTM interfaces; bare screw
terminals are not the vehicle service disconnect.

### Bus and power protection

At every custom node, place a
[PESD2CANFD27LT-Q](https://assets.nexperia.com/documents/data-sheet/PESD2CANFD27LT-Q.pdf)
or an electrically reviewed equivalent directly at the CAN connector with the
shortest possible transient return to `CAN_REF`. Nexperia qualifies it to
AEC-Q101 for automotive CAN FD ESD/surge protection and explicitly requires
placement close to the connector. Provide a flow-through footprint for an
automotive CAN FD common-mode choke, but populate it only if EMC testing calls
for it; use zero-ohm links for the baseline.

Every Pi and node battery feed requires, in order:

1. a branch fuse sized to the wire and downstream load;
2. reverse-battery and reverse-current blocking;
3. load-dump/overvoltage clamping or disconnection plus negative-transient
   protection;
4. an automotive-rated buck with cold-crank operating range and UV/OV
   indication;
5. local bulk and high-frequency decoupling.

Validate the assembled front end against the car's measured supply and
ISO 7637-2/ISO 16750-2 pulses; an IC's nominal input range is not validation.
TI's
[12 V/24 V automotive input reference design](https://www.ti.com/tool/TIDA-01167)
identifies overload, reverse polarity, jump start, transients, and suppressed
or unsuppressed load dump as separate requirements. Do not connect raw
vehicle battery to the Nano `VIN`, 5 V pin, Pi 5 V rail, or a HAT merely
because its label includes 12 V.

## Timestamps and diagnostics

Use kernel receive timestamps as the canonical Pi log time on both buses.
Request `SO_TIMESTAMPING_NEW` with raw hardware RX timestamps and software
fallback reporting, and record which source each frame used. The kernel
distinguishes software timestamps at driver ingress from adapter-generated
hardware timestamps in its
[timestamping API](https://docs.kernel.org/networking/timestamping.html).
At image acceptance, `ethtool -T can0` and `ethtool -T can1` must advertise
hardware RX/raw-hardware timestamping; otherwise fail the image rather than
silently changing log semantics.

Also capture:

- CAN error frames using `CAN_RAW_ERR_FILTER`;
- controller state, TX/RX error counters, bus-off count, packet errors, and
  drops from `ip -details -statistics link show can0` and `can1`;
- socket overflow via `SO_RXQ_OVFL`;
- the Nano's MCP251863 error counters, hardware receive-overflow count, reset
  reason, watchdog reason, supply UV/OV state, and last valid-command age in
  every controller heartbeat.

SocketCAN disables error-frame delivery by default but defines filtered error
message frames for physical/MAC faults
([kernel documentation](https://www.kernel.org/doc/html/latest/networking/can.html#network-problem-notifications)).
Diagnostics are therefore an explicit application subscription, not something
ordinary frame logging receives automatically.

## Required failure behavior

- Loss, staleness, overflow, bus-off, controller reset, invalid diagnostics, or
  Pi process death on can1 revokes remote command authority. Each Nano applies
  its local bounded fallback after its heartbeat/command deadline without
  waiting for the Pi.
- A Nano reset holds its actuator fallback and its transceiver in standby
  until local initialization, identity, configuration, diagnostics, and bus
  health pass. It never emits an actuator command merely because CAN traffic
  exists.
- An unpowered Pi or Nano must not load either bus. The selected transceivers
  specify high-impedance/disconnected unpowered behavior, but this must be
  proven on the assembled boards.
- A stuck-low MCU TX output must not hold the bus dominant; the selected node
  transceivers' dominant timeout is required and bench-tested.
- A CAN wire short can still take down the entire non-redundant custom bus.
  Isolation, TVS, and per-node fusing limit collateral electrical damage but
  do not make the communication topology fault tolerant. That event selects
  local actuator fallback.
- can0 never gains a recovery or maintenance transmit mode. A transmit attempt
  must fail in SocketCAN listen-only mode, and the independent ADM3055E silent
  strap must prevent bus drive even if software deliberately leaves
  listen-only mode during a current-limited bench test.

## Hardware acceptance before installation

1. Inspect the exact HAT revision and photograph the can0 `SILENT` rework,
   termination jumpers, labels, and connector mapping.
2. On an isolated bench bus, prove J1 is `can0`, J2 is `can1`, `ethtool -T`
   advertises hardware timestamps, and `ip -details` reports the intended
   modes and exact calculated bit timing.
3. Attempt Classical and FD sends on can0 with both correct and deliberately
   incorrect software configuration while a second analyzer watches the
   wires. No dominant edge or ACK may come from can0.
4. Measure approximately 60 ohms across the powered-off custom trunk; remove
   each endpoint terminator in turn and prove the expected resistance change.
5. Run sustained maximum expected traffic plus `canfdtest`/`cangen` margin
   traffic through every assembled node. Require zero bus errors, overruns,
   socket drops, and heartbeat deadline misses.
6. Scope CANH, CANL, and differential voltage at both ends and the worst
   intermediate node at hot/cold supply corners and with all actuator loads
   switching. The actual cable, connectors, terminations, node count, and
   stubs must pass at 500 kbit/s / 2 Mbit/s.
7. Brown out, power-cycle, unplug, open, and current-limited short each node
   and the Pi. Verify high-impedance unpowered behavior, dominant timeout,
   explicit bus-off handling, local fallback, and deliberate recovery.
8. Validate timestamp ordering, wrap handling, cross-channel skew, and clock
   behavior by injecting a shared observable event onto both channels. Record
   the measured bound in the eventual data-contract decision.
9. Repeat termination and passive-listener checks on the installed
   Haltech/CANTCU bus before enabling Celerity logging. Never discover the
   installed bitrate by transmitting.

## Installed-car facts still unresolved

These facts cannot be responsibly inferred from component documentation:

- exact Raspberry Pi model, OS/kernel image, enclosure, temperature, peak 5 V
  demand, shutdown behavior, and whether the selected HAT mechanically and
  electrically clears that Pi;
- exact Haltech/CANTCU network bitrate, sample point, connector/pinout, wire
  route, topology, existing termination resistance, tap location, common-mode
  range, and whether other devices share it;
- which physical HAT connector enumerates as each interface on the final image
  and whether the Rev B `SILENT` rework is acceptable on the procured revision;
- custom-trunk length, node count and placement, unavoidable stub lengths,
  cable impedance/gauge, routing beside ignition/injector/motor wiring, and
  the worst noise and temperature zones;
- available switched battery feed, fuse location, crank minimum, alternator
  clamp/load-dump behavior, jump-start expectation, ground offsets, and sleep
  current budget;
- Nano actuator electronics current, actuator/motor current and inrush,
  local fallback energy path, watchdog deadline, and whether the Nano has
  enough SRAM/flash/interrupt margin after the complete control firmware;
- authorized-distributor stock and lead time for the HAT, MCP251863,
  protection parts, DTM housings/contacts/seals, and the correct production
  crimp tooling.

Until these are measured, 500 kbit/s / 2 Mbit/s is the design baseline to
validate, not evidence that the installed harness has passed it.
