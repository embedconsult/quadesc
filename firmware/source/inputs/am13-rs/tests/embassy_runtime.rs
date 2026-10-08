#![cfg(feature = "embassy")]
use core::{
    alloc::{GlobalAlloc, Layout},
    future::{Future, poll_fn},
    sync::atomic::{AtomicUsize, Ordering},
    task::Poll,
};
#[path = "support/uart_model.rs"]
mod uart_model;
use embassy_executor::raw::Executor;
use embassy_time::{Duration, Instant, Timer, with_timeout};
use std::{alloc::System, cell::Cell};
thread_local! { static COUNT: Cell<bool> = const { Cell::new(false) }; static OPS: Cell<[usize;4]> = const { Cell::new([0;4]) }; }
struct Allocator;
fn count(i: usize) {
    COUNT.with(|c| {
        if c.get() {
            OPS.with(|o| {
                let mut v = o.get();
                v[i] += 1;
                o.set(v);
            });
        }
    });
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count(0);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        count(1);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        count(2);
        unsafe { System.realloc(p, l, n) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        count(3);
        unsafe { System.dealloc(p, l) }
    }
}
#[global_allocator]
static ALLOC: Allocator = Allocator;
static DONE: AtomicUsize = AtomicUsize::new(0);
static PENDS: AtomicUsize = AtomicUsize::new(0);
#[unsafe(export_name = "__pender")]
fn pender(_: *mut ()) {
    PENDS.fetch_add(1, Ordering::Relaxed);
}
#[embassy_executor::task(pool_size = 8)]
async fn timers() {
    Timer::at(Instant::from_ticks(0)).await; // due immediately, outside CS wake
    for _ in 0..10 {
        // Register then cancel a far-future timer; this leaves a bounded per-task entry.
        let mut cancelled = core::pin::pin!(Timer::after_ticks(10_000));
        poll_fn(|cx| {
            let _ = cancelled.as_mut().poll(cx);
            Poll::Ready(())
        })
        .await;
        Timer::after_ticks(2).await; // moves deadline earlier; no new slot
        let timed_out = with_timeout(Duration::from_ticks(1), core::future::pending::<()>()).await;
        assert!(timed_out.is_err());
    }
    DONE.fetch_add(1, Ordering::Relaxed);
}
#[test]
fn real_executor_timer_cancel_timeout_cleanup_and_allocator_detector() {
    // Deliberately prove every instrumented allocator entry detects attempts.
    COUNT.with(|c| c.set(true));
    unsafe {
        let l = Layout::from_size_align(16, 8).unwrap();
        let p = ALLOC.alloc(l);
        let p = ALLOC.realloc(p, l, 32);
        ALLOC.dealloc(p, Layout::from_size_align(32, 8).unwrap());
        let p = ALLOC.alloc_zeroed(l);
        ALLOC.dealloc(p, l);
    }
    COUNT.with(|c| c.set(false));
    assert_eq!(OPS.with(Cell::get), [1, 1, 1, 2]);
    OPS.with(|o| o.set([0; 4]));
    let executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    for _ in 0..8 {
        executor.spawner().spawn(timers()).unwrap();
    }
    let device = unsafe { am13_rs::Peripherals::from_io(FixedIo) };
    let clocks = device.clocks.sysosc_32mhz().unwrap();
    let uart = device
        .uc4
        .configure(clocks, 38400)
        .unwrap()
        .enable()
        .unwrap();
    executor.spawner().spawn(async_uart(uart)).unwrap();
    let model = Box::leak(Box::new(uart_model::Model::default()));
    let uart = model.uart();
    executor
        .spawner()
        .spawn(completion_cancel(uart, model))
        .unwrap();
    assert_eq!(Instant::now().as_ticks(), 0);
    COUNT.with(|c| c.set(true)); // includes FIRST poll, timer registration, cancellation and completed future cleanup
    for tick in 0..100 {
        if tick == 5 {
            UART_STATUS.store(64 | 1, Ordering::Relaxed);
        }
        if tick == 10 {
            UART_STATUS.store(1, Ordering::Relaxed);
        }
        if tick == 15 {
            UART_STATUS.store(0x25, Ordering::Relaxed); // RX remains busy after TX
            UART_RIS.fetch_or(1 << 12, Ordering::Relaxed);
        }
        unsafe {
            executor.poll();
            am13_rs::time::on_tick();
        }
    }
    COUNT.with(|c| c.set(false));
    assert_eq!(DONE.load(Ordering::Relaxed), 8);
    assert_eq!(UART_DONE.load(Ordering::Relaxed), 1);
    assert_eq!(CANCEL_DONE.load(Ordering::Relaxed), 1);
    assert_eq!(UART_WRITES.load(Ordering::Relaxed), 1);
    assert_eq!(Instant::now().as_ticks(), 100);
    assert_eq!(OPS.with(Cell::get), [0; 4]);
    assert!(PENDS.load(Ordering::Relaxed) > 0);
    assert!(!am13_rs::time::capacity_faulted());
}

