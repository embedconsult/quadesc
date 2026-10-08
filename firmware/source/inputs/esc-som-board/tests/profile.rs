mod support;
use am13_rs::{Peripherals, gpio::Error};
use esc_som_board::{StartupError, initialize, pins};
use support::Model;
#[test]
fn all_motor_outputs_inactive_and_led_off_before_uart_mux() {
    let m = Model::default();
    let mut r = initialize(unsafe { Peripherals::from_io(m.clone()) }, 38_400).unwrap();
    let s = m.0.borrow();
    let first_uart_mux = s
        .writes
        .iter()
        .position(|(a, _)| *a == pins::BSL_UART_TX.pad())
        .unwrap();
    for (pin, high) in [pins::DRV_ENABLE, pins::INL_ENABLE]
        .into_iter()
        .chain(pins::MOTOR_OUTPUTS)
        .map(|p| (p, false))
        .chain(pins::DRIVER_CS.map(|p| (p, true)))
        .chain([(pins::STATUS_LED, true)])
    {
        let latch = s
            .writes
            .iter()
            .position(|(a, v)| {
                *a == pin.base() + if high { 0x1290 } else { 0x12a0 } && *v == 1 << pin.bit()
            })
            .unwrap();
        let doe = s
            .writes
            .iter()
            .position(|(a, v)| *a == pin.base() + 0x12d0 && *v == 1 << pin.bit())
            .unwrap();
        let mux = s.writes.iter().position(|(a, _)| *a == pin.pad()).unwrap();
        assert!(latch < doe && doe < mux && mux < first_uart_mux);
        assert_eq!(
            s.values[&(pin.base() + 0x1280)] & (1 << pin.bit()) != 0,
            high
        );
    }
    for pin in [
        pins::BSL_CAN_TX,
        pins::BSL_CAN_RX,
        pins::CAN_TERM,
        am13_rs::gpio::Pin::new(am13_rs::gpio::Port::A, 13),
        am13_rs::gpio::Pin::new(am13_rs::gpio::Port::A, 14),
    ] {
        assert!(!s.writes.iter().any(|(a, _)| *a == pin.pad()));
    }
    assert!(
        s.writes
            .iter()
            .all(|(a, _)| (0x40000000..0x40700000).contains(a))
    );
    assert_eq!(s.values[&0x40641110], 52);
    assert_eq!(s.values[&0x40641114], 5);
    drop(s);
    let before = m.0.borrow().writes.len();
    r.led.set_on(true).unwrap();
    r.led.verify(true).unwrap();
    r.led.set_on(false).unwrap();
    assert_eq!(
        &m.0.borrow().writes[before..],
        &[(0x400f32a0, 1 << 18), (0x400f3290, 1 << 18)]
    );
}
#[test]
fn forced_led_level_is_failure_not_optical_success() {
    let m = Model::default();
    let mut r = initialize(unsafe { Peripherals::from_io(m.clone()) }, 38_400).unwrap();
    m.force(0x400f3380, 1 << 18);
    assert_eq!(r.led.set_on(true), Err(Error::Readback));
}
#[test]
fn failed_safe_output_check_stops_before_uart() {
    let m = Model::default();
    m.force(0x400f3380, 1 << 10);
    assert!(matches!(
        initialize(unsafe { Peripherals::from_io(m.clone()) }, 38_400),
        Err(StartupError::Gpio(Error::Readback))
    ));
    assert!(
        !m.0.borrow()
            .writes
            .iter()
            .any(|(a, _)| (0x40600000..0x40700000).contains(a))
    );
}
#[test]
fn pinned_csv_confirms_led_uart_and_enable_assignments() {
    let csv = include_str!("../reference-inputs/summary.csv");
    for row in [
        "PB18,84,1,GPIO50",
        "PA0,20,7,UC4",
        "PA1,21,7,UC4",
        "PB10,79,1,GPIO42",
        "PB11,80,1,GPIO43",
    ] {
        assert!(csv.contains(row));
    }
    assert_eq!(
        (pins::STATUS_LED.port(), pins::STATUS_LED.bit()),
        (am13_rs::gpio::Port::B, 18)
    );
}

#[test]
fn every_protected_output_occurs_in_current_pin_authority() {
    let csv = include_str!("../reference-inputs/summary.csv");
    for pin in pins::MOTOR_OUTPUTS
        .into_iter()
        .chain(pins::DRIVER_CS)
        .chain([pins::DRV_ENABLE, pins::INL_ENABLE, pins::STATUS_LED])
    {
        let port = match pin.port() {
            am13_rs::gpio::Port::A => 'A',
            am13_rs::gpio::Port::B => 'B',
            am13_rs::gpio::Port::C => 'C',
        };
        assert!(
            csv.lines()
                .any(|row| row.starts_with(&format!("P{port}{},", pin.bit())))
        );
    }
}

#[test]
fn moved_board_uart_completes_tx_while_rx_remains_busy() {
    let m = Model::default();
    let r = initialize(unsafe { Peripherals::from_io(m.clone()) }, 38_400).unwrap();
    let mut uart = r.uart; // sole service owner; no board/peripheral reacquisition
    m.force(0x40641108, 0x25);
    assert!(uart.is_tx_complete());
    assert!(uart.try_write(0x55));
    assert!(!uart.is_tx_complete()); // TXFE alone does not prove final stop
    m.force(0x40641030, 0x1002);
    assert!(uart.is_tx_complete());
    assert_eq!(uart.try_read(), Err(am13_rs::uart::Error::Receive));
    assert!(uart.is_tx_complete());
}

#[test]
fn initializer_programs_application_selected_baud() {
    for (baud, integer, fractional) in [(19_200, 104, 11), (38_400, 52, 5)] {
        let m = Model::default();
        let resources = initialize(unsafe { Peripherals::from_io(m.clone()) }, baud).unwrap();
        let s = m.0.borrow();
        assert_eq!(s.values[&0x40641110], integer);
        assert_eq!(s.values[&0x40641114], fractional);
        let tx_mux = s
            .writes
            .iter()
            .position(|(a, _)| *a == pins::BSL_UART_TX.pad())
            .unwrap();
        let divisor = s
            .writes
            .iter()
            .position(|(a, v)| *a == 0x40641114 && *v == fractional)
            .unwrap();
        assert!(divisor < tx_mux);
        drop(s);
        let _uart = resources.uart; // resources are moved from the one initializer call
    }
}

#[test]
fn invalid_baud_rejects_before_uart_programming_or_mux() {
    for baud in [0, 4_000_000] {
        let m = Model::default();
        assert!(matches!(
            initialize(unsafe { Peripherals::from_io(m.clone()) }, baud),
            Err(StartupError::Uart(am13_rs::uart::Error::Baud))
        ));
        let s = m.0.borrow();
        assert!(
            !s.writes
                .iter()
                .any(|(a, _)| (0x40600000..0x40700000).contains(a))
        );
        assert!(
            !s.writes
                .iter()
                .any(|(a, _)| *a == pins::BSL_UART_TX.pad() || *a == pins::BSL_UART_RX.pad())
        );
        assert_eq!(
            s.values[&(pins::STATUS_LED.base() + 0x1280)] & (1 << pins::STATUS_LED.bit()) != 0,
            true
        );
    }
}
