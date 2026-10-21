use num_enum::TryFromPrimitive;

use super::{DecodedPowertrainFrame, PowertrainDecodeError, PowertrainSource, signal};

/// Relative identifiers in Celerity's commissioned Nexus GCAN v1 profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u16)]
pub(super) enum NexusFrame {
    ControlTemperatures = 0,
    AirPath = 1,
    FluidHealth = 2,
    OperatingContext = 3,
}

impl NexusFrame {
    pub(super) fn from_id(base_id: u16, can_id: u16) -> Option<Self> {
        Self::try_from(can_id.checked_sub(base_id)?).ok()
    }
}

/// Celerity's commissioned Nexus GCAN v1 profile, not a factory Haltech stream.
pub(super) fn decode(
    frame: NexusFrame,
    can_id: u16,
    payload: &[u8],
) -> Result<DecodedPowertrainFrame, PowertrainDecodeError> {
    if payload.len() != 8 {
        return Err(PowertrainDecodeError::WrongLength);
    }

    let unsigned =
        |offset: usize| f64::from(u16::from_be_bytes([payload[offset], payload[offset + 1]]));
    let signed = |offset: usize| {
        f64::from(i16::from_be_bytes([payload[offset], payload[offset + 1]])) / 10.0
    };
    let fields: &[(&'static str, f64, f64, f64)] = match frame {
        NexusFrame::ControlTemperatures => {
            if payload[4..] != [0; 4] {
                return Err(PowertrainDecodeError::InvalidNexusPayload);
            }

            &[
                ("coolant_temperature_c", signed(0), -50.0, 300.0),
                (
                    "intercooler_outlet_air_temperature_c",
                    signed(2),
                    -50.0,
                    300.0,
                ),
            ]
        }

        NexusFrame::AirPath => &[
            ("pre_intercooler_air_temperature_c", signed(0), -50.0, 300.0),
            ("ambient_air_temperature_c", signed(2), -50.0, 300.0),
            (
                "pre_intercooler_boost_pressure_kpa",
                unsigned(4) / 10.0,
                0.0,
                1_000.0,
            ),
            ("map_kpa_absolute", unsigned(6) / 10.0, 0.0, 1_000.0),
        ],
        NexusFrame::FluidHealth => &[
            ("oil_temperature_c", signed(0), -50.0, 300.0),
            ("gearbox_oil_temperature_c", signed(2), -50.0, 300.0),
            ("coolant_pressure_kpa_gauge", signed(4), -100.0, 1_000.0),
            ("oil_pressure_kpa_gauge", signed(6), -100.0, 2_000.0),
        ],
        NexusFrame::OperatingContext => {
            if payload[6] > 1 || payload[7] > 1 {
                return Err(PowertrainDecodeError::InvalidNexusPayload);
            }

            &[
                ("engine_rpm", unsigned(0), 0.0, 20_000.0),
                ("throttle_percent", unsigned(2) / 10.0, 0.0, 100.0),
                ("vehicle_speed_kph", unsigned(4) / 10.0, 0.0, 500.0),
                ("thermo_fan_1", f64::from(payload[6]), 0.0, 1.0),
            ]
        }
    };

    let mut signals = Vec::new();
    for &(name, value, minimum, maximum) in fields {
        if !(minimum..=maximum).contains(&value) {
            return Err(PowertrainDecodeError::InvalidNexusPayload);
        }

        signals.push(signal(name, value));
    }

    if frame == NexusFrame::ControlTemperatures {
        // Preserve the thermal-v1 ABI: its air input is explicitly the outlet sensor.
        signals.push(signal("air_temperature_c", signed(2)));
    } else if frame == NexusFrame::OperatingContext {
        signals.push(signal("thermo_fan_2", f64::from(payload[7])));
    }

    Ok(DecodedPowertrainFrame {
        can_id,
        source: PowertrainSource::NexusGcanV1,
        signals,
        raw: payload.to_vec(),
    })
}
