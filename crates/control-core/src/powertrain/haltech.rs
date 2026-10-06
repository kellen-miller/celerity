use super::{DecodedPowertrainFrame, PowertrainDecodeError, PowertrainSource, signal};

/// Published Haltech ECU Broadcast v2 message layouts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum HaltechFrame {
    EngineLoadAndPressure,
    FuelAndOilPressure,
    WheelSpeeds,
    EngineLimiting,
    VehicleSpeed,
    BatteryAndBarometricPressure,
    ExhaustGasTemperatureGroup { group: u8 },
    AmbientAir,
    PreIntercoolerBoostPressure,
    FluidTemperatures,
    DrivelineTemperatures,
    ThermoFansAndCheckEngine,
    GenericSensorGroup { group: u8 },
    EcuTemperature,
    EngineProtection,
    EngineState,
    CalculatedAirTemperature,
}

impl HaltechFrame {
    pub(super) const fn from_id(can_id: u16) -> Option<Self> {
        Some(match can_id {
            0x360 => Self::EngineLoadAndPressure,
            0x361 => Self::FuelAndOilPressure,
            0x36c => Self::WheelSpeeds,
            0x36e => Self::EngineLimiting,
            0x370 => Self::VehicleSpeed,
            0x372 => Self::BatteryAndBarometricPressure,
            0x373..=0x375 => Self::ExhaustGasTemperatureGroup {
                group: (can_id - 0x373) as u8,
            },
            0x376 => Self::AmbientAir,
            0x377 => Self::PreIntercoolerBoostPressure,
            0x3e0 => Self::FluidTemperatures,
            0x3e1 => Self::DrivelineTemperatures,
            0x3e4 => Self::ThermoFansAndCheckEngine,
            0x3e7..=0x3e9 => Self::GenericSensorGroup {
                group: (can_id - 0x3e7) as u8,
            },
            0x469 => Self::EcuTemperature,
            0x6f3 => Self::EngineProtection,
            0x6f4 => Self::EngineState,
            0x6f7 => Self::CalculatedAirTemperature,
            _ => return None,
        })
    }

