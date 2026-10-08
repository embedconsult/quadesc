//! Target link fixture for the explicit board resource path. Not a characterization app.
#![no_std]
#![no_main]
use am13_rs::Peripherals;

#[cortex_m_rt::entry]
fn main() -> ! {
    let device = Peripherals::take().unwrap_or_else(|| halt());
    let mut board =
        esc_som_board::peripherals::initialize(device, 38_400).unwrap_or_else(|_| halt());
    board
        .can_termination
        .set_enabled(false)
        .unwrap_or_else(|_| halt());
    // Configure one timer, leave its output forced low and the driver disabled.
    board.pwm[1]
        .configure_frequency(20_000)
        .unwrap_or_else(|_| halt());
    board.pwm[1]
        .set_width_ticks(0, 800)
        .unwrap_or_else(|_| halt());
    // MCAN clock can be established after choosing the termination state.
    let can = board.mcan0.configure(250_000).unwrap_or_else(|_| halt());
    let _status = can.status();
    loop {
        core::hint::spin_loop();
    }
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
