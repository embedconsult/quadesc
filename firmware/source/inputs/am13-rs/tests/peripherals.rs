mod support;
use am13_rs::io::RegisterIo;
use am13_rs::{
    adc::{Channel, Error as AdcError, Reference},
    can, i2c, pwm, spi, Peripherals,
};
use std::{cell::Cell, rc::Rc};
use support::Model;
fn device(m: &Model) -> Peripherals<Model> {
    unsafe { Peripherals::from_io(m.clone()) }
}
#[derive(Clone)]
struct ConversionModel {
    model: Model,
    eoc_reads: Rc<Cell<u8>>,
}
#[derive(Clone)]
struct I2cCompletionModel {
    model: Model,
    pending: Rc<Cell<u32>>,
    polls: Rc<Cell<usize>>,
}
impl I2cCompletionModel {
    fn new(model: &Model) -> Self {
        Self {
            model: model.clone(),
            pending: Rc::new(Cell::new(0)),
            polls: Rc::new(Cell::new(0)),
        }
    }
}
impl RegisterIo for I2cCompletionModel {
    fn read(&self, a: usize) -> u32 {
        if a == 0x4060_a030 {
            let count = self.polls.get() + 1;
            self.polls.set(count);
            if count % 3 == 0 {
                self.pending.get()
            } else {
                0
            }
        } else {
            self.model.read(a)
        }
    }
    fn write(&self, a: usize, v: u32) {
        if a == 0x4060_a048 {
            self.pending.set(0);
            self.polls.set(0);
        }
        if a == 0x4060_a100 && v & 2 != 0 {
            let read = self.model.value(0x4060_a14c) & 1 != 0;
            self.pending
                .set(if read { 1 } else { 2 } | if v & 4 != 0 { 1 << 9 } else { 0 });
        }
        self.model.write(a, v)
    }
    fn delay_cycles(&self, n: u32) {
        self.model.delay_cycles(n)
    }
}
impl RegisterIo for ConversionModel {
    fn read(&self, a: usize) -> u32 {
        if a == 0x4000_301c {
            let count = self.eoc_reads.get();
            self.eoc_reads.set(count + 1);
            if count == 0 {
                0
            } else {
                0x100
            }
        } else {
            self.model.read(a)
        }
    }
    fn write(&self, a: usize, v: u32) {
        self.model.write(a, v)
    }
    fn delay_cycles(&self, n: u32) {
        self.model.delay_cycles(n)
    }
}

#[test]
fn adc_preserves_trim_and_reports_fresh_result_or_timeout() {
    let m = Model::default();
    for i in 0..3 {
        m.force(0x4000_0000 + i * 0x2000 + 0x101c, 0);
    }
    let mut adc = device(&m).adc.initialize(Reference::Internal3V3).unwrap();
    assert!(m
        .0
        .borrow()
        .writes
        .iter()
        .all(|(a, _)| ![0x4000_0804, 0x4000_2804, 0x4000_4804].contains(a)));
    m.force(0x4000_301c, 0x100);
    assert_eq!(
        adc.read(Channel {
            instance: 1,
            selector: 7
        }),
        Err(AdcError::Timeout)
    ); // stale completion cannot pass
    m.force(0x4000_301c, 0);
    assert_eq!(
        adc.read(Channel {
            instance: 3,
            selector: 0
        }),
        Err(AdcError::InvalidChannel)
    );
}

#[test]
fn adc_reads_selected_raw_halfword_after_eoc() {
    let m = Model::default();
    m.force(0x4000_b000, 0xf789);
    let backend = ConversionModel {
        model: m.clone(),
        eoc_reads: Rc::new(Cell::new(0)),
    };
    let mut adc = unsafe { Peripherals::from_io(backend) }
        .adc
        .initialize(Reference::Internal3V3)
        .unwrap();
    assert_eq!(
        adc.read(Channel {
            instance: 1,
            selector: 7
        })
        .unwrap()
        .raw,
        0x789
    );
    assert_eq!(m.value(0x4000_304c), 7 << 15);
    assert_eq!(m.value(0x4000_504c), 0); // unrelated ADC2 selector untouched during read
}

