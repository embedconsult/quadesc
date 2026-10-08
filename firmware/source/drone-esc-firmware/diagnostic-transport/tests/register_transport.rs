use am13_diagnostic_transport::{TxFault, read_yielding, write_admitted_response, write_bounded};
use am13_rs::{Peripherals, io::RegisterIo, uart::Error};
use bloxide_calibration::OperationKey;
use embassy_executor::raw::Executor;
use embassy_time::{Instant, Timer};
use esc_som_board::initialize;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

#[derive(Default)]
struct Registers {
    values: BTreeMap<usize, u32>,
    forced: BTreeMap<usize, u32>,
    auto_eot: bool,
    tx: Vec<u8>,
}
#[derive(Clone, Default)]
struct Model(Arc<Mutex<Registers>>);
impl Model {
    fn force(&self, address: usize, value: u32) {
        self.0.lock().unwrap().forced.insert(address, value);
    }
    fn eot(&self, enabled: bool) {
        self.0.lock().unwrap().auto_eot = enabled;
    }
    fn tx(&self) -> Vec<u8> {
        self.0.lock().unwrap().tx.clone()
    }
}
impl RegisterIo for Model {
    fn read(&self, address: usize) -> u32 {
        let r = self.0.lock().unwrap();
        *r.forced
            .get(&address)
            .or(r.values.get(&address))
            .unwrap_or(&0)
    }
    fn write(&self, address: usize, value: u32) {
        let mut r = self.0.lock().unwrap();
        r.values.insert(address, value);
        for base in [0x400f0000, 0x400f2000, 0x400f4000] {
            for (offset, register, set) in [
                (0x1290, 0x1280, true),
                (0x12a0, 0x1280, false),
                (0x12d0, 0x12c0, true),
                (0x12e0, 0x12c0, false),
            ] {
                if address == base + offset {
                    let old = *r.values.get(&(base + register)).unwrap_or(&0);
                    let next = if set { old | value } else { old & !value };
                    r.values.insert(base + register, next);
                    if register == 0x1280 {
                        r.values.insert(base + 0x1380, next);
                    }
                }
            }
        }
        if address == 0x40641048 {
            r.values.insert(0x40641030, 0);
        }
        if address == 0x40641120 {
            r.tx.push(value as u8);
            if r.auto_eot {
                r.values.insert(0x40641030, 1 << 12);
            }
        }
    }
    fn delay_cycles(&self, _: u32) {}
}
fn uart(model: &Model) -> am13_rs::uart::Uart<Model> {
    initialize(unsafe { Peripherals::from_io(model.clone()) }, 19_200)
        .unwrap()
        .uart
}
static DONE: AtomicBool = AtomicBool::new(false);
static HEARTBEATS: AtomicUsize = AtomicUsize::new(0);
static UNCERTAIN: Mutex<Option<OperationKey>> = Mutex::new(None);
fn record_uncertain(key: OperationKey) {
    *UNCERTAIN.lock().unwrap() = Some(key);
}
fn key(sequence: u64) -> OperationKey {
    OperationKey {
        service_epoch: 9,
        session_generation: 3,
        sequence,
    }
}
#[unsafe(export_name = "__pender")]
fn pender(_: *mut ()) {}

#[embassy_executor::task]
async fn heartbeat() {
    while !DONE.load(Ordering::Relaxed) {
        HEARTBEATS.fetch_add(1, Ordering::Relaxed);
        Timer::after_ticks(1).await;
    }
}
#[embassy_executor::task]
async fn scenarios() {
    let good = Model::default();
    good.eot(true);
    let mut port = uart(&good);
    assert_eq!(write_bounded(&mut port, b"OK\n").await, Ok(()));
    assert_eq!(good.tx(), b"OK\n");

    let bad = Model::default();
    let mut port = uart(&bad);
    let before = Instant::now();
    assert_eq!(
        write_admitted_response(&mut port, b"OK\n", key(41), record_uncertain).await,
        Err(TxFault::Timeout)
    );
    assert!((4000..=4002).contains(&(Instant::now() - before).as_ticks()));
    assert_eq!(bad.tx(), b"O"); // admitted, final stop bit unknown
    assert_eq!(*UNCERTAIN.lock().unwrap(), Some(key(41)));

    let full = Model::default();
    let mut port = uart(&full);
    full.force(0x40641108, 1 << 6);
    let before = Instant::now();
    assert_eq!(
        write_admitted_response(&mut port, b"OK\n", key(42), record_uncertain).await,
        Err(TxFault::Timeout)
    );
    assert!((4000..=4002).contains(&(Instant::now() - before).as_ticks()));
    assert!(full.tx().is_empty()); // known byte nonadmission
    assert_eq!(*UNCERTAIN.lock().unwrap(), Some(key(42)));

    let broken = Model::default();
    let mut port = uart(&broken);
    broken.force(0x40641030, 1 << 17);
    let before = Instant::now();
    for _ in 0..20 {
        assert_eq!(read_yielding(&mut port).await, Err(Error::Receive));
    }
    assert!((200..=220).contains(&(Instant::now() - before).as_ticks()));
    DONE.store(true, Ordering::Relaxed);
}
#[test]
fn pinned_time_driver_bounds_tx_and_yields_rx_for_other_tasks() {
    let executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    executor.spawner().spawn(scenarios()).unwrap();
    executor.spawner().spawn(heartbeat()).unwrap();
    for _ in 0..8500 {
        unsafe {
            executor.poll();
            am13_rs::time::on_tick();
        }
        if DONE.load(Ordering::Relaxed) {
            break;
        }
    }
    assert!(DONE.load(Ordering::Relaxed));
    assert!(HEARTBEATS.load(Ordering::Relaxed) > 8000);
}
