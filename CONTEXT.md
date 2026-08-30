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