#[test]
fn spi_external_frame_has_no_loopback_and_timeout_is_bounded() {
    let m = Model::default();
    let mut spi = device(&m).uc3_spi.configure().unwrap();
    assert_eq!(m.value(0x4065_8110), 159);
    assert_eq!(m.value(0x4065_8100), 0x20f);
    assert_eq!(m.value(0x4065_814c), 0x15);
    m.force(0x4065_8108, 0);
    m.force(0x4065_8124, 0x05a3);
    assert_eq!(spi.transfer_word(0x8800), Ok(0x05a3));
    assert_eq!(m.value(0x4065_8120), 0x8800);
    m.force(0x4065_8108, 4);
    assert_eq!(spi.transfer_word(0), Err(spi::Error::Timeout));
}

#[test]
fn i2c_write_address_and_nack_are_register_accurate() {
    let m = Model::default();
    let complete = I2cCompletionModel::new(&m);
    let mut i2c = unsafe { Peripherals::from_io(complete.clone()) }
        .uc2_i2c
        .configure()
        .unwrap();
    assert_eq!(m.value(0x4060_a110), 31);
    assert_eq!(i2c.write(0x7f, &[1]), Err(i2c::Error::Invalid));
    assert_eq!(i2c.write(0x40, &[0x55]), Ok(()));
    assert!(complete.polls.get() >= 3); // completion must follow the transfer start
    assert_eq!(m.value(0x4060_a14c), 0x80);
    assert_eq!(m.value(0x4060_a100), 0x0001_0007);
    m.force(0x4060_a108, 4);
    assert_eq!(i2c.write(0x40, &[1, 2]), Err(i2c::Error::AddressNack));
}

#[test]
fn i2c_write_read_uses_repeated_start_without_intermediate_stop() {
    let m = Model::default();
    let mut i2c = unsafe { Peripherals::from_io(I2cCompletionModel::new(&m)) }
        .uc2_i2c
        .configure()
        .unwrap();
    m.force(0x4060_a124, 0xa5);
    let mut response = [0; 2];
    i2c.write_read(0x48, &[0x10], &mut response).unwrap();
    assert_eq!(response, [0xa5, 0xa5]);
    let controls: Vec<u32> =
        m.0.borrow()
            .writes
            .iter()
            .filter(|(a, _)| *a == 0x4060_a100)
            .map(|(_, v)| *v)
            .collect();
    assert_eq!(controls, [0x0001_0003, 0x0002_0007]);
    assert_eq!(m.value(0x4060_a14c), (0x48 << 1) | 1);
}

#[test]
fn i2c_idle_status_does_not_complete_a_transfer() {
    let m = Model::default();
    let mut i2c = device(&m).uc2_i2c.configure().unwrap();
    assert_eq!(i2c.write(0x40, &[0x55]), Err(i2c::Error::Timeout));
}

#[test]
fn pwm_quantizes_shared_period_and_forces_low_on_stop() {
    let m = Model::default();
    let mut modules = device(&m).pwm.split();
    let p = &mut modules[1];
    assert_eq!(p.configure_frequency(20_000).unwrap().period_ticks, 1600);
    let e = p.set_width_ticks(5, 400).unwrap();
    assert_eq!((e.frequency_hz, e.high_ticks), (20_000, 400));
    assert_eq!(m.value(0x4001_1508), 400); // MCPWM1 PWM3 CMPB
    assert_eq!(p.set_duty_permyriad(5, 2_500).unwrap().high_ticks, 400);
    assert_eq!(p.set_width_ns(5, 12_500).unwrap().high_ticks, 400);
    assert_eq!(p.set_width_ticks(6, 1), Err(pwm::Error::InvalidOutput));
    p.start().unwrap();
    p.stop();
    assert_eq!(m.value(0x4001_1530), 0x11);
    assert_eq!(m.value(0x4001_1010) & 3, 2);
}

