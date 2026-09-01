# Celerity

Celerity is a vehicle control runtime that coordinates bounded active systems
without taking engine or transmission authority.

## Language

**Radiator Split Command**:
A normalized request for duct-mechanism travel, where `0.0` is the calibrated
intercooler-side endpoint and `1.0` is the calibrated radiator-side endpoint.
It does not represent measured airflow.
_Avoid_: Radiator air fraction, airflow split percentage

**Radiator-Protective Position**:
The highest validated radiator-side mechanism position within the configured
operating range.
_Avoid_: Maximum cooling position

**Controller-Local Fallback**:
The powered duct position selected by the actuator controller when remote
command authority is absent or revoked.
_Avoid_: Passive fail position

**Passive Fail Position**:
The duct position selected mechanically when actuator-controller power is
absent.
_Avoid_: Controller-local fallback

**Local Status Viewer**:
An operator-opened, read-only browser surface served from the vehicle host. It
reports observed runtime status and never holds vehicle authority.
_Avoid_: Control panel, driver controller

**Status Evidence**:
Read-only diagnostic information about the runtime. It is either Current,
Stale, Unavailable, or Unknown and never creates authority.
_Avoid_: Control state, commanded state

**Current**:
Status Evidence that was read successfully and is current for presentation.
_Avoid_: Healthy, authoritative

**Stale**:
Last-known Status Evidence whose latest read failed.
_Avoid_: Current, healthy

**Unavailable**:
Status Evidence that cannot be presented after repeated read failures.
_Avoid_: Fallback, hard fault

**Unknown**:
Status Evidence that has not yet been observed.
_Avoid_: Healthy, zero

**No Accepted Command**:
No matching controller acknowledgement is currently observed for a Radiator
Split Command. It does not describe physical duct position or airflow.
_Avoid_: Zero command, closed position
