mod support;
use am13_rs::{gpio::{Pin, Port}, Peripherals};
use support::Model;
fn device(m: &Model) -> Peripherals<Model> { unsafe { Peripherals::from_io(m.clone()) } }

#[test]
fn gpio_input_observes_pad_not_output_latch() {
    let m = Model::default();
    let mut g = device(&m).gpio;
    g.power().unwrap();
    let pin = Pin::new(Port::B, 15);
    let input = g.input_owned(pin, am13_rs::gpio::Pull::None).unwrap();
    m.force(pin.base() + 0x1280, 0);
    m.force(pin.base() + 0x1380, 1 << pin.bit());
    assert!(input.is_high());
    m.force(pin.base() + 0x1280, 1 << pin.bit());
    m.force(pin.base() + 0x1380, 0);
    assert!(!input.is_high());
}
