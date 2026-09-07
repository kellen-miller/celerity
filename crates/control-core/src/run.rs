use serde::{Deserialize, Serialize};

use crate::RuntimeEvent;

#[allow(
    clippy::doc_markdown,
    clippy::missing_const_for_fn,
    clippy::trivially_copy_pass_by_ref
)]
mod wire {
    include!(concat!(env!("OUT_DIR"), "/celerity.run.v1.rs"));
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Completion {
    Complete,
    Incomplete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnqueueResult {
    Accepted,
    Dropped,
    Degraded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RunChunk {
    pub file: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawCanEvidence {
    pub monotonic_ns: u64,
    pub source: &'static str,
    pub channel: &'static str,
    pub direction: i32,
    pub id: u32,
    pub fd: bool,
    pub bit_rate_switch: bool,
    pub data: Vec<u8>,
    pub hardware_timestamp_ns: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunRecord {
    sequence: u64,
    monotonic_ns: u64,
    source: String,
    payload: wire::run_event::Payload,
}

impl RunRecord {
    #[must_use]
    pub fn lifecycle(sequence: u64, monotonic_ns: u64, payload: &str) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: "runtime".to_owned(),
            payload: wire::run_event::Payload::LifecycleEvent(wire::LifecycleEvent {
                encoded: payload.to_owned(),
            }),
        }
    }

    #[must_use]
    pub fn evidence(sequence: u64, monotonic_ns: u64, source: &str, payload: &str) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: source.to_owned(),
            payload: wire::run_event::Payload::Annotation(wire::Annotation {
                encoded: payload.to_owned(),
            }),
        }
    }

    /// Encodes one ordered production runtime event into the canonical Run
    /// stream without changing its semantic fields.
    ///
    /// # Errors
    ///
    /// Returns a serialization error if the versioned event cannot be encoded.
    pub fn runtime_event(sequence: u64, event: &RuntimeEvent) -> Result<Self, serde_json::Error> {
        Ok(Self {
            sequence,
            monotonic_ns: event.monotonic_ms_for_evidence().saturating_mul(1_000_000),
            source: "runtime_event".to_owned(),
            payload: wire::run_event::Payload::LifecycleEvent(wire::LifecycleEvent {
                encoded: serde_json::to_string(event)?,
            }),
        })
    }

    #[must_use]
    pub fn raw_can(sequence: u64, evidence: RawCanEvidence) -> Self {
        Self {
            sequence,
            monotonic_ns: evidence.monotonic_ns,
            source: evidence.source.to_owned(),
            payload: wire::run_event::Payload::RawCan(wire::RawCanFrame {
                channel: evidence.channel.to_owned(),
                direction: evidence.direction,
                id: evidence.id,
                extended: evidence.id > 0x7ff,
                fd: evidence.fd,
                bit_rate_switch: evidence.bit_rate_switch,
                error_state_indicator: false,
                data: evidence.data,
                hardware_timestamp_ns: evidence.hardware_timestamp_ns,
                receive_overflow_count: 0,
                dropped_count: 0,
            }),
        }
    }

    #[must_use]
    pub fn signal(
        sequence: u64,
        monotonic_ns: u64,
        signal: &str,
        value: f64,
        decoder_generation: u64,
        age_ns: u64,
        source_event_sequence: u64,
    ) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: "model_signal".to_owned(),
            payload: wire::run_event::Payload::SignalObservation(wire::SignalObservation {
                signal: signal.to_owned(),
                value,
                unit: "native".to_owned(),
                reference: "vehicle".to_owned(),
                quality: wire::Quality::Valid as i32,
                reason: String::new(),
                source_event_sequence,
                decoder_generation,
                age_ns,
            }),
        }
    }

    #[must_use]
    pub fn control(sequence: u64, monotonic_ns: u64, payload: &str) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: "control".to_owned(),
            payload: wire::run_event::Payload::ControlDecision(wire::ControlDecision {
                encoded: payload.to_owned(),
            }),
        }
    }

    #[must_use]
    pub fn experiment(sequence: u64, monotonic_ns: u64, payload: &str) -> Self {
        Self {
            sequence,
            monotonic_ns,
            source: "experiment".to_owned(),
            payload: wire::run_event::Payload::ExperimentEvent(wire::ExperimentEvent {
                encoded: payload.to_owned(),
            }),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RunManifest {
    pub schema_version: u32,
    pub run_id: String,
    pub completion: Completion,
    pub incomplete_reason: Option<String>,
    pub first_sequence: Option<u64>,
    pub last_sequence: Option<u64>,
    pub chunk_file: String,
    pub chunk_sha256: String,
    #[serde(default)]
    pub chunks: Vec<RunChunk>,
    #[serde(default)]
    pub dropped_record_count: u64,
    pub configuration_generation: u64,
    pub configuration_sha256: String,
    pub model_bundle_digest: Option<String>,
    pub protocol_major: u8,
    pub firmware_generation: u32,
    pub decoder_generation: u64,
    pub model_abi: String,
    pub model_input_signals: Vec<String>,
    pub model_history_length: usize,
    pub sample_period_ms: u64,
    pub command_lattice: Vec<u16>,
    pub maximum_calibration_error: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunContext {
    pub configuration_generation: u64,
    pub configuration_sha256: String,
    pub model_bundle_digest: Option<String>,
    pub protocol_major: u8,
    pub firmware_generation: u32,
    pub decoder_generation: u64,
    pub model_abi: String,
    pub model_input_signals: Vec<String>,
    pub model_history_length: usize,
    pub sample_period_ms: u64,
    pub command_lattice: Vec<u16>,
    pub maximum_calibration_error: f64,
}

#[derive(Debug)]
pub struct RunError(String);

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for RunError {}

mod recovery;
mod storage;
mod writer;

pub use recovery::{recover_incomplete_runs, replay_events};
pub use writer::RunWriter;

#[cfg(test)]
mod tests {
    use super::{RawCanEvidence, RunRecord, wire};

    #[test]
    fn typed_records_preserve_exact_run_v1_payloads() {
        let raw = RunRecord::raw_can(
            1,
            RawCanEvidence {
                monotonic_ns: 10,
                source: "actuator_rx",
                channel: "actuator",
                direction: wire::Direction::Receive as i32,
                id: 0x18ff_0101,
                fd: true,
                bit_rate_switch: true,
                data: vec![1, 2, 3, 4],
                hardware_timestamp_ns: Some(99),
            },
        );
        let wire::run_event::Payload::RawCan(raw) = raw.payload else {
            panic!("raw CAN oneof");
        };
        assert_eq!(raw.id, 0x18ff_0101);
        assert!(raw.extended && raw.fd && raw.bit_rate_switch);
        assert_eq!(raw.data, [1, 2, 3, 4]);
        assert_eq!(raw.hardware_timestamp_ns, Some(99));
        assert_eq!((raw.receive_overflow_count, raw.dropped_count), (0, 0));

        let signal = RunRecord::signal(2, 20, "coolant_temperature_c", 91.5, 7, 3, 1);
        let wire::run_event::Payload::SignalObservation(signal) = signal.payload else {
            panic!("signal oneof");
        };
        assert_eq!(signal.source_event_sequence, 1);
        assert_eq!(signal.decoder_generation, 7);
        assert_eq!(signal.age_ns, 3);

        assert!(matches!(
            RunRecord::control(3, 30, "{}").payload,
            wire::run_event::Payload::ControlDecision(_)
        ));
        assert!(matches!(
            RunRecord::experiment(4, 40, "{}").payload,
            wire::run_event::Payload::ExperimentEvent(_)
        ));
    }
}
