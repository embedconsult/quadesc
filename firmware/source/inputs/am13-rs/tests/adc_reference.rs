mod support;
use am13_rs::{
    Peripherals,
    adc::{Channel, Error, Reference},
    io::RegisterIo,
};
use std::{cell::RefCell, rc::Rc};
use support::Model;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Event {
    Read(usize),
    Write(usize, u32),
    Delay(u32),
}
#[derive(Clone, Default)]
struct Trace {
    model: Model,
    events: Rc<RefCell<Vec<Event>>>,
}
impl RegisterIo for Trace {
    fn read(&self, a: usize) -> u32 {
        self.events.borrow_mut().push(Event::Read(a));
        self.model.read(a)
    }
    fn write(&self, a: usize, v: u32) {
        self.events.borrow_mut().push(Event::Write(a, v));
        self.model.write(a, v);
    }
    fn delay_cycles(&self, n: u32) {
        self.events.borrow_mut().push(Event::Delay(n));
    }
}
const REF: usize = 0x400b_0488;

#[test]
fn explicit_modes_replace_inherited_reference_and_settle_after_all_cores() {
    for (mode, bits) in [
        (Reference::Internal3V3, 0x000),
        (Reference::Internal2V5, 0x100),
        (Reference::ExternalHalfScale, 0x001),
        (Reference::ExternalFullScale, 0x101),
    ] {
        let t = Trace::default();
        t.model.write(REF, bits ^ 0x101);
        let mut adc = unsafe { Peripherals::from_io(t.clone()) }
            .adc
            .initialize(mode)
            .unwrap();
        assert_eq!(t.model.value(REF), bits);
        let events = t.events.borrow();
        assert_eq!(&events[..2], &[Event::Write(REF, bits), Event::Read(REF)]);
        assert!(matches!(events.last(), Some(Event::Delay(n)) if *n >= 160_000));
        let settle = events.len() - 1;
        for base in [0x4000_0000, 0x4000_2000, 0x4000_4000] {
            let powered = events
                .iter()
                .position(|e| *e == Event::Write(base + 0x1000, 0x84))
                .unwrap();
            assert!(powered < settle);
            assert!(events[powered + 1..settle].contains(&Event::Read(base + 0x1000)));
            assert!(!events.iter().any(
                |e| matches!(e, Event::Write(a, _) if [base + 0x804, base + 0x10e8].contains(a))
            ));
        }
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::Write(_, v) if *v == 0xc000_00bf))
        );
        drop(events);
        // With no completion event, a fresh trigger must time out; it can only
        // be requested after the ReadyAdcs owner (and the settling call) exists.
        assert_eq!(
            adc.read(Channel {
                instance: 0,
                selector: 0
            }),
            Err(Error::Timeout)
        );
        let events = t.events.borrow();
        let trigger = events
            .iter()
            .position(|e| *e == Event::Write(0x4000_1330, 0xc000_00bf))
            .unwrap();
        assert!(settle < trigger);
    }
}

#[test]
fn either_reference_bit_mismatch_rejects_ready_before_core_configuration() {
    for wrong in [0x001, 0x100, 0x101] {
        let t = Trace::default();
        t.model.force(REF, wrong);
        assert!(matches!(
            unsafe { Peripherals::from_io(t.clone()) }
                .adc
                .initialize(Reference::Internal3V3),
            Err(Error::NotReady)
        ));
        assert_eq!(*t.events.borrow(), [Event::Write(REF, 0), Event::Read(REF)]);
    }
}

#[test]
fn power_readback_failure_never_publishes_ready() {
    for instance in 0..3 {
        for offset in [0x800, 0x1000] {
            let t = Trace::default();
            t.model.force(0x4000_0000 + instance * 0x2000 + offset, 0);
            assert!(matches!(
                unsafe { Peripherals::from_io(t.clone()) }
                    .adc
                    .initialize(Reference::Internal3V3),
                Err(Error::NotReady)
            ));
            assert!(!t.events.borrow().contains(&Event::Delay(160_000)));
        }
    }
}
