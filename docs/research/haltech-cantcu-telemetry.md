> **Supersession:** This is decoder provenance, not installed-car validation. Live Celerity remains RX-only and enables only a measured, collision-free CANTCU datastream; unknown frames remain raw evidence.

# Haltech Elite 2500 and CANTCU telemetry boundary

Research current through 2026-07-30. Sources are first-party Haltech,
CANformance, component-vendor, and Linux documentation.

## Decision

Celerity should treat the existing Haltech/CANTCU CAN3 network as an
untrusted, receive-only telemetry source:

1. Connect a dedicated `can_powertrain` controller/transceiver to the existing
   1 Mbit/s Classical CAN bus as a short, unterminated stub.
2. Permanently hold that transceiver in hardware silent mode. Also configure
   the Linux CAN controller as listen-only, but do not count software mode as
   the safety boundary.
3. Decode only Haltech ECU Broadcast CAN v2 and, if it is deliberately enabled
   and assigned a collision-free base ID, the CANTCU Default CAN Datastream.
   Do not decode CANTCU CAN1/CAN2 traffic or infer the raw frames used by its
   Haltech I/O-box emulation.
4. Use a separate controller, transceiver, connector, twisted pair, and
   termination for `can_control`, the Celerity CAN FD network. Do not create a
   kernel, userspace, or application bridge between the interfaces. Only
   `can_control` may transmit.

This preserves the settled authority boundary: Haltech owns the engine,
CANTCU and the OEM TCM own the transmission, and Celerity can lose all power
or software integrity without being electrically able to command either.

The Haltech bus is 1 Mbit/s with standard 11-bit identifiers and big-endian
payloads. Haltech calls its published stream the ECU Broadcast CAN Protocol
and documents it for Elite ECUs. The current v2 document requires Elite
firmware 3.11 or later, warns that listed rates are not guaranteed, and warns
that non-Haltech devices can collide with future IDs.
([Haltech ECU Broadcast CAN Protocol v2, pp. 2-3 and 19](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067))

