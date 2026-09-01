# First physical deployment checklist

Every item below is **unexecuted**. Hardware-free CI is not evidence for it.

## Bench and HIL

- [ ] Confirm target flash/stack margin and worst-case executor timing on ARM.
- [ ] Measure FDCAN receive/transmit, error, bus-off, and recovery behavior.
- [ ] Measure PWM cadence/jitter and watchdog/local-fallback latency under load.
- [ ] Reset and power-cycle the controller repeatedly; verify each new boot session remains in local fallback until discovery and volatile configuration reconciliation complete.
- [ ] Measure model inference ceiling and required cycle margin on the target.

## Electrical and fail-silent boundary

- [ ] Validate protected power, grounding, EMC, transceivers, harness, and termination.
- [ ] Prove the powertrain tap is physically receive-only and independently fail-silent.
- [ ] Verify zero on-wire drive from the powertrain interface across process faults.
- [ ] Remove runtime/controller power and prove passive radiator-biased return.

## Mechanism and thermal system

- [ ] Calibrate end stops, split mapping, settling time, backlash, and obstruction behavior.
- [ ] Run bounded coolant/IAT experiments with explicit abort conditions.
- [ ] Establish the safe thermal envelope before enabling model-assisted commands.

## Display

- [ ] Check viewer legibility at seated distance in direct sun and at night.
- [ ] Check glare and reflections at the installed display angle.
- [ ] Confirm every viewer state fits the actual display without scrolling.
- [ ] Measure celerityd cycle timing with the viewer closed and open; prove the
      viewer does not perturb control-loop timing.

## Installed vehicle

- [ ] Validate live Haltech/CANTCU decoding on this car against independent instruments.
- [ ] Complete installation inspection and fault-injection sign-off.
- [ ] Demonstrate model accuracy/OOD bounds using held-out vehicle Runs.
- [ ] Complete staged road and track acceptance with named approvers.
