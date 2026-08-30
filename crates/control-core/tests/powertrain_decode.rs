use std::{fs, path::PathBuf};

use control_core::{PowertrainDecoder, PowertrainSource, StartupMode, ValidatedBundle};

fn decoder(cantcu: &str, name: &str) -> PowertrainDecoder {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/examples/simulation.toml"),
    )
    .expect("simulation fixture")
    .replace("mode = \"disabled\"", cantcu);
    let path = std::env::temp_dir().join(format!(
        "celerity-powertrain-{name}-{}.toml",
        std::process::id()
    ));
    fs::write(&path, source).expect("powertrain bundle");
    ValidatedBundle::load(&path, StartupMode::Simulation)
        .expect("valid powertrain bundle")
        .powertrain_decoder()
}

#[test]
fn haltech_thermal_frames_are_big_endian_and_keep_raw_evidence() {
    let raw = [0x0e, 0x62, 0x0c, 0xee, 0x0c, 0x80, 0x0f, 0xa0];
    let decoded = decoder("mode = \"disabled\"", "haltech")
        .decode(0x3e0, &raw)
        .expect("Haltech temperatures");
    assert_eq!(decoded.source, PowertrainSource::HaltechBroadcastV2);
    assert_eq!(decoded.raw, raw);
    assert!((decoded.signals[0].value - 95.1).abs() < 1e-9);
    assert!((decoded.signals[1].value - 57.9).abs() < 1e-9);
    assert_eq!(decoded.signals[0].name, "coolant_temperature_c");
}

#[test]
fn cantcu_is_opt_in_and_little_endian() {
    let decoded = decoder("mode = \"default\"\nbase_id = 1280", "cantcu")
        .decode(0x500, &[0x34, 0x12, 0x78, 0x56, 0xbc, 0x9a, 44, 1])
        .expect("CANTCU frame");
    assert_eq!(decoded.source, PowertrainSource::CantcuDefault);
    assert!((decoded.signals[0].value - 4660.0).abs() < f64::EPSILON);
    assert!((decoded.signals[1].value - 22_136.0).abs() < f64::EPSILON);
}

#[test]
fn unknown_frames_remain_opaque_and_recognized_dlc_is_strict() {
    let decoder = decoder("mode = \"disabled\"", "unknown");
    let unknown = decoder.decode(0x123, &[1, 2, 3]).expect("opaque frame");
    assert_eq!(unknown.source, PowertrainSource::Unknown);
    assert!(unknown.signals.is_empty());
    assert_eq!(unknown.raw, [1, 2, 3]);
    assert_eq!(
        decoder.decode(0x360, &[0; 7]),
        Err(control_core::PowertrainDecodeError::WrongLength)
    );
}
