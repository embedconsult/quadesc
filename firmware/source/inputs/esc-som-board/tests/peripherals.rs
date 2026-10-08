mod support;
use am13_rs::Peripherals;
use esc_som_board::{peripherals, pins};
use support::Model;

#[test]
fn v3_profile_owns_correct_pads_and_starts_switches_low() {
    let csv = include_str!("../reference-inputs/summary.csv");
    for entry in peripherals::ANALOG {
        let (pad, channel) = entry.name.split_once('/').unwrap();
        assert!(
            csv.lines()
                .any(|line| line.starts_with(&format!("{pad},")) && line.contains(channel))
        );
    }
    for (pin, mux, module, output) in pins::PWM_ROUTES {
        let p = match pin.port() {
            am13_rs::gpio::Port::A => 'A',
            am13_rs::gpio::Port::B => 'B',
            am13_rs::gpio::Port::C => 'C',
        };
        assert!(
            csv.lines()
                .any(|line| line.starts_with(&format!("P{p}{},", pin.bit()))
                    && line.split(',').nth(2).unwrap().parse::<u8>().unwrap() == mux
                    && line.contains(&format!(
                        "MCPWM{module}_{}{}",
                        output / 2 + 1,
                        if output & 1 == 0 { 'A' } else { 'B' }
                    )))
        );
    }
    let m = Model::default();
    let _r =
        peripherals::initialize(unsafe { Peripherals::from_io(m.clone()) }, 38_400).unwrap();
    assert!(!m.0.borrow().writes.iter().any(|(a,_)| *a==pins::CAN_TERM.pad()));
    assert_eq!(m.value(pins::SPI_SCLK.pad()) & 0x1f, 6);
    assert_eq!(m.value(pins::I2C_SDA.pad()) & 0x1f, 4);
    assert_eq!(m.value(pins::BSL_CAN_RX.pad()) & 0x1f, 10);
    assert_eq!(m.value(peripherals::ANALOG[0].pin.pad()), 0);
    let state = m.0.borrow();
    let writes = &state.writes;
    let first_pwm_mux = writes
        .iter()
        .position(|(a, _)| *a == pins::PWM_ROUTES[0].0.pad())
        .unwrap();
    for module in 0..4 {
        let force = writes
            .iter()
            .position(|(a, v)| *a == 0x4001_0130 + module * 0x1000 && *v == 0x11)
            .unwrap();
        assert!(force < first_pwm_mux);
    }
    drop(state);
    assert!(
        m.0.borrow()
            .writes
            .iter()
            .all(|(a, _)| !(0x4011_0000..0x4011_8000).contains(a))
    );

}

#[test]
fn drv8323_register_frame_selects_only_requested_chip() {
    let m = Model::default();
    let mut board =
        peripherals::initialize(unsafe { Peripherals::from_io(m.clone()) }, 38_400).unwrap();
    m.force(0x4065_8108, 0); // UC3 idle, transmit room and reply present
    m.force(0x4065_8124, 0x05a3);
    let start = m.0.borrow().writes.len();
    assert_eq!(board.drivers.read_register(2, 3).unwrap(), 0x05a3);
    assert_eq!(m.value(0x4065_8120), 0x8000 | (3 << 11));
    let state = m.0.borrow();
    let writes = &state.writes[start..];
    let chip = pins::DRIVER_CS[2];
    assert!(
        writes
            .iter()
            .any(|(a, v)| *a == chip.base() + 0x12a0 && *v == 1 << chip.bit())
    );
    assert!(
        writes
            .iter()
            .any(|(a, v)| *a == chip.base() + 0x1290 && *v == 1 << chip.bit())
    );
    for other in [0, 1, 3] {
        let pin = pins::DRIVER_CS[other];
        assert!(!writes.iter().any(|(a,v)| *a == pin.base() + 0x12a0 && *v == 1<<pin.bit()));
    }
}

#[test]
fn diagnostic_enable_sequence_forces_all_pwm_low_and_inl_low_before_drv_high() {
    let m=Model::default();
    let mut board=peripherals::initialize(unsafe { Peripherals::from_io(m.clone()) },38_400).unwrap();
    let start=m.0.borrow().writes.len();
    // Concrete mainboard.rs Hardware::stop + enables(1) sequence.
    for p in &mut board.pwm {p.stop();}
    board.drivers.set_inl_enabled(false).unwrap();
    board.drivers.set_enabled(true).unwrap();
    board.drivers.set_inl_enabled(false).unwrap();
    let state=m.0.borrow();let writes=&state.writes[start..];
    let inl_low=writes.iter().position(|&(a,v)|a==pins::INL_ENABLE.base()+0x12a0 && v==1<<pins::INL_ENABLE.bit()).unwrap();
    let drv_high=writes.iter().position(|&(a,v)|a==pins::DRV_ENABLE.base()+0x1290 && v==1<<pins::DRV_ENABLE.bit()).unwrap();
    assert!(inl_low<drv_high);
    assert!(!writes.iter().any(|&(a,v)|a==pins::INL_ENABLE.base()+0x1290 && v==1<<pins::INL_ENABLE.bit()));
    for module in 0..4 {for pair in 0..3 {
        let at=0x4001_0130+module*0x1000+pair*0x200;
        assert_eq!(m.value(at),0x11); // both A and B continuous software force LOW
        assert!(writes.iter().position(|&(a,v)|a==at && v==0x11).unwrap()<drv_high);
    }assert_eq!(m.value(0x4001_0010+module*0x1000)&3,2);}
}
