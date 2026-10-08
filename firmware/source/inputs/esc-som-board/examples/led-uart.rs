//! Platform qualification fixture only. No XCP, calibration actor or diagnostic monitor.
#![no_std]
#![no_main]
use am13_rs::{Peripherals, io::Mmio, uart::Uart};
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker};
use esc_som_board::Led;

#[embassy_executor::task]
async fn led_task(mut led: Led<Mmio>) {
    let mut ticker = Ticker::every(Duration::from_millis(500));
    let mut on = false;
    loop {
        ticker.next().await;
        on = !on;
        if led.set_on(on).is_err() {
            halt();
        }
    }
}
#[embassy_executor::task]
async fn uart_task(mut uart: Uart<Mmio>) {
    use embedded_io_async::{Read, Write};
    // One owned byte survives TX waits. This fixture requires paced host traffic.
    let mut command = [0; 1];
    loop {
        match uart.read(&mut command).await {
            Ok(1) => {
                // Exercise final-stop-bit completion in the linked fixture too.
                if uart.write_all(&command).await.is_err() || uart.flush().await.is_err() {
                    halt();
                }
            }
            Ok(_) => (),
            Err(_) => (), // Transport owner would report/reset its parser here.
        }
        // Bound work even when receive errors persist.
        embassy_time::Timer::after_ticks(1).await;
    }
}
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let device = Peripherals::take().unwrap_or_else(|| halt());
    let board = esc_som_board::initialize(device, 38_400).unwrap_or_else(|_| halt());
    spawner.spawn(led_task(board.led)).unwrap();
    spawner.spawn(uart_task(board.uart)).unwrap();
    // This fixture has no allocator. Production must freeze its startup heap here,
    // before these tasks first poll or IRQ-driven application work begins.
    board.systick.start(board.clocks);
    unsafe { cortex_m::interrupt::enable() };
    core::future::pending::<()>().await;
}
fn halt() -> ! {
    cortex_m::interrupt::disable();
    loop {
        core::hint::spin_loop();
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    halt()
}
#[cortex_m_rt::exception]
unsafe fn HardFault(_: &cortex_m_rt::ExceptionFrame) -> ! {
    halt()
}
#[cortex_m_rt::exception]
unsafe fn DefaultHandler(_: i16) -> ! {
    halt()
}
