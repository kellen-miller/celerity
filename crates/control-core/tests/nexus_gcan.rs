use std::{fs, path::PathBuf};

use control_core::{
    BundleError, PowertrainDecodeError, PowertrainSource, StartupMode, ValidatedBundle,
};

fn bundle(haltech: &str, cantcu: &str, name: &str) -> Result<ValidatedBundle, BundleError> {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/examples/simulation.toml"),
    )
    .expect("simulation fixture")
    .replace("mode = \"broadcast-v2\"", haltech)
    .replace("mode = \"disabled\"", cantcu);
    let path =
        std::env::temp_dir().join(format!("celerity-nexus-{name}-{}.toml", std::process::id()));
    fs::write(&path, source).expect("Nexus bundle");
    ValidatedBundle::load(&path, StartupMode::Simulation)
}

const NEXUS: &str = "mode = \"nexus-gcan-v1\"\nbase_id = 1536\nmaximum_period_ms = 100";

#[test]
fn commissioned_profile_decodes_independent_wire_vectors() {
    let decoder = bundle(NEXUS, "mode = \"disabled\"", "vectors")
        .expect("commissioned Nexus profile")
        .powertrain_decoder();
    let vectors = [
        (
            0x600,
            [0x03, 0xe8, 0x01, 0x90, 0, 0, 0, 0],
            vec![
                ("coolant_temperature_c", 100.0),
                ("intercooler_outlet_air_temperature_c", 40.0),
                ("air_temperature_c", 40.0),
            ],
        ),
        (
            0x601,
            [0x04, 0xe2, 0xff, 0xce, 0x08, 0x98, 0x07, 0x08],
            vec![
                ("pre_intercooler_air_temperature_c", 125.0),
                ("ambient_air_temperature_c", -5.0),
                ("pre_intercooler_boost_pressure_kpa", 220.0),
                ("map_kpa_absolute", 180.0),
            ],
        ),
        (
            0x602,
            [0x03, 0x89, 0x02, 0xbc, 0xff, 0x9c, 0x0f, 0xa0],
            vec![
                ("oil_temperature_c", 90.5),
                ("gearbox_oil_temperature_c", 70.0),
                ("coolant_pressure_kpa_gauge", -10.0),
                ("oil_pressure_kpa_gauge", 400.0),
            ],
        ),
        (
            0x603,
            [0x1b, 0x58, 0x02, 0x8f, 0x04, 0xd2, 1, 0],
            vec![
                ("engine_rpm", 7_000.0),
                ("throttle_percent", 65.5),
                ("vehicle_speed_kph", 123.4),
                ("thermo_fan_1", 1.0),
                ("thermo_fan_2", 0.0),
            ],
        ),
    ];
    for (id, raw, expected) in vectors {
        let observation = decoder.decode(id, &raw).expect("valid Nexus frame");
        assert_eq!(observation.source, PowertrainSource::NexusGcanV1);
        assert_eq!(observation.raw, raw);
        assert_eq!(observation.signals.len(), expected.len());
        for (actual, (name, value)) in observation.signals.iter().zip(expected) {
            assert_eq!(actual.name, name);
            assert!((actual.value - value).abs() < 1e-9);
        }
    }
}

#[test]
fn nexus_selection_keeps_factory_broadcast_opaque_and_cantcu_independent() {
    let decoder = bundle(
        NEXUS,
        "mode = \"default\"\nbase_id = 1280",
        "source-ownership",
    )
    .expect("nonoverlapping streams")
    .powertrain_decoder();
    for id in [0x360, 0x3e0, 0x5ff, 0x604] {
        let observation = decoder.decode(id, &[1, 2, 3]).expect("opaque evidence");
        assert_eq!(observation.source, PowertrainSource::Unknown);
        assert!(observation.signals.is_empty());
        assert_eq!(observation.raw, [1, 2, 3]);
    }

    assert_eq!(
        decoder.decode(0x500, &[0; 8]).expect("CANTCU frame").source,
        PowertrainSource::CantcuDefault,
    );
    let broadcast = bundle(
        "mode = \"broadcast-v2\"",
        "mode = \"disabled\"",
        "broadcast",
    )
    .expect("broadcast profile")
    .powertrain_decoder();
    assert_eq!(
        broadcast
            .decode(0x600, &[0; 8])
            .expect("opaque GCAN")
            .source,
        PowertrainSource::Unknown,
    );
}

