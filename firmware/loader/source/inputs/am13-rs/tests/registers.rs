mod support;
use am13_rs::{
    gpio::{Error, Pin, Port},
    uart::{self, Divisor},
    Peripherals,
};
use support::Model;
fn device(m: &Model) -> Peripherals<Model> {
    unsafe { Peripherals::from_io(m.clone()) }
}
#[test]
fn flash_snapshot_is_read_only_and_rwait_zero_denies_basic_readiness() {
    let m = Model::default();
    m.force(0x6011_1074, 512 | (2 << 12));
    m.force(0x4002_9000, 1 << 8);
    m.force(0x4004_3210, 3);
    let observation = device(&m).flash.observe();
    assert!(observation.basic_readiness());
    assert!(m.0.borrow().writes.is_empty());
    assert_eq!(m.0.borrow().reads.len(), 7);
    assert!(m.0.borrow().reads.contains(&0x4002_900c));
    m.force(0x4002_9000, 0);
    assert!(!device(&m).flash.observe().basic_readiness());
    m.force(0x4002_9000, 1 << 8);
    m.force(0x4004_33d0, 4);
    assert!(!device(&m).flash.observe().basic_readiness());
}
#[test]
fn sysosc_domain_and_baud_rounding_regression() {
    let m = Model::default();
    let d = device(&m);
    let c = d.clocks.sysosc_32mhz().unwrap();
    assert_eq!((c.mclk_hz(), c.uc4_hz()), (32_000_000, 32_000_000));
    for (baud, i, f) in [(19200, 104, 11), (38400, 52, 5), (115200, 17, 23)] {
        let div = Divisor::calculate(c.uc4_hz(), baud).unwrap();
        assert_eq!((div.integer(), div.fractional()), (i, f));
    }
    for (clock, baud) in [
        (0, 19200),
        (32_000_000, 0),
        (32_000_000, 1),
        (32_000_000, u32::MAX),
    ] {
        assert!(Divisor::calculate(clock, baud).is_none());
    }
    let _uart = d.uc4.configure(c, 19200).unwrap().enable().unwrap();
    assert_eq!((m.value(0x40641008), m.value(0x40641000)), (8, 0));
    assert_eq!((m.value(0x40641110), m.value(0x40641114)), (104, 11));
    assert_eq!(m.value(0x40641104), 0x30);
}
#[test]
fn stuck_clock_is_bounded_failure() {
    let m = Model::default();
    m.force(0x400b0204, 0x10);
    assert!(device(&m).clocks.sysosc_32mhz().is_err());
    assert!(m.0.borrow().reads.len() <= 100_010);
}
#[test]
fn uart_errors_precede_data_and_full_tx_never_writes() {
    let m = Model::default();
    let d = device(&m);
    let c = d.clocks.sysosc_32mhz().unwrap();
    let mut u = d.uc4.configure(c, 38400).unwrap().enable().unwrap();
    m.force(0x40641030, 2);
    assert_eq!(u.try_read(), Err(uart::Error::Receive));
    assert!(!m.0.borrow().reads.contains(&0x40641124));
    m.0.borrow_mut().forced.remove(&0x40641030);
    m.force(0x40641108, 4 | 64 | 1);
    assert_eq!(u.try_read(), Ok(None));
    assert!(!u.try_write(0xaa));
    assert!(u.is_tx_complete()); // No byte was admitted.
    assert!(!m.0.borrow().writes.iter().any(|(a, _)| *a == 0x40641120));
    m.force(0x40641108, 0);
    m.force(0x40641124, 0xa5);
    assert_eq!(u.try_read(), Ok(Some(0xa5)));
    assert!(u.try_write(0x5a));
    assert!(!u.is_tx_complete());
    m.force(0x40641030, 1 << 12);
    assert!(u.is_tx_complete());
    assert_eq!(m.value(0x40641120), 0x5a);
    m.force(0x40641124, 0x100);
    assert_eq!(u.try_read(), Err(uart::Error::Receive));
    assert_eq!(u.rx_errors(), 2);
}
#[test]
fn bad_uart_readback_disables_uart() {
    let m = Model::default();
    let d = device(&m);
    let c = d.clocks.sysosc_32mhz().unwrap();
    m.force(0x40641114, 99);
    assert!(matches!(
        d.uc4.configure(c, 38400).unwrap().enable(),
        Err(uart::Error::Readback)
    ));
    assert_eq!(m.value(0x40641100), 0x18);
}
#[test]
fn gpio_single_pin_claim_survives_drop_and_power_cannot_reset_outputs() {
    let m = Model::default();
    let mut g = device(&m).gpio;
    g.power().unwrap();
    let pin = Pin::new(Port::B, 18);
    let mut output = g.output(pin, true).unwrap();
    assert_eq!(g.power(), Err(Error::AlreadyOwned));
    assert!(matches!(g.output(pin, false), Err(Error::AlreadyOwned)));
    output.set_level(false).unwrap();
    m.force(pin.base() + 0x1380, 1 << 18);
    assert_eq!(output.verify(false), Err(Error::Readback));
    drop(output);
    assert!(matches!(g.output(pin, true), Err(Error::AlreadyOwned)));
}
#[test]
fn systick_100us_exact_register_sequence() {
    let m = Model::default();
    let d = device(&m);
    let c = d.clocks.sysosc_32mhz().unwrap();
    m.0.borrow_mut().writes.clear();
    d.systick.start(c);
    assert_eq!(
        m.0.borrow().writes,
        [
            (0xe000e010, 0),
            (0xe000e014, 3199),
            (0xe000e018, 0),
            (0xe000ed04, 1 << 25),
            (0xe000e010, 7)
        ]
    );
}

#[test]
fn failed_power_claim_is_not_usable_or_retryable() {
    let m = Model::default();
    m.force(0x400f0800, 0);
    let mut g = device(&m).gpio;
    assert_eq!(g.power(), Err(Error::Power));
    assert_eq!(g.power(), Err(Error::AlreadyOwned));
    assert!(matches!(
        g.output(Pin::new(Port::A, 0), false),
        Err(Error::Power)
    ));
}