#[test]
fn can_xtal_clock_and_classical_ram_layout() {
    let m = Model::default();
    m.force(0x400b_0204, 1 << 8); // XTAL good, SYSOSC selection still valid
    m.force(0x4011_7208, 2); // message RAM initialization complete
    assert!(matches!(
        device(&m).mcan0.configure(333_333),
        Err(can::Error::InvalidBitrate)
    ));
    let mut can = device(&m).mcan0.configure(250_000).unwrap();
    assert_eq!(can.nominal_bitrate, 250_000);
    assert_eq!(m.value(0x400b_0140) & (1 << 8), 0); // CANCLK from XTAL
    assert_eq!(m.value(0x4011_70a0), 1 << 16); // one RX FIFO0 element
    assert_eq!(m.value(0x4011_70c0), (1 << 16) | 0x10); // TX at 0x10
    assert_eq!(
        m.value(0x4011_701c),
        ((2 - 1) << 16) | ((19 - 1) << 8) | (5 - 1) | ((5 - 1) << 25)
    );
    assert_eq!(can.receive(), Ok(None));
    assert_eq!(can::Frame::new(0x800, &[1]), Err(can::Error::InvalidFrame));
    m.force(0x4011_70d8, 1); // model a completed TX
    m.force(0x4011_7050, 1 << 9); // new transmission-complete event
    can.send(can::Frame::new(0x123, &[1, 2, 3, 4, 5]).unwrap())
        .unwrap();
    assert_eq!(m.value(0x4011_0010), 0x123 << 18);
    assert_eq!(m.value(0x4011_0014), 5 << 16);
    assert_eq!(m.value(0x4011_0018), 0x0403_0201);
    m.force(0x4011_70a4, 1);
    m.force(0x4011_0000, 0x321 << 18);
    m.force(0x4011_0004, 2 << 16);
    m.force(0x4011_0008, 0x0000_bbaa);
    let rx = can.receive().unwrap().unwrap();
    assert_eq!(
        (rx.id, rx.len, rx.data[0], rx.data[1]),
        (0x321, 2, 0xaa, 0xbb)
    );
    assert_eq!(m.value(0x4011_70a8), 0);
}

#[test]
fn finite_pulse_hardware_shadow_disarms_rising_edge_without_polling() {
    for output in 0..6 {
        let m=Model::default();let mut modules=device(&m).pwm.split();let p=&mut modules[0];
        let e=p.single_shot_ns(output,1000).unwrap();assert_eq!(e.high_ticks,32);
        assert_eq!(m.value(0x400b048c)&1,1); // SYSCTL clock sync actually enabled
        assert_eq!(p.single_shot_ns(output,1000),Err(pwm::Error::Busy));
        let base=0x40010000;let unit=output as usize/2;let side=(output as usize&1)*8;
        let aq=base+0x120+unit*0x200+side;let cmp=base+0x100+unit*0x200+side;
        let period=m.value(base+0x14);let shift=unit*8+(output as usize&1)*2;
        assert_eq!((m.value(base+0x50)>>shift)&3,1); // load only at PERIOD
        let mut level=false;let mut rises=0;let mut high_ticks=0;
        // Event model from TI TRM26.6.4: active AQ actions and period shadow load.
        // Deliberately never call software poll for twenty counter wraps.
        for _ in 0..20 {for count in 0..=period {
            let active=m.value(aq);
            let event=if count==0 {Some(0)} else if count==m.value(cmp) {Some(if output&1==0{4}else{8})} else if count==period{Some(2)}else{None};
            if let Some(shift)=event {let action=(active>>shift)&3;let old=level;match action{1=>level=false,2=>level=true,3=>level=!level,_=>{}}if !old&&level{rises+=1;}}
            if level {high_ticks+=1;}
            if count==period {m.write(aq,m.value(aq+4));}
        }}
        assert_eq!((rises,high_ticks,level),(1,32,false));
        assert!(p.poll_single_shot());assert!(!p.poll_single_shot());
        p.single_shot_ns(output,2000).unwrap();p.stop();assert!(!p.poll_single_shot());
        assert_eq!(m.value(base+0x130+unit*0x200),0x11);
        p.single_shot_ns(output,1000).unwrap(); // explicit rearm after cancel
    }
}
#[test]
fn duty_edits_including_one_hundred_percent_stay_forced_low_until_start(){
 let m=Model::default();let mut modules=device(&m).pwm.split();let p=&mut modules[0];
 p.configure_frequency(1000).unwrap();p.set_duty_permyriad(0,10000).unwrap();
 assert_eq!(m.value(0x40010130),0x11);
 p.start().unwrap();assert_eq!(m.value(0x40010130)&7,2);p.stop();assert_eq!(m.value(0x40010130),0x11);
}
