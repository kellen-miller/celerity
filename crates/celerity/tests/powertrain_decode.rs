use celerity::{
    CantcuDefaultStream, PowertrainDecodeError, PowertrainSource, decode_powertrain_frame,
};

#[test]
fn haltech_thermal_frames_are_big_endian_and_keep_raw_evidence() {
    let raw = [0x0e, 0x62, 0x0c, 0xee, 0x0c, 0x80, 0x0f, 0xa0];
    let decoded = decode_powertrain_frame(0x3e0, &raw, None).expect("Haltech temperatures");
    assert_eq!(decoded.source, PowertrainSource::HaltechBroadcastV2);
    assert_eq!(decoded.raw, raw);
    assert!((decoded.signals[0].value - 95.1).abs() < 1e-9);
    assert!((decoded.signals[1].value - 57.9).abs() < 1e-9);
    assert_eq!(decoded.signals[0].name, "coolant_temperature_c");
}

#[test]
fn cantcu_is_opt_in_little_endian_and_collision_checked() {
    let stream = CantcuDefaultStream::new(0x500).expect("collision-free base");
    let decoded = decode_powertrain_frame(
        0x500,
        &[0x34, 0x12, 0x78, 0x56, 0xbc, 0x9a, 44, 1],
        Some(stream),
    )
    .expect("CANTCU frame");
    assert_eq!(decoded.source, PowertrainSource::CantcuDefault);
    assert!((decoded.signals[0].value - 4660.0).abs() < f64::EPSILON);
    assert!((decoded.signals[1].value - 22_136.0).abs() < f64::EPSILON);
    assert_eq!(
        CantcuDefaultStream::new(0x35f),
        Err(PowertrainDecodeError::CantcuIdCollision)
    );
}

#[test]
fn unknown_frames_remain_opaque_and_recognized_dlc_is_strict() {
    let unknown = decode_powertrain_frame(0x123, &[1, 2, 3], None).expect("opaque frame");
    assert_eq!(unknown.source, PowertrainSource::Unknown);
    assert!(unknown.signals.is_empty());
    assert_eq!(unknown.raw, [1, 2, 3]);
    assert_eq!(
        decode_powertrain_frame(0x360, &[0; 7], None),
        Err(PowertrainDecodeError::WrongLength)
    );
}
