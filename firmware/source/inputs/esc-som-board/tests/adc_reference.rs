mod support;
use am13_rs::{Peripherals, adc};
use esc_som_board::{
    peripherals::{self, StartupError},
    pins,
};
use support::Model;

#[test]
fn assembled_board_selects_internal_3v3_span() {
    let m = Model::default();
    // Inherited external full-scale mode must be replaced, not assumed.
    am13_rs::io::RegisterIo::write(&m, 0x400b_0488, 0x101);
    peripherals::initialize(unsafe { Peripherals::from_io(m.clone()) }, 38_400).unwrap();
    assert_eq!(m.value(0x400b_0488), 0);
}

#[test]
fn reference_failure_propagates_with_drivers_and_pwm_low() {
    let m = Model::default();
    m.force(0x400b_0488, 1);
    assert!(matches!(
        peripherals::initialize(unsafe { Peripherals::from_io(m.clone()) }, 38_400),
        Err(StartupError::Adc(adc::Error::NotReady))
    ));
    for pin in [pins::DRV_ENABLE, pins::INL_ENABLE] {
        assert_eq!(m.value(pin.base() + 0x1280) & (1 << pin.bit()), 0);
        assert!(
            !m.0.borrow()
                .writes
                .iter()
                .any(|&(a, v)| a == pin.base() + 0x1290 && v & (1 << pin.bit()) != 0)
        );
    }
    for module in 0..4 {
        for pair in 0..3 {
            assert_eq!(m.value(0x4001_0130 + module * 0x1000 + pair * 0x200), 0x11);
        }
    }
}