#[derive(Clone)]
struct FixedIo;
static UART_REGS: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];
static UART_RIS: AtomicUsize = AtomicUsize::new(0);
static UART_STATUS: AtomicUsize = AtomicUsize::new(4 | 64 | 1);
static UART_WRITES: AtomicUsize = AtomicUsize::new(0);
static UART_DONE: AtomicUsize = AtomicUsize::new(0);
impl am13_rs::io::RegisterIo for FixedIo {
    fn read(&self, a: usize) -> u32 {
        match a {
            0x400b0204 => 0,
            0x400b0104 => 7 << 24,
            0x40672800 => 1,
            0x40641000 => UART_REGS[0].load(Ordering::Relaxed) as u32,
            0x40641008 => UART_REGS[1].load(Ordering::Relaxed) as u32,
            0x40641100 => UART_REGS[2].load(Ordering::Relaxed) as u32,
            0x40641110 => UART_REGS[3].load(Ordering::Relaxed) as u32,
            0x40641114 => UART_REGS[4].load(Ordering::Relaxed) as u32,
            0x40641108 => UART_STATUS.load(Ordering::Relaxed) as u32,
            0x40641124 => 0xa5,
            0x40641030 => UART_RIS.load(Ordering::Relaxed) as u32,
            _ => 0,
        }
    }
    fn write(&self, a: usize, v: u32) {
        let index = match a {
            0x40641000 => Some(0),
            0x40641008 => Some(1),
            0x40641100 => Some(2),
            0x40641110 => Some(3),
            0x40641114 => Some(4),
            _ => None,
        };
        if let Some(i) = index {
            UART_REGS[i].store(v as usize, Ordering::Relaxed);
        }
        if a == 0x40641048 {
            UART_RIS.fetch_and(!(v as usize), Ordering::Relaxed);
        }
        if a == 0x40641120 {
            assert_eq!(v, 0xa5);
            UART_WRITES.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn delay_cycles(&self, _: u32) {}
}
#[embassy_executor::task]
async fn async_uart(mut uart: am13_rs::uart::Uart<FixedIo>) {
    use embedded_io_async::{Read, Write};
    assert_eq!(uart.read(&mut []).await, Ok(0));
    assert_eq!(uart.write(&[]).await, Ok(0));
    {
        let mut cancelled = core::pin::pin!(uart.write(&[0xa5]));
        poll_fn(|cx| {
            assert!(cancelled.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    assert_eq!(UART_WRITES.load(Ordering::Relaxed), 0);
    let mut buf = [0; 4];
    assert_eq!(uart.read(&mut buf).await, Ok(1));
    assert_eq!(buf, [0xa5, 0, 0, 0]);
    // Exactly one byte is acknowledged. No hidden prefix is lost on cancellation.
    assert_eq!(uart.write(&buf).await, Ok(1));
    uart.flush().await.unwrap();
    UART_DONE.store(1, Ordering::Relaxed);
}

static CANCEL_DONE: AtomicUsize = AtomicUsize::new(0);
#[embassy_executor::task]
async fn completion_cancel(
    mut uart: am13_rs::uart::Uart<&'static uart_model::Model>,
    model: &'static uart_model::Model,
) {
    use embedded_io_async::{Read, Write};
    model.rx_busy.set(true);
    uart.flush().await.unwrap(); // untouched + receive-only BUSY
    for _ in 0..4 {
        assert_eq!(uart.write(&[0x55, 0xaa]).await, Ok(1));
        {
            let mut rejected = core::pin::pin!(uart.write(&[0xaa]));
            poll_fn(|cx| {
                assert!(rejected.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        } // cancelled write admitted no second byte
        {
            let mut cancelled = core::pin::pin!(uart.flush());
            poll_fn(|cx| {
                assert!(cancelled.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
        } // cancelled flush must not discard TX state or EOT
        model.advance(); // FIFO empty, serializer data still active
        model.advance(); // final stop bit still active
        {
            let mut repolled = core::pin::pin!(uart.flush());
            poll_fn(|cx| {
                assert!(repolled.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            model.advance(); // final stop bit completed, RX still busy
            model.ris.set(model.ris.get() | 2); // concurrent receive error
            Timer::after_ticks(1).await;
            repolled.await.unwrap();
        }
        assert_eq!(model.ris.get(), 0x1002); // polling flush consumed neither flag
        assert_eq!(
            uart.read(&mut [0]).await,
            Err(am13_rs::uart::Error::Receive)
        );
        uart.flush().await.unwrap();
    }
    assert_eq!(model.writes.get(), 4);
    assert_eq!(uart.rx_errors(), 4);
    CANCEL_DONE.store(1, Ordering::Relaxed);
}
