use num_enum::TryFromPrimitive;

use super::{DecodedPowertrainFrame, PowertrainDecodeError, PowertrainSource, signal};

/// Published Haltech ECU Broadcast v2 identifiers and message layouts.
#[derive(Clone, Copy, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u16)]
pub(super) enum HaltechFrame {
    EngineLoadAndPressure = 0x360,
    FuelAndOilPressure = 0x361,
    WheelSpeeds = 0x36c,
    EngineLimiting = 0x36e,
    VehicleSpeed = 0x370,
    BatteryAndBarometricPressure = 0x372,
    ExhaustGasTemperatures1To4 = 0x373,
    ExhaustGasTemperatures5To8 = 0x374,
    ExhaustGasTemperatures9To12 = 0x375,
    AmbientAir = 0x376,
    PreIntercoolerBoostPressure = 0x377,
    FluidTemperatures = 0x3e0,
    DrivelineTemperatures = 0x3e1,
    ThermoFansAndCheckEngine = 0x3e4,
    GenericSensors1To4 = 0x3e7,
    GenericSensors5To8 = 0x3e8,
    GenericSensors9To10 = 0x3e9,
    EcuTemperature = 0x469,
    EngineProtection = 0x6f3,
    EngineState = 0x6f4,
    CalculatedAirTemperature = 0x6f7,
}

impl HaltechFrame {
    pub(super) fn from_id(can_id: u16) -> Option<Self> {
        Self::try_from(can_id).ok()
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
            Self::ExhaustGasTemperatures1To4
            | Self::ExhaustGasTemperatures5To8
            | Self::ExhaustGasTemperatures9To12 => {
                let first =
                    usize::from(self as u16 - Self::ExhaustGasTemperatures1To4 as u16) * 4 + 1;
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
            Self::GenericSensors1To4 | Self::GenericSensors5To8 | Self::GenericSensors9To10 => {
                let first = usize::from(self as u16 - Self::GenericSensors1To4 as u16) * 4 + 1;
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
