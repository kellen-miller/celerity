> **Current decision:** STM32G431KB plus native FDCAN and TCAN1044AV-Q1 remains authoritative. STM32G474 is considered only after measured G431 resource evidence fails.

# Actuator controller hardware

Research decision for [issue #15](https://github.com/kellen-miller/celerity/issues/15).

## Recommendation

Use the [NUCLEO-G431KB](https://www.st.com/en/evaluation-tools/nucleo-g431kb.html) for bench development and hardware-in-the-loop testing. Use the same [STM32G431KB](https://www.st.com/en/microcontrollers-microprocessors/stm32g431kb.html) MCU on a purpose-built carrier for the installed controller. The Nucleo board is a development tool, not vehicle-installation hardware.

Connect the MCU's native FDCAN peripheral to a [TCAN1044AV-Q1](https://www.ti.com/product/TCAN1044A-Q1) automotive transceiver. Do not add an MCP251863 or another SPI CAN controller. The `V` transceiver variant provides a 3.3 V VIO interface; its standby input defaults high, and it adds a dominant-state timeout plus high-impedance bus pins when unpowered. It supports CAN FD data rates through 8 Mbps. See the [TCAN1044A-Q1 datasheet](https://www.ti.com/lit/ds/symlink/tcan1044a-q1.pdf).

The transceiver's weak internal standby pull-up is a fail-safe, not the carrier's
design control. Add an external pull-up so reset, unpowered, and undriven MCU
states hold the transceiver in standby; firmware may pull it low only after local
initialization and health checks succeed.

The installed carrier must preserve [issue #3's](https://github.com/kellen-miller/celerity/issues/3) protected power, harness, CAN TVS, and termination contract. Selecting the MCU and transceiver does not relax that electrical boundary.

The carrier also owns a CAN-suitable external clock, the protected 5 V and
3.3 V rails, and the actuator-output electronics. Vehicle battery and actuator
current do not enter through the Nucleo board or the four-wire CAN control
connector; actuator power, fusing, return, and output-stage protection remain a
separate physical circuit.

## Why STM32G431KB

The [STM32G431KB datasheet](https://www.st.com/resource/en/datasheet/stm32g431kb.pdf) provides the required controller resources in a compact part:

- 128 KiB flash and 32 KiB SRAM, treated as a candidate budget until the real
  firmware proves acceptable margin;
- native FDCAN, avoiding SPI latency, another clock domain, and another failure surface;
- capable timers and PWM outputs for deterministic actuator drive;
- independent and window watchdogs;
- 1 KiB OTP and a 96-bit unique device identifier for manufacturing identity and immutable calibration metadata.

The NUCLEO-G431KB exposes that MCU in a Nano-format board with integrated ST-LINK, making one firmware and peripheral model usable from initial bench work through custom-carrier validation. Production suitability still comes from the carrier's automotive power, protection, connector, thermal, and mechanical design—not from the Nucleo form factor.

## Rust firmware fit

Rust supports `thumbv7em-none-eabihf` as a stable [Tier 2 bare-metal target](https://doc.rust-lang.org/rustc/platform-support/thumbv7em-none-eabi.html). Prefer [`embassy-stm32`](https://github.com/embassy-rs/embassy/tree/main/embassy-stm32) because its current STM32G431 support covers FDCAN, PWM/timers, watchdogs, and internal flash. Its [direct and buffered CAN implementations](https://github.com/embassy-rs/embassy/tree/main/embassy-stm32/src/can) support an FD CAN design without introducing a separate controller abstraction.

Keep the firmware lifecycle cohesive: initialize protected outputs, configuration, FDCAN, lease state, PWM, and watchdogs in an obvious order; process commands and CAN errors in the main control flow; and use a timer-driven lease expiry that forces the defined safe actuator state independently of message arrival. Network I/O, flash writes, watchdog servicing, resets, and safe-state transitions should remain visible rather than being hidden behind layers of single-use helpers.

[`stm32g4xx-hal`](https://github.com/stm32-rs/stm32g4xx-hal) with [`fdcan`](https://github.com/stm32-rs/fdcan) remains a viable fallback, but it is less active and its repositories retain [open CAN correctness concerns](https://github.com/stm32-rs/stm32g4xx-hal/issues?q=is%3Aissue%20state%3Aopen%20CAN). That makes it a weaker default for safety-relevant control firmware.

## Alternatives considered

| Option | Decision |
| --- | --- |
| [NUCLEO-G474RE / STM32G474RE](https://www.st.com/en/evaluation-tools/nucleo-g474re.html) | Fallback only. Its [512 KiB flash, 128 KiB SRAM, and dual-bank flash](https://www.st.com/en/microcontrollers-microprocessors/stm32g474re.html) provide more headroom, but the board and package are larger. Move to it only if linked-image, stack, and runtime measurements show the G431 lacks acceptable margin. |
| [RP2040](https://www.raspberrypi.com/products/rp2040/) or [RP2350](https://www.raspberrypi.com/products/rp2350/) plus [MCP251863](https://www.microchip.com/en-us/product/mcp251863) | Reject. Neither MCU supplies native CAN FD, so the design adds an SPI bus, an external CAN controller, additional interrupt/error handling, and more hardware and firmware integration risk. |
| [Teensy 4.1](https://www.pjrc.com/store/teensy41.html) / [i.MX RT](https://www.nxp.com/products/processors-and-microcontrollers/arm-microcontrollers/i-mx-rt-crossover-mcus:IMX-RT-SERIES) | Reject for the default. The compute platform is capable, but the maintained Rust CAN FD path is weaker than STM32G4 plus Embassy and the board brings unnecessary size and performance. |
| Classic [Arduino Nano](https://docs.arduino.cc/hardware/nano/) / AVR | Reject. The Rust target and library ecosystem, memory, and peripheral resources do not fit a native CAN FD actuator controller. |

## Bench acceptance gates

The choice is accepted for an installed carrier only after the bench and HIL setup passes all of the following:

- compile, link, map-file, worst-case stack, and runtime memory measurements with explicit flash and SRAM margin;
- sustained ISO CAN FD at 500 kbit/s arbitration and 2 Mbit/s data phase with BRS, including peak traffic, injected errors, error-passive transitions, and bus-off recovery;
- lease expiry during silence, malformed or stale commands, task stalls, and resets, with every path reaching the defined safe actuator state;
- independent watchdog and window-watchdog fault tests, including stalled control and communication paths;
- PWM frequency, duty, jitter, startup, shutdown, and safe-state timing under maximum interrupt and CAN load;
- configuration-flash writes interrupted at each power-loss point, proving recovery to a valid prior/default configuration;
- TCAN1044AV-Q1 standby and default-state behavior, stuck-dominant protection, and unpowered bus-loading tests;
- protected-power transient, brownout, reverse-polarity, EMC, operating-temperature, harness fault, TVS, and termination tests required by issue #3.

Failure of a gate should improve the carrier, firmware, or test evidence first. The G474 becomes the hardware fallback only when measurements demonstrate that the G431's resource margin itself is the limiting factor.