    pub(super) fn decode(
        self,
        can_id: u16,
        payload: &[u8],
    ) -> Result<DecodedPowertrainFrame, PowertrainDecodeError> {
        if payload.len() != 8 {
            return Err(PowertrainDecodeError::WrongLength);
        }
        let be = |offset: usize| u16::from_be_bytes([payload[offset], payload[offset + 1]]);
        let mut signals = Vec::new();
        match self {
            Self::EngineLoadAndPressure => {
                signals.push(signal("engine_rpm", f64::from(be(0))));
                signals.push(signal("map_kpa_absolute", f64::from(be(2)) / 10.0));
                signals.push(signal("throttle_percent", f64::from(be(4)) / 10.0));
                signals.push(signal(
                    "coolant_pressure_kpa_gauge",
                    f64::from(be(6)) / 10.0 - 101.3,
                ));
            }
            Self::FuelAndOilPressure => {
                signals.push(signal(
                    "fuel_pressure_kpa_gauge",
                    f64::from(be(0)) / 10.0 - 101.3,
                ));
                signals.push(signal(
                    "oil_pressure_kpa_gauge",
                    f64::from(be(2)) / 10.0 - 101.3,
                ));
                signals.push(signal("engine_demand_percent", f64::from(be(4)) / 10.0));
            }
            Self::WheelSpeeds => {
                for (index, name) in [
                    "wheel_speed_front_left_kph",
                    "wheel_speed_front_right_kph",
                    "wheel_speed_rear_left_kph",
                    "wheel_speed_rear_right_kph",
                ]
                .into_iter()
                .enumerate()
                {
                    signals.push(signal(name, f64::from(be(index * 2)) / 10.0));
                }
            }
            Self::EngineLimiting => signals.push(signal(
                "engine_limiting_active",
                if be(0) != 0 { 1.0 } else { 0.0 },
            )),
            Self::VehicleSpeed => {
                signals.push(signal("vehicle_speed_kph", f64::from(be(0)) / 10.0));
            }
            Self::BatteryAndBarometricPressure => {
                signals.push(signal("battery_voltage_v", f64::from(be(0)) / 10.0));
                signals.push(signal("barometric_pressure_kpa", f64::from(be(6)) / 10.0));
            }
            Self::ExhaustGasTemperatureGroup { group } => {
                let first = usize::from(group) * 4 + 1;
                for index in 0..4 {
                    signals.push(signal(
                        egt_name(first + index),
                        f64::from(be(index * 2)) / 10.0 - 273.1,
                    ));
                }
            }
            Self::AmbientAir => signals.push(signal(
                "ambient_air_temperature_c",
                f64::from(be(0)) / 10.0 - 273.1,
            )),
            Self::PreIntercoolerBoostPressure => signals.push(signal(
                "pre_intercooler_boost_pressure_kpa",
                f64::from(be(0)) / 10.0,
            )),
            Self::FluidTemperatures => {
                for (offset, name) in [
                    "coolant_temperature_c",
                    "air_temperature_c",
                    "fuel_temperature_c",
                    "oil_temperature_c",
                ]
                .into_iter()
                .enumerate()
                {
                    signals.push(signal(name, f64::from(be(offset * 2)) / 10.0 - 273.1));
                }
            }
            Self::DrivelineTemperatures => {
                signals.push(signal(
                    "gearbox_oil_temperature_c",
                    f64::from(be(0)) / 10.0 - 273.1,
                ));
                signals.push(signal(
                    "differential_oil_temperature_c",
                    f64::from(be(2)) / 10.0 - 273.1,
                ));
                signals.push(signal(
                    "pre_intercooler_air_temperature_c",
                    f64::from(be(6)) / 10.0 - 273.1,
                ));
            }
            Self::ThermoFansAndCheckEngine => {
                signals.push(signal("thermo_fan_1", bool_value(payload[3] & 0x01 != 0)));
                signals.push(signal("thermo_fan_2", bool_value(payload[3] & 0x02 != 0)));
                signals.push(signal("thermo_fan_3", bool_value(payload[3] & 0x04 != 0)));
                signals.push(signal("thermo_fan_4", bool_value(payload[3] & 0x08 != 0)));
                signals.push(signal("check_engine", bool_value(payload[7] & 0x80 != 0)));
            }
            Self::GenericSensorGroup { group } => {
                let first = usize::from(group) * 4 + 1;
                for index in 0..(11 - first).min(4) {
                    signals.push(signal(
                        generic_sensor_name(first + index),
                        f64::from(be(index * 2)),
                    ));
                }
            }
            Self::EcuTemperature => {
                signals.push(signal("ecu_temperature_c", f64::from(be(0)) / 10.0 - 273.1));
            }
            Self::EngineProtection => {
                signals.push(signal("engine_protection_severity", f64::from(payload[5])));
                signals.push(signal("engine_protection_obd_reason", f64::from(be(6))));
            }
            Self::EngineState => {
                signals.push(signal("engine_state", f64::from(payload[1] & 0x0f)));
            }
            Self::CalculatedAirTemperature => signals.push(signal(
                "calculated_air_temperature_c",
                f64::from(be(4)) / 10.0 - 273.1,
            )),
        }
        Ok(DecodedPowertrainFrame {
            can_id,
            source: PowertrainSource::HaltechBroadcastV2,
            signals,
            raw: payload.to_vec(),
        })
    }
}

const fn egt_name(index: usize) -> &'static str {
    [
        "egt_1_c", "egt_2_c", "egt_3_c", "egt_4_c", "egt_5_c", "egt_6_c", "egt_7_c", "egt_8_c",
        "egt_9_c", "egt_10_c", "egt_11_c", "egt_12_c",
    ][index - 1]
}

const fn generic_sensor_name(index: usize) -> &'static str {
    [
        "generic_sensor_1_raw",
        "generic_sensor_2_raw",
        "generic_sensor_3_raw",
        "generic_sensor_4_raw",
        "generic_sensor_5_raw",
        "generic_sensor_6_raw",
        "generic_sensor_7_raw",
        "generic_sensor_8_raw",
        "generic_sensor_9_raw",
        "generic_sensor_10_raw",
    ][index - 1]
}

const fn bool_value(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}
