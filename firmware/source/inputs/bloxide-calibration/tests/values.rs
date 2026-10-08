use bloxide_calibration::{
    BoundedI32, BoundedU16, Celsius, EncodedValue, Milliseconds, Permille, ValueError,
    define_bounded_f32,
};

type PeriodMs = BoundedU16<Milliseconds, 100, 10_000>;
type DutyPermille = BoundedU16<Permille, 0, 1000>;
type TemperatureCentiC = BoundedI32<Celsius, -4_000, 12_500>;

define_bounded_f32!(pub ThermalGain, unit = Celsius, min = -20.0, max = 20.0);

#[test]
fn led_integer_boundaries_and_widths_are_checked() {
    assert_eq!(PeriodMs::try_new(99), Err(ValueError::OutOfBounds));
    assert_eq!(PeriodMs::try_new(100).unwrap().get(), 100);
    assert_eq!(PeriodMs::try_new(10_000).unwrap().get(), 10_000);
    assert_eq!(PeriodMs::try_new(10_001), Err(ValueError::OutOfBounds));

    for value in [0, 1, 999, 1000] {
        assert_eq!(DutyPermille::try_new(value).unwrap().get(), value);
    }
    assert_eq!(DutyPermille::try_new(1001), Err(ValueError::OutOfBounds));
    assert!(matches!(
        PeriodMs::try_decode(EncodedValue::try_from_slice(&[1]).unwrap()),
        Err(ValueError::Encoding(_))
    ));
}

#[test]
fn unrelated_integer_and_float_consumers_reject_invalid_values() {
    assert_eq!(TemperatureCentiC::try_new(-4000).unwrap().get(), -4000);
    assert_eq!(
        TemperatureCentiC::try_new(12_501),
        Err(ValueError::OutOfBounds)
    );
    assert_eq!(ThermalGain::try_new(f32::NAN), Err(ValueError::NonFinite));
    assert_eq!(
        ThermalGain::try_new(f32::INFINITY),
        Err(ValueError::NonFinite)
    );
    assert_eq!(ThermalGain::try_new(21.0), Err(ValueError::OutOfBounds));
    let encoded = EncodedValue::try_from_slice(&5.5_f32.to_le_bytes()).unwrap();
    assert_eq!(ThermalGain::try_decode(encoded).unwrap().get(), 5.5);
    let nan = EncodedValue::try_from_slice(&f32::NAN.to_le_bytes()).unwrap();
    assert_eq!(ThermalGain::try_decode(nan), Err(ValueError::NonFinite));
}
