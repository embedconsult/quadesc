mod support;
use am13_rs::{
    Peripherals,
    adc::{Channel, Error, Reference},
    io::RegisterIo,
};
use support::Model;
#[derive(Clone, Default)]
struct Io {
    m: Model,
    complete: bool,
}
const B: usize = 0x40002000;
impl RegisterIo for Io {
    fn read(&self, a: usize) -> u32 {
        self.m.read(a)
    }
    fn write(&self, a: usize, v: u32) {
        self.m.write(a, v);
        if a == B + 0x1024 {
            self.m.write(B + 0x101c, 0);
        }
        if a == B + 0x102c {
            self.m.write(B + 0x1028, 0);
        }
        if a == B + 0x1330 && v & (1 << 30) != 0 && self.complete {
            self.m.write(B + 0x101c, 0x100);
            self.m.write(0x4000b000, 1977);
        }
    }
    fn delay_cycles(&self, _: u32) {}
}
fn channel() -> Channel {
    Channel {
        instance: 1,
        selector: 22,
    }
}
#[test]
fn hardware_trigger_route_no_software_force_restores_and_next_legacy_read_works() {
    let io = Io {
        complete: true,
        ..Default::default()
    };
    let mut a = unsafe { Peripherals::from_io(io.clone()) }
        .adc
        .initialize(Reference::Internal3V3)
        .unwrap();
    for (cycles, bits) in [(128, 0x5f), (256, 0x8f), (448, 0xbf)] {
        io.m.0.borrow_mut().writes.clear();
        let mut arm = vec![];
        let r = a
            .capture(channel(), cycles, true, |on| {
                arm.push(on);
                if on {
                    assert_eq!(io.m.value(B + 0x1330), 0x80600000 | bits);
                    io.m.write(B + 0x101c, 0x100);
                    io.m.write(0x4000b000, 2001);
                }
            })
            .unwrap();
        assert_eq!(r.raw, 2001);
        assert_eq!(arm, [false, true, false, false]);
        assert_eq!(io.m.value(B + 0x1330), 0x800000bf);
        assert!(
            !io.m
                .0
                .borrow()
                .writes
                .iter()
                .any(|&(a, v)| a == B + 0x1330 && v & (1 << 30) != 0)
        );
        assert_eq!(a.read(channel()).unwrap().raw, 1977);
    }
}
#[test]
fn software_matched_window_timeout_config_failure_and_bounded_reads() {
    for complete in [false, true] {
        let io = Io {
            complete,
            ..Default::default()
        };
        let mut a = unsafe { Peripherals::from_io(io.clone()) }
            .adc
            .initialize(Reference::Internal3V3)
            .unwrap();
        let r = a.capture(channel(), 128, false, |on| assert!(!on));
        assert_eq!(
            r.map(|v| v.raw),
            if complete {
                Ok(1977)
            } else {
                Err(Error::Timeout)
            }
        );
        assert_eq!(io.m.value(B + 0x1330), 0x800000bf);
        assert!(io.m.0.borrow().reads.len() < 1100);
        assert_eq!(
            a.capture(channel(), 1, true, |_| panic!()),
            Err(Error::InvalidChannel)
        );
    }
}
#[test]
fn busy_timeout_leaves_detached_not_reconfigured() {
    let io = Io::default();
    let mut a = unsafe { Peripherals::from_io(io.clone()) }
        .adc
        .initialize(Reference::Internal3V3)
        .unwrap();
    io.m.force(B + 0x1000, 1 << 13);
    assert_eq!(
        a.capture(channel(), 128, true, |on| assert!(!on)),
        Err(Error::Timeout)
    );
    assert_eq!(io.m.value(B + 0x1330), 0);
}
#[test]
fn pwm_trigger_configuration_does_not_change_output_compares_or_shared_clock() {
    let io = Io::default();
    let p = unsafe { Peripherals::from_io(io.clone()) };
    let mut pwm = p.pwm.split();
    pwm[0].configure_frequency(50000).unwrap();
    pwm[0].set_duty_permyriad(0, 5000).unwrap();
    pwm[0].start().unwrap();
    io.m.force(0x40010000, 4);
    io.m.0.borrow_mut().writes.clear();
    pwm[0].adc_trigger_configure(480).unwrap();
    pwm[0].adc_trigger_enable(true);
    assert_eq!(io.m.value(0x40010040), 480);
    assert_eq!(io.m.value(0x40010064), 8);
    assert_eq!(io.m.value(0x40010068), 1);
    assert!(io.m.0.borrow().writes.iter().all(|&(a, _)| {
        [
            0x40010060, 0x40010030, 0x40010040, 0x40010064, 0x40010068, 0x40010074,
        ]
        .contains(&a)
    }));
    pwm[0].stop();
    assert_eq!(io.m.value(0x40010060), 0);
    assert!(pwm[0].adc_trigger_configure(640).is_err());
}

#[test]
fn completion_overflow_or_config_readback_mismatch_is_rejected_and_detached() {
    for overflow in [false, true] {
        let io = Io::default();
        let mut adc = unsafe { Peripherals::from_io(io.clone()) }
            .adc
            .initialize(Reference::Internal3V3)
            .unwrap();
        if !overflow {
            io.m.force(B + 0x1330, 0);
        }
        let result = adc.capture(channel(), 128, true, |on| {
            if on {
                io.m.write(B + 0x101c, 0x100);
                io.m.write(B + 0x1028, 1);
            }
        });
        assert!(result.is_err());
        assert_eq!(io.m.value(B + 0x1330), 0x800000bf);
    }
}
