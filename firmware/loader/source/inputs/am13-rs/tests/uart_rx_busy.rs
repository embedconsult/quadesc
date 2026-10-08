//! R1 correctness regression derived from the independent receive-only BUSY probe.
#![cfg(feature = "embassy")]
use am13_rs::{Peripherals, io::RegisterIo, uart::Uart};
use core::sync::atomic::{AtomicU32, Ordering::SeqCst};
use embassy_executor::raw::Executor;
use embedded_io_async::Write;
static REGS: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];
static STATUS: AtomicU32 = AtomicU32::new(0x25); // TXFE | RXFE | BUSY (RX shifting)
static DONE: AtomicU32 = AtomicU32::new(0);
static TX_WRITES: AtomicU32 = AtomicU32::new(0);
#[derive(Clone)]
struct RxOnly;
fn index(a: usize) -> Option<usize> {
    [0x40641000, 0x40641008, 0x40641100, 0x40641110, 0x40641114]
        .iter()
        .position(|x| *x == a)
}
impl RegisterIo for RxOnly {
    fn read(&self, a: usize) -> u32 {
        if let Some(i) = index(a) {
            return REGS[i].load(SeqCst);
        }
        match a {
            0x400b0104 => 7 << 24,
            0x40672800 => 1,
            0x40641108 => STATUS.load(SeqCst),
            _ => 0,
        }
    }
    fn write(&self, a: usize, v: u32) {
        if let Some(i) = index(a) {
            REGS[i].store(v, SeqCst);
        }
        if a == 0x40641120 {
            TX_WRITES.fetch_add(1, SeqCst);
        }
    }
    fn delay_cycles(&self, _: u32) {}
}
#[unsafe(export_name = "__pender")]
fn pender(_: *mut ()) {}
#[embassy_executor::task]
async fn flush_task(mut uart: Uart<RxOnly>) {
    // No byte has ever been transmitted. Reception should not delay TX flush.
    uart.flush().await.unwrap();
    DONE.store(1, SeqCst);
}
#[test]
fn receive_only_busy_does_not_stall_empty_transmitter_flush() {
    let p = unsafe { Peripherals::from_io(RxOnly) };
    let clocks = p.clocks.sysosc_32mhz().unwrap();
    let uart = p.uc4.configure(clocks, 38400).unwrap().enable().unwrap();
    let executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    executor.spawner().spawn(flush_task(uart)).unwrap();
    for _ in 0..100 {
        unsafe {
            executor.poll();
            am13_rs::time::on_tick();
        }
    }
    assert_eq!(TX_WRITES.load(SeqCst), 0);
    assert_eq!(DONE.load(SeqCst), 1); // RX stayed busy throughout; zero TX writes
}