#[test]
fn malformed_nexus_frames_cannot_produce_partial_signal_updates() {
    let decoder = bundle(NEXUS, "mode = \"disabled\"", "malformed")
        .expect("Nexus profile")
        .powertrain_decoder();
    for id in 0x600..=0x603 {
        for payload in [&[0; 7][..], &[0; 9][..]] {
            assert_eq!(
                decoder.decode(id, payload),
                Err(PowertrainDecodeError::WrongLength)
            );
        }
    }

    for (id, payload) in [
        (0x600, [0x80, 0, 0, 0, 0, 0, 0, 0]),
        (0x600, [0, 0, 0, 0, 0, 0, 0, 1]),
        (0x601, [0, 0, 0, 0, 0xff, 0xff, 0, 0]),
        (0x602, [0, 0, 0, 0, 0x80, 0, 0, 0]),
        (0x603, [0, 0, 0x03, 0xe9, 0, 0, 0, 0]),
        (0x603, [0, 0, 0, 0, 0, 0, 2, 0]),
        (0x603, [0, 0, 0, 0, 0, 0, 0, 2]),
    ] {
        assert_eq!(
            decoder.decode(id, &payload),
            Err(PowertrainDecodeError::InvalidNexusPayload)
        );
    }
}

#[test]
fn nexus_ids_must_fit_and_avoid_both_haltech_and_cantcu() {
    for (base, cantcu, error) in [
        (
            2045,
            "mode = \"disabled\"",
            PowertrainDecodeError::InvalidNexusBase,
        ),
        (
            65_535,
            "mode = \"disabled\"",
            PowertrainDecodeError::InvalidNexusBase,
        ),
        (
            861,
            "mode = \"disabled\"",
            PowertrainDecodeError::NexusIdCollision,
        ),
        (
            989,
            "mode = \"disabled\"",
            PowertrainDecodeError::NexusIdCollision,
        ),
        (
            1277,
            "mode = \"default\"\nbase_id = 1280",
            PowertrainDecodeError::NexusIdCollision,
        ),
        (
            1286,
            "mode = \"default\"\nbase_id = 1280",
            PowertrainDecodeError::NexusIdCollision,
        ),
    ] {
        assert_eq!(
            bundle(
                &NEXUS.replace("1536", &base.to_string()),
                cantcu,
                &format!("base-{base}")
            ),
            Err(BundleError::InvalidHaltechConfiguration(error)),
        );
    }

    let boundary = bundle(
        &NEXUS.replace("1536", "2044"),
        "mode = \"disabled\"",
        "boundary",
    )
    .expect("last four standard identifiers fit");
    assert_eq!(
        boundary
            .powertrain_decoder()
            .decode(0x7ff, &[0; 8])
            .expect("last ID")
            .source,
        PowertrainSource::NexusGcanV1,
    );
}

#[test]
fn nexus_timing_uses_the_commissioned_period() {
    for period in [250, 251, u64::MAX] {
        assert_eq!(
            bundle(
                &NEXUS.replace("100", &period.to_string()),
                "mode = \"disabled\"",
                &format!("period-{period}")
            ),
            Err(BundleError::InvalidRuntimeTiming),
        );
    }

    assert_eq!(
        bundle(
            &NEXUS.replace("100", "0"),
            "mode = \"disabled\"",
            "zero-period"
        ),
        Err(BundleError::InvalidHaltechConfiguration(
            PowertrainDecodeError::InvalidNexusPeriod
        )),
    );
    for invalid in [
        "",
        "mode = \"nexus-gcan-v1\"\nbase_id = 1536",
        "mode = \"nexus-gcan-v2\"\nbase_id = 1536\nmaximum_period_ms = 100",
        "mode = \"broadcast-v2\"\nbase_id = 1536",
        "mode = \"nexus-gcan-v1\"\nbase_id = 1536\nmaximum_period_ms = 100\nunknown = true",
    ] {
        assert!(matches!(
            bundle(
                invalid,
                "mode = \"disabled\"",
                &format!("invalid-{}", invalid.len())
            ),
            Err(BundleError::Parse(_))
        ));
    }

    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/examples/nexus-gcan-simulation.toml");
    ValidatedBundle::load(&example, StartupMode::Simulation).expect("documented Nexus example");
}

#[test]
fn faster_nexus_profile_validates_each_freshness_window() {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../config/examples/nexus-gcan-simulation.toml"),
    )
    .expect("Nexus example")
    .replace("maximum_period_ms = 100", "maximum_period_ms = 50");
    for (input, model, accepted) in [(100, 100, true), (50, 100, false), (100, 50, false)] {
        let configured = source
            .replace(
                "input_stale_after_ms = 250",
                &format!("input_stale_after_ms = {input}"),
            )
            .replace(
                "model_signals_stale_after_ms = 250",
                &format!("model_signals_stale_after_ms = {model}"),
            );
        let path = std::env::temp_dir().join(format!(
            "celerity-nexus-windows-{input}-{model}-{}.toml",
            std::process::id()
        ));
        fs::write(&path, configured).expect("timing bundle");
        let result = ValidatedBundle::load(&path, StartupMode::Simulation);
        if accepted {
            result.expect("100 ms windows exceed the commissioned 50 ms period");
        } else {
            assert_eq!(result, Err(BundleError::InvalidRuntimeTiming));
        }
    }
}
