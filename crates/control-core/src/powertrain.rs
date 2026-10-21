mod haltech;
mod nexus;

use haltech::HaltechFrame;
use nexus::NexusFrame;
use num_enum::TryFromPrimitive;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum HaltechReception {
    BroadcastV2 {},
    NexusGcanV1 {
        base_id: u16,
        maximum_period_ms: u64,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(tag = "mode", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum CantcuReception {
    Disabled {},
    Default { base_id: u16 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedSignal {
    pub name: &'static str,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PowertrainSource {
    HaltechBroadcastV2,
    NexusGcanV1,
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
    InvalidNexusBase,
    NexusIdCollision,
    InvalidNexusPeriod,
    InvalidNexusPayload,
}

/// Relative message layouts in the documented CANTCU Default CAN Datastream.
#[derive(Clone, Copy, Debug, Eq, PartialEq, TryFromPrimitive)]
#[repr(u16)]
enum CantcuFrame {
    EngineAndPedal = 0,
    TorqueAndShift = 1,
    DrivenWheelAndShifter = 2,
    DigitalIo = 3,
    AnalogInputs = 4,
    ShiftStatus = 5,
    TargetRpmAndDeltas = 6,
}

impl CantcuFrame {
    fn from_id(base_id: u16, can_id: u16) -> Option<Self> {
        Self::try_from(can_id.checked_sub(base_id)?).ok()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PowertrainDecoder {
    haltech: HaltechReception,
    cantcu: Option<CantcuDefaultStream>,
}

impl PowertrainDecoder {
    pub(crate) const fn unconfigured() -> Self {
        Self {
            haltech: HaltechReception::BroadcastV2 {},
            cantcu: None,
        }
    }

    pub(crate) fn from_reception(
        haltech: HaltechReception,
        reception: CantcuReception,
    ) -> Result<Self, PowertrainDecodeError> {
        let cantcu = match reception {
            CantcuReception::Disabled {} => None,
            CantcuReception::Default { base_id } => Some(CantcuDefaultStream::new(base_id)?),
        };

        if let HaltechReception::NexusGcanV1 {
            base_id,
            maximum_period_ms,
        } = haltech
        {
            let last = base_id
                .checked_add(NexusFrame::OperatingContext as u16)
                .filter(|last| *last <= 0x7ff)
                .ok_or(PowertrainDecodeError::InvalidNexusBase)?;
            if maximum_period_ms == 0 {
                return Err(PowertrainDecodeError::InvalidNexusPeriod);
            }

            if (base_id..=last).any(|id| {
                HaltechFrame::from_id(id).is_some()
                    || cantcu
                        .is_some_and(|stream| CantcuFrame::from_id(stream.base_id, id).is_some())
            }) {
                return Err(PowertrainDecodeError::NexusIdCollision);
            }
        }

        Ok(Self { haltech, cantcu })
    }

    pub(crate) const fn maximum_period_ms(&self) -> u64 {
        match self.haltech {
            HaltechReception::BroadcastV2 {} => 200,
            HaltechReception::NexusGcanV1 {
                maximum_period_ms, ..
            } => maximum_period_ms,
        }
    }

    /// Decodes one receive-only powertrain frame using the startup configuration.
    ///
    /// # Errors
    ///
    /// Rejects recognized frames with incorrect length or invalid Nexus values.
    pub fn decode(
        &self,
        can_id: u16,
        payload: &[u8],
    ) -> Result<DecodedPowertrainFrame, PowertrainDecodeError> {
        match self.haltech {
            HaltechReception::BroadcastV2 {} => {
                if let Some(frame) = HaltechFrame::from_id(can_id) {
                    return frame.decode(can_id, payload);
                }
            }
            HaltechReception::NexusGcanV1 { base_id, .. } => {
                if let Some(frame) = NexusFrame::from_id(base_id, can_id) {
                    return nexus::decode(frame, can_id, payload);
                }
            }
        }

        decode_cantcu_frame(can_id, payload, self.cantcu)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct CantcuDefaultStream {
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
    fn new(base_id: u16) -> Result<Self, PowertrainDecodeError> {
        let last = base_id
            .checked_add(CantcuFrame::TargetRpmAndDeltas as u16)
            .ok_or(PowertrainDecodeError::InvalidCantcuBase)?;
        if last > 0x7ff {
            return Err(PowertrainDecodeError::InvalidCantcuBase);
        }
        if (base_id..=last).any(|can_id| HaltechFrame::from_id(can_id).is_some()) {
            return Err(PowertrainDecodeError::CantcuIdCollision);
        }
        Ok(Self { base_id })
    }
}

/// Decodes the explicitly enabled little-endian CANTCU Default stream.
/// Unknown frames preserve their exact bytes without invented semantics.
///
/// # Errors
///
/// Returns `WrongLength` when a recognized fixed-layout frame is not 8 bytes.
fn decode_cantcu_frame(
    can_id: u16,
    payload: &[u8],
    cantcu: Option<CantcuDefaultStream>,
) -> Result<DecodedPowertrainFrame, PowertrainDecodeError> {
    if let Some(stream) = cantcu
        && let Some(frame) = CantcuFrame::from_id(stream.base_id, can_id)
    {
        if payload.len() != 8 {
            return Err(PowertrainDecodeError::WrongLength);
        }
        let le = |offset: usize| u16::from_le_bytes([payload[offset], payload[offset + 1]]);
        let signed = |offset: usize| i16::from_le_bytes([payload[offset], payload[offset + 1]]);
        let mut signals = Vec::new();
        match frame {
            CantcuFrame::EngineAndPedal => {
                signals.push(signal("cantcu_engine_rpm", f64::from(le(0))));
                signals.push(signal("cantcu_input_rpm", f64::from(le(2))));
                signals.push(signal("cantcu_output_rpm", f64::from(le(4))));
                signals.push(signal("cantcu_pedal_percent", f64::from(payload[6])));
                signals.push(signal("cantcu_brake_switch", f64::from(payload[7])));
            }
            CantcuFrame::TorqueAndShift => {
                signals.push(signal("cantcu_engine_torque_nm", f64::from(signed(0))));
                signals.push(signal("cantcu_target_torque_nm", f64::from(signed(2))));
                signals.push(signal("cantcu_shift_in_progress", f64::from(payload[4])));
                signals.push(signal("cantcu_intervention", f64::from(payload[5])));
                signals.push(signal("cantcu_shift_cut_percent", f64::from(payload[6])));
                signals.push(signal("cantcu_blip_percent", f64::from(payload[7])));
            }
            CantcuFrame::DrivenWheelAndShifter => {
                signals.push(signal("cantcu_driven_wheel_speed", f64::from(le(0))));
                signals.push(signal("cantcu_gear", f64::from(payload[2].cast_signed())));
                signals.push(signal("cantcu_shifter_state", f64::from(payload[3])));
                signals.push(signal(
                    "cantcu_clutch_slip",
                    f64::from(payload[4].cast_signed()),
                ));
                signals.push(signal("cantcu_paddles", f64::from(payload[7])));
            }
            CantcuFrame::DigitalIo => {
                for (index, value) in payload.iter().enumerate() {
                    signals.push(signal(cantcu_digital_name(index), f64::from(*value)));
                }
            }
            CantcuFrame::AnalogInputs => {
                for index in 0..4 {
                    signals.push(signal(
                        cantcu_analog_name(index),
                        f64::from(le(index * 2)) / 1_000.0,
                    ));
                }
            }
            CantcuFrame::ShiftStatus => {
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
            CantcuFrame::TargetRpmAndDeltas => {
                signals.push(signal("cantcu_target_rpm", f64::from(le(0))));
                signals.push(signal("cantcu_delta_rpm", f64::from(signed(2))));
                signals.push(signal("cantcu_delta_torque_nm", f64::from(signed(4))));
            }
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

const fn signal(name: &'static str, value: f64) -> DecodedSignal {
    DecodedSignal { name, value }
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

#[cfg(test)]
mod tests {
    use super::{CantcuFrame, HaltechFrame};

    #[test]
    fn haltech_ids_map_to_named_frames() {
        assert_eq!(
            HaltechFrame::from_id(0x3e0),
            Some(HaltechFrame::FluidTemperatures)
        );
        assert_eq!(
            HaltechFrame::from_id(0x375),
            Some(HaltechFrame::ExhaustGasTemperatures9To12)
        );
        assert_eq!(HaltechFrame::from_id(0x3e6), None);
    }

    #[test]
    fn cantcu_ids_map_to_named_relative_frames() {
        assert_eq!(
            CantcuFrame::from_id(0x500, 0x505),
            Some(CantcuFrame::ShiftStatus)
        );
        assert_eq!(
            CantcuFrame::from_id(0x500, 0x506),
            Some(CantcuFrame::TargetRpmAndDeltas)
        );
        assert_eq!(CantcuFrame::from_id(0x500, 0x4ff), None);
        assert_eq!(CantcuFrame::from_id(0x500, 0x507), None);
    }
}