CANTCU uses CAN1 and CAN2 for the transmission and CAN3 for car/ECU
integration. Its Haltech integration connects CAN3 to the 1 Mbit/s Haltech
bus. CAN1 and CAN2 are terminated inside CANTCU; CAN3 is not.
([CANTCU pinout](https://wiki.canformance.net/en/CANTCU/hardware/CANTCU_pinout),
[CANTCU Haltech integration](https://wiki.canformance.net/CANTCU/integrations/supportedECUs/haltech))

## Haltech telemetry useful to thermal control

All multi-byte values below are big-endian. Byte ranges are inclusive and
zero-based. Temperature values in kelvin use `value = raw / 10`; converting
those values to Celsius therefore uses `value = raw / 10 - 273.1`. Generic
sensor scaling depends on the sensor type configured in Haltech.
([protocol addressing and scaling, pp. 3-4 and 21](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067))

| ID | Nominal rate | Relevant fields |
| --- | ---: | --- |
| `0x360` | 50 Hz | bytes 0-1 RPM (`raw`); 2-3 manifold pressure, kPa absolute (`raw / 10`); 4-5 throttle position, % (`raw / 10`); 6-7 coolant pressure, kPa gauge (`raw / 10 - 101.3`) |
| `0x361` | 50 Hz | bytes 0-1 fuel pressure and 2-3 oil pressure, kPa gauge (`raw / 10 - 101.3`); 4-5 engine demand, % (`raw / 10`) |
| `0x36C` | 20 Hz | four wheel speeds in bytes 0-1, 2-3, 4-5, and 6-7, km/h (`raw / 10`) |
| `0x36E` | 20 Hz | bytes 0-1 engine-limiting-active boolean |
| `0x370` | 20 Hz | bytes 0-1 vehicle speed, km/h (`raw / 10`) |
| `0x372` | 10 Hz | bytes 0-1 battery voltage (`raw / 10`); 6-7 barometric pressure, kPa absolute (`raw / 10`) |
| `0x373`-`0x375` | 10 Hz | EGT sensors 1-12, two-byte kelvin values (`raw / 10`) |
| `0x376` | 10 Hz | bytes 0-1 ambient-air temperature, kelvin (`raw / 10`); humidity fields occupy the rest |
| `0x377` | 50 Hz | bytes 0-1 pre-intercooler boost pressure, kPa (`raw / 10`) |
| `0x3E0` | 5 Hz | coolant, air, fuel, and oil temperatures in consecutive two-byte kelvin fields (`raw / 10`) |
| `0x3E1` | 5 Hz | gearbox-oil, differential-oil, and pre-intercooler air temperatures in bytes 0-1, 2-3, and 6-7, kelvin (`raw / 10`) |
| `0x3E4` | 5 Hz | byte 3 bits 3-0 thermo-fans 4-1; byte 7 bit 7 check-engine light |
| `0x3E7`-`0x3E9` | 20 Hz | generic sensors 1-10 as consecutive unsigned 16-bit values |
| `0x469` | 5 Hz | bytes 0-1 ECU temperature, kelvin (`raw / 10`) |
| `0x6F3` | 5 Hz | byte 5 engine-protection severity; bytes 6-7 engine-protection reason encoded as an OBD-II DTC |
| `0x6F4` | 100 Hz | byte 1 low nibble engine state: stopped, cranking, idling, running, or limiting |
| `0x6F7` | 10 Hz | bytes 4-5 calculated-air temperature, kelvin (`raw / 10`) |

The IDs, rates, fields, and conversions in this table come directly from the
Haltech protocol table.
([engine/load/pressure fields, pp. 4-5](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067),
[speed, ambient, and temperature fields, pp. 6-7](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067),
[states and generic sensors, pp. 8-10](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067),
[diagnostic and calculated fields, pp. 11 and 17-18](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067))

The thermal model must not assume that Haltech's generic `Air Temperature`
channel is physically post-intercooler. It is useful as charge-air
temperature only after the installed sensor location is verified. Likewise,
the pre-intercooler and generic-sensor fields exist in the protocol but may be
unset in this car. Generic sensors 1-10 can carry switch, voltage, pressure,
absolute pressure, temperature, or percentage data; the wire payload does not
identify which type was selected. Their configured conversions are switch
`0/1`; voltage `raw / 1000` V; gauge pressure `raw / 10 - 101.3` kPa;
absolute pressure `raw / 10` kPa; temperature `raw / 10` K,
`raw / 10 - 273.1` C, or `raw * 0.18 - 255.372` F; and percentage
`raw / 10`.
([generic sensor table, p. 21](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067))

The thermal messages document neither source timestamps nor sequence
counters. Celerity must timestamp frames on receipt, enforce freshness per
signal, reject impossible values, and enter fallback when required inputs
become missing, stale, invalid, or out of distribution. Because Haltech says
its rates are not guaranteed, final stale thresholds must be based on measured
car captures with margin, not equality to the nominal periods.
([protocol notes, p. 19](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067))

## CANTCU integration boundary

### Haltech integration

The current CANformance instructions call this the "Old IOBox" integration.
It first appeared in CANTCU 1.0.132, emulates Haltech I/O Expander Box B (and
optionally Box A), and consumes the Haltech Supported Dash stream. The
documented Haltech-to-CANTCU values are engine RPM, pedal/throttle, MAP, wheel
speeds, brake switch, coolant temperature, and engine-oil temperature. The
documented CANTCU-to-Haltech values are gearbox gear and mode, gearbox-oil
temperature, gearbox delta torque, cut/blip percentages, and cut/blip states.
([CANTCU Haltech integration](https://wiki.canformance.net/CANTCU/integrations/supportedECUs/haltech))

The Box B values are exposed to Haltech at 50 Hz through four virtual analog
and four virtual digital-pulse inputs: gear, oil temperature (-40 to 160 C
mapped to 0-5 V), cut/blip trigger, drive mode, cut %, blip %, clutch slip,
and converter slip. Optional Box A adds TCU input RPM, output RPM, target RPM,
and delta torque, also at 50 Hz.
([CANTCU Haltech integration, I/O and extra-values tables](https://wiki.canformance.net/CANTCU/integrations/supportedECUs/haltech))

CANformance documents those semantics but does not publish the raw CAN IDs,
byte layout, checksums, or counters emitted by its Haltech I/O-box emulation.
Celerity must not infer them. If a TCU value is normalized and rebroadcast by
the Elite through a published Haltech Broadcast field, it may be accepted only
after a live capture proves the mapping on this car.

### Documented CANTCU wire stream

CANTCU separately documents a Default CAN Datastream on CAN3. It is the only
current first-party, general-purpose CANTCU wire format suitable for direct
Celerity decoding. It has seven consecutive Classical CAN frames; activation,
base ID, and CAN speed are configurable; identifiers are standard 11-bit; and
all 16-bit values are little-endian. This endianness is the opposite of the
Haltech broadcast and requires a separate decoder.
([CANTCU Default CAN Datastream](https://wiki.canformance.net/CANTCU/software/config/candefaultout))

| ID relative to configured base | Rate | Contents |
| --- | ---: | --- |
| `Base` | 50 Hz | engine, TCU input, and TCU output RPM (`uint16`, factor 1); pedal position and brake switch (`uint8`) |
| `Base+1` | 50 Hz | engine and target torque (`int16`); shift-in-progress, intervention, shift-cut %, and blip % (`uint8`) |
| `Base+2` | 20 Hz | driven wheel speed (`uint16`); gear, shifter state, clutch/converter slip, and paddles (`int8`/`uint8`) |
| `Base+3` | 10 Hz | four digital inputs and four digital outputs (`uint8`) |
| `Base+4` | 10 Hz | four 0-5 V analog inputs (`uint16`, factor 0.001 V) |
| `Base+5` | 10 Hz | last-shift time; TCU oil temperature (`int8`, 1 C); drive modes; launch state; supply voltage (`uint16`, factor 0.001 V) |
| `Base+6` | 50 Hz | TCU target RPM (`uint16`), delta RPM and delta torque (`int16`) |

The configured base ID must avoid every live Haltech, CANTCU emulation, and
other-device ID. Enabling or moving the stream is an installation/configuration
operation, not a Celerity runtime action.
([CANTCU Default CAN Datastream](https://wiki.canformance.net/CANTCU/software/config/candefaultout),
[Haltech protocol collision warning, p. 19](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067))

### Firmware floor

- Haltech ECU Broadcast v2 requires Elite firmware 3.11 or later. Haltech's
  newer multi-master CAN behavior requires Elite 3.12.x or later and NSP
  1.40.x or later.
  ([broadcast protocol, p. 2](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067),
  [Haltech Multi-master CAN](https://support.haltech.com/portal/en/kb/articles/multi-master-can))
- CANTCU's Haltech integration first appeared in 1.0.132. Version 1.0.135
  fixed an approximately 20 C Haltech coolant/oil-temperature error, 1.0.141
  changed the Haltech pedal source to actual pedal position and improved
  integration behavior, and 1.0.142 fixed Haltech DriveLogic scaling and a
  wheel-speed issue. The latest stable version documented on the research date
  is 1.0.142, released 2026-03-09.
  ([CANTCU Haltech integration](https://wiki.canformance.net/CANTCU/integrations/supportedECUs/haltech),
  [CANTCU Configurator releases](https://wiki.canformance.net/CANTCU/software/configurator))

CANformance does not state a minimum firmware version for the Default CAN
Datastream itself.

The documented compatibility floors are Elite 3.11 and CANTCU 1.0.132. Because
1.0.135 corrected a thermal-signal defect, Celerity's deployment baseline
should instead be CANTCU 1.0.142, with Elite 3.12/NSP 1.40 used when the
installed Haltech network relies on multi-master support. Firmware upgrades
are not implied by this research: inventory and back up the running
configuration before deciding whether to change either controller.

## Diagnostics

The passive Haltech stream exposes useful health context: trigger error count
(`0x369`), engine-limiting active (`0x36E`), check-engine state (`0x3E4`),
engine-protection severity and OBD-II reason (`0x6F3`), and engine state
(`0x6F4`).
([Haltech protocol table](https://support.haltech.com/portal/api/kbArticles/309315000127455298/locale/en/attachments/8whone12beb10b56646c7893d210fa06b2b86/content?inline=true&portalId=edbsndab83bda6a605a18494b81368a73ed74e00f5941b9c7dc264955a9257f1b8067))

CANTCU Configurator can actively read and clear transmission fault codes and
read type, VIN, immobilizer state, software versions, differential ratio, and
some adaptations. CANformance warns that another diagnostic tester on CAN1
interferes with those functions. These are workshop/service operations, not a
Celerity runtime telemetry interface, and Celerity must never issue or proxy
them.
([CANTCU fault codes](https://wiki.canformance.net/CANTCU/software/diagnostics/faultcodes),
[CANTCU transmission information](https://wiki.canformance.net/CANTCU/software/diagnostics/diaginfo))

The Default CAN Datastream table does not define CANTCU/TCM DTCs, a limp-mode
bit, a source timestamp, a sequence counter, or a message CRC. Those values
must remain unavailable to runtime policy unless CANformance publishes a
first-party format later.

## Native logging and export

The Elite 2500 has 2 MB of on-board log memory, supports up to 40 channels,
and supports per-channel sample periods down to 5 ms (200 Hz). Duration is
configuration-dependent; ESP estimates it. Logging stops when memory fills
and does not resume until the log memory is cleared. ESP can extract/play back
on-board logs, log all available channels to a connected laptop, and export a
recorded log through `Options > Export`.
([Elite 2500 product specification](https://www.haltech.com/product/ht-151300-elite-2500-ecu/),
[Haltech Elite datalogging](https://support.haltech.com/portal/en/kb/articles/datalog-elite))

Elite firmware 3.x uses Haltech NSP; firmware 2.x uses ESP. Haltech documents
both desktop packages for Windows 10/11, and its current NSP build requires an
x64 processor rather than ARM.
([Haltech software downloads](https://store.haltech.com/downloads/software/))

CANTCU's latest stable Configurator is likewise a Windows 10/11 application.
It provides realtime values, a datalogger/viewer, firmware and transmission
flashing, and diagnostics. Its CAN Analyzer requires a USB-connected CANTCU,
shows received CAN3 traffic, and saves that raw traffic as CSV. The stable
documentation does not state decoded-log duration, channel-count, sample-rate,
or export-format guarantees.
([CANTCU Configurator](https://wiki.canformance.net/CANTCU/software/configurator),
[CANTCU CAN Analyzer](https://wiki.canformance.net/CANTCU/software/configurator/cananalyzer))

CANTCU 1.1 beta lists internal logging as a new function, but the release is
announced for August 2026 and is not the stable 1.0.142 baseline on the
research date. Celerity must not depend on it.
([CANTCU Configurator changelog](https://wiki.canformance.net/CANTCU/software/configurator))

## Headless Raspberry Pi extraction

No first-party Haltech or CANformance document found in the current product
documentation defines a Raspberry Pi/Linux daemon, USB protocol, SDK, command
line logger, or unattended log-extraction API. The only documented controller
USB paths use the Windows NSP/ESP or CANTCU Configurator applications.
([Haltech software requirements](https://store.haltech.com/downloads/software/),
[CANTCU USB communication](https://wiki.canformance.net/en/CANTCU/hardware/CANTCU_pinout),
[CANTCU Configurator requirements](https://wiki.canformance.net/CANTCU/software/configurator))

A headless Pi can still receive the published CAN broadcasts without
"extracting" vendor-native logs. Linux SocketCAN exposes CAN as network
interfaces, supports receive filters, and supports controller listen-only
mode. That is a standard Linux CAN capture path, not a vendor-supported USB
integration.
([Linux SocketCAN documentation](https://docs.kernel.org/networking/can.html))

## Physically enforced receive-only design

Software listen-only prevents normal controller transmission and ACKs, but a
software or driver defect must not be able to change the safety property. Use
a transceiver with a hardware silent input and strap that input permanently
to silent on the powertrain channel. For example, NXP documents that a high
level on the TJA1057 `S` pin disables its transmitter, releases the bus pins
recessive, and leaves the receiver operating. The exact production part may
differ, but this electrical behavior and an immutable strap are requirements.
([NXP TJA1057 data sheet, section 7.1.2](https://www.nxp.com/docs/en/data-sheet/TJA1057.pdf))

The powertrain channel must:

- expose only CANH, CANL, and the required reference/isolation boundary to the
  existing bus;
- have no enabled 120-ohm termination, because it is a stub rather than a bus
  end;
- use a short twisted-pair stub and preserve the existing two-end
  termination;
- keep its transceiver's transmitter disabled independently of Pi GPIO,
  controller state, driver state, and userspace;
- configure SocketCAN with `listen-only on` as defense in depth;
- install receive filters for the approved IDs and log all unexpected IDs
  during commissioning; and
- never attach to CANTCU CAN1 or CAN2.

CANformance documents two 120-ohm end resistors (60 ohms measured with power
off), short stubs, and twisted-pair wiring as the intended topology. It also
documents that CANTCU CAN3 has no built-in termination.
([CANTCU installation manual, CAN-bus wiring](https://wiki.canformance.net/en/CANTCU/installmanual))

The custom-control channel must have its own CAN FD transceiver, harness,
connector, and two-end termination. Sharing a Pi does not make the networks
electrically or logically one bus; bridging them would. Production
configuration should fail closed if the physical interface-to-role mapping
changes, and the process that opens `can_powertrain` should have no send path
and no privilege to reconfigure the link.

Because a silent listener does not acknowledge traffic, the existing active
Haltech/CANTCU nodes must continue to provide the acknowledgements their
network already relies on. Commissioning must prove that adding and removing
the powered and unpowered Celerity tap does not change termination, error
counts, bus load, or Haltech/CANTCU operation.

## Facts to inventory from the car

Before implementation or stale-value budgets are fixed, record:

- Elite 2500 exact part, firmware, configuration backup, and whether NSP or
  ESP manages it.
- Which Elite physical CAN port is assigned to the Haltech bus, which port is
  assigned to Vehicle CAN, and which selectable terminating resistors are on.
- CANTCU hardware revision, firmware, Configurator version, transmission type,
  CAN3 car protocol, and configuration backup.
- Whether CANTCU Default CAN Datastream is enabled; its base ID, speed, and
  matching first-party DBC version.
- Whether I/O Box A and B are enabled, every virtual input assignment, and
  whether Haltech rebroadcasts any normalized TCU values.
- A powered-off resistance reading and wiring drawing showing both bus ends,
  all terminations, the Haltech/CANTCU connection, and the proposed Celerity
  stub.
- A raw CAN3 capture covering power-up, crank, idle, fan transitions, thermal
  soak, driving, shifts, shutdown, sensor disconnects, and controller
  disconnects. Record observed IDs, DLCs, periods/jitter, bus load, error
  frames, and ID collisions.
- Physical locations and calibration/type settings for coolant temperature,
  Haltech `Air Temperature`, pre-intercooler temperature and pressure,
  ambient temperature, gearbox-oil temperature, coolant pressure, and all ten
  generic sensors.
- Whether post-intercooler charge-air temperature is actually instrumented.
  If not, that is a sensor gap; do not silently substitute manifold or ambient
  temperature.
- Which wheel/vehicle-speed source is valid on this drivetrain and what it
  does during wheelspin, reverse, dyno mode, and loss of one wheel-speed
  source.
- Actual Haltech and CANTCU diagnostic states and logged values for induced
  sensor timeout and CAN-disconnect tests.
- The selected dual-channel interface's controller and transceiver part
  numbers, exposed termination, silent-mode wiring, power-off bus loading,
  galvanic-isolation behavior, and Linux driver support for listen-only mode.

## Explicitly unknown or proprietary

- CANTCU CAN1/CAN2 transmission traffic, OEM TCM diagnostic sessions, and
  internal gateway logic are proprietary surfaces for Celerity's purposes.
- The raw CANformance implementation of Haltech I/O Expander Box A/B is not
  published in the current integration instructions. Its frames must not be
  reverse-engineered into the runtime contract.
- Haltech and CANTCU USB protocols are not documented as public extraction
  APIs.
- Stable CANTCU decoded-log limits and file format are not documented.
- CANTCU 1.1 internal-logging behavior and limits are beta/future behavior.
- Protocol presence does not prove a sensor is installed, correctly located,
  calibrated, valid, or populated on this car.
- Nominal Haltech rates are not guarantees; exact freshness and jitter remain
  commissioning measurements.
