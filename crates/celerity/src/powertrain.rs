#[derive(Clone, Debug, PartialEq)]
pub struct DecodedSignal {
    pub name: &'static str,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowertrainSource {
    HaltechBroadcastV2,
    CantcuDefault,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedPowertrainFrame {
    pub can_id: u16,
    pub source: PowertrainSource,
    pub signals: Vec<DecodedSignal>,
    pub raw: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowertrainDecodeError {
    InvalidCantcuBase,
    CantcuIdCollision,
    WrongLength,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CantcuDefaultStream {
    base_id: u16,
}

impl CantcuDefaultStream {
    /// Enables the documented seven-frame CANTCU Default CAN Datastream only
    /// at a collision-free standard-ID base.
    ///
    /// # Errors
    ///
    /// Rejects bases outside the standard-ID range or overlapping a published
    /// Haltech Broadcast v2 identifier consumed by Celerity.
    pub fn new(base_id: u16) -> Result<Self, PowertrainDecodeError> {
        let last = base_id
            .checked_add(6)
            .ok_or(PowertrainDecodeError::InvalidCantcuBase)?;
        if last > 0x7ff {
            return Err(PowertrainDecodeError::InvalidCantcuBase);
        }
        if (base_id..=last).any(is_haltech_id) {
            return Err(PowertrainDecodeError::CantcuIdCollision);
        }
        Ok(Self { base_id })
    }
}

/// Decodes published Haltech Broadcast v2 big-endian fields and, only when
/// deliberately enabled, the opposite-endian CANTCU Default stream. Unknown
/// frames preserve their exact bytes without invented semantics.
///
/// # Errors
///
/// Returns `WrongLength` when a recognized fixed-layout frame is not 8 bytes.
pub fn decode_powertrain_frame(
    can_id: u16,
    payload: &[u8],
    cantcu: Option<CantcuDefaultStream>,
) -> Result<DecodedPowertrainFrame, PowertrainDecodeError> {
    if is_haltech_id(can_id) {
        if payload.len() != 8 {
            return Err(PowertrainDecodeError::WrongLength);
        }
        let be = |offset: usize| u16::from_be_bytes([payload[offset], payload[offset + 1]]);
        let mut signals = Vec::new();
        match can_id {
            0x360 => {
                signals.push(signal("engine_rpm", f64::from(be(0))));
                signals.push(signal("map_kpa_absolute", f64::from(be(2)) / 10.0));
                signals.push(signal("throttle_percent", f64::from(be(4)) / 10.0));
                signals.push(signal(
                    "coolant_pressure_kpa_gauge",
                    f64::from(be(6)) / 10.0 - 101.3,
                ));
            }
            0x361 => {
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
            0x36c => {
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
            0x36e => signals.push(signal(
                "engine_limiting_active",
                if be(0) != 0 { 1.0 } else { 0.0 },
            )),
            0x370 => signals.push(signal("vehicle_speed_kph", f64::from(be(0)) / 10.0)),
            0x372 => {
                signals.push(signal("battery_voltage_v", f64::from(be(0)) / 10.0));
                signals.push(signal("barometric_pressure_kpa", f64::from(be(6)) / 10.0));
            }
            0x373..=0x375 => {
                let first = usize::from(can_id - 0x373) * 4 + 1;
                for index in 0..4 {
                    signals.push(signal(
                        egt_name(first + index),
                        f64::from(be(index * 2)) / 10.0 - 273.1,
                    ));
                }
            }
            0x376 => signals.push(signal(
                "ambient_air_temperature_c",
                f64::from(be(0)) / 10.0 - 273.1,
            )),
            0x377 => signals.push(signal(
                "pre_intercooler_boost_pressure_kpa",
                f64::from(be(0)) / 10.0,
            )),
            0x3e0 => {
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
            0x3e1 => {
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
            0x3e4 => {
                signals.push(signal("thermo_fan_1", bool_value(payload[3] & 0x01 != 0)));
                signals.push(signal("thermo_fan_2", bool_value(payload[3] & 0x02 != 0)));
                signals.push(signal("thermo_fan_3", bool_value(payload[3] & 0x04 != 0)));
                signals.push(signal("thermo_fan_4", bool_value(payload[3] & 0x08 != 0)));
                signals.push(signal("check_engine", bool_value(payload[7] & 0x80 != 0)));
            }
            0x3e7..=0x3e9 => {
                let first = usize::from(can_id - 0x3e7) * 4 + 1;
                for index in 0..(11 - first).min(4) {
                    signals.push(signal(
                        generic_sensor_name(first + index),
                        f64::from(be(index * 2)),
                    ));
                }
            }
            0x469 => signals.push(signal("ecu_temperature_c", f64::from(be(0)) / 10.0 - 273.1)),
            0x6f3 => {
                signals.push(signal("engine_protection_severity", f64::from(payload[5])));
                signals.push(signal("engine_protection_obd_reason", f64::from(be(6))));
            }
            0x6f4 => signals.push(signal("engine_state", f64::from(payload[1] & 0x0f))),
            0x6f7 => signals.push(signal(
                "calculated_air_temperature_c",
                f64::from(be(4)) / 10.0 - 273.1,
            )),
            _ => {}
        }
        return Ok(DecodedPowertrainFrame {
            can_id,
            source: PowertrainSource::HaltechBroadcastV2,
            signals,
            raw: payload.to_vec(),
        });
    }

    if let Some(stream) = cantcu
        && (stream.base_id..=stream.base_id + 6).contains(&can_id)
    {
        if payload.len() != 8 {
            return Err(PowertrainDecodeError::WrongLength);
        }
        let le = |offset: usize| u16::from_le_bytes([payload[offset], payload[offset + 1]]);
        let signed = |offset: usize| i16::from_le_bytes([payload[offset], payload[offset + 1]]);
        let mut signals = Vec::new();
        match can_id - stream.base_id {
            0 => {
                signals.push(signal("cantcu_engine_rpm", f64::from(le(0))));
                signals.push(signal("cantcu_input_rpm", f64::from(le(2))));
                signals.push(signal("cantcu_output_rpm", f64::from(le(4))));
                signals.push(signal("cantcu_pedal_percent", f64::from(payload[6])));
                signals.push(signal("cantcu_brake_switch", f64::from(payload[7])));
            }
            1 => {
                signals.push(signal("cantcu_engine_torque_nm", f64::from(signed(0))));
                signals.push(signal("cantcu_target_torque_nm", f64::from(signed(2))));
                signals.push(signal("cantcu_shift_in_progress", f64::from(payload[4])));
                signals.push(signal("cantcu_intervention", f64::from(payload[5])));
                signals.push(signal("cantcu_shift_cut_percent", f64::from(payload[6])));
                signals.push(signal("cantcu_blip_percent", f64::from(payload[7])));
            }
            2 => {
                signals.push(signal("cantcu_driven_wheel_speed", f64::from(le(0))));
                signals.push(signal("cantcu_gear", f64::from(payload[2].cast_signed())));
                signals.push(signal("cantcu_shifter_state", f64::from(payload[3])));
                signals.push(signal(
                    "cantcu_clutch_slip",
                    f64::from(payload[4].cast_signed()),
                ));
                signals.push(signal("cantcu_paddles", f64::from(payload[7])));
            }
            3 => {
                for (index, value) in payload.iter().enumerate() {
                    signals.push(signal(cantcu_digital_name(index), f64::from(*value)));
                }
            }
            4 => {
                for index in 0..4 {
                    signals.push(signal(
                        cantcu_analog_name(index),
                        f64::from(le(index * 2)) / 1_000.0,
                    ));
                }
            }
            5 => {
                signals.push(signal("cantcu_last_shift_time_ms", f64::from(le(0))));
                signals.push(signal(
                    "cantcu_oil_temperature_c",
                    f64::from(payload[2].cast_signed()),
                ));
                signals.push(signal("cantcu_drive_mode", f64::from(payload[3])));
                signals.push(signal("cantcu_launch_state", f64::from(payload[5])));
                signals.push(signal(
                    "cantcu_supply_voltage_v",
                    f64::from(le(6)) / 1_000.0,
                ));
            }
            6 => {
                signals.push(signal("cantcu_target_rpm", f64::from(le(0))));
                signals.push(signal("cantcu_delta_rpm", f64::from(signed(2))));
                signals.push(signal("cantcu_delta_torque_nm", f64::from(signed(4))));
            }
            _ => unreachable!("validated seven-frame range"),
        }
        return Ok(DecodedPowertrainFrame {
            can_id,
            source: PowertrainSource::CantcuDefault,
            signals,
            raw: payload.to_vec(),
        });
    }

    Ok(DecodedPowertrainFrame {
        can_id,
        source: PowertrainSource::Unknown,
        signals: Vec::new(),
        raw: payload.to_vec(),
    })
}

const fn is_haltech_id(can_id: u16) -> bool {
    matches!(
        can_id,
        0x360
            | 0x361
            | 0x36c
            | 0x36e
            | 0x370
            | 0x372..=0x377
            | 0x3e0
            | 0x3e1
            | 0x3e4
            | 0x3e7..=0x3e9
            | 0x469
            | 0x6f3
            | 0x6f4
            | 0x6f7
    )
}

const fn signal(name: &'static str, value: f64) -> DecodedSignal {
    DecodedSignal { name, value }
}

const fn bool_value(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
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

const fn cantcu_digital_name(index: usize) -> &'static str {
    [
        "cantcu_digital_input_1",
        "cantcu_digital_input_2",
        "cantcu_digital_input_3",
        "cantcu_digital_input_4",
        "cantcu_digital_output_1",
        "cantcu_digital_output_2",
        "cantcu_digital_output_3",
        "cantcu_digital_output_4",
    ][index]
}

const fn cantcu_analog_name(index: usize) -> &'static str {
    [
        "cantcu_analog_input_1_v",
        "cantcu_analog_input_2_v",
        "cantcu_analog_input_3_v",
        "cantcu_analog_input_4_v",
    ][index]
}
