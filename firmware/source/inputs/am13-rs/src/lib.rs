#![no_std]
//! Chip drivers. SoM wiring and application lifecycle do not belong here.
pub mod adc;
pub mod can;
pub mod clock;
pub mod flash;
pub mod gpio;
pub mod i2c;
pub mod io;
pub mod pwm;
pub mod spi;
#[cfg(feature = "embassy")]
pub mod time;
pub mod timer;
pub mod uart;

#[cfg(target_arch = "arm")]
use core::sync::atomic::{AtomicBool, Ordering};
use io::Mmio;
#[cfg(target_arch = "arm")]
static TAKEN: AtomicBool = AtomicBool::new(false);

/// Exclusive resources, acquired once per MCU boot. No reacquisition on Drop.
pub struct Peripherals<I: io::RegisterIo = Mmio> {
    pub clocks: clock::ClockControl<I>,
    pub gpio: gpio::Gpio<I>,
    pub uc4: uart::Uc4<I>,
    pub adc: adc::Adcs<I>,
    pub uc3_spi: spi::Uc3<I>,
    pub uc2_i2c: i2c::Uc2<I>,
    pub pwm: pwm::McPwms<I>,
    pub mcan0: can::Mcan0<I>,
    pub systick: timer::SysTick<I>,
    pub flash: flash::FlashController<I>,
}
impl Peripherals<Mmio> {
    /// AM13 only. Quiesces inherited interrupts; start the timebase after startup freeze.
    #[cfg(target_arch = "arm")]
    pub fn take() -> Option<Self> {
        if TAKEN.swap(true, Ordering::AcqRel) {
            return None;
        }
        cortex_m::interrupt::disable();
        // SAFETY: singleton claim on this supported target; interrupts remain disabled.
        let io = unsafe { Mmio::new() };
        io::quiesce(&io);
        Some(unsafe { Self::from_io(io) })
    }
}
impl<I: io::RegisterIo> Peripherals<I> {
    /// Construct with an isolated register backend (normally a host model).
    /// # Safety
    /// Caller guarantees exclusive device access for this backend's lifetime.
    /// Clones must refer to the same device. No other owner/IRQ may access it.
    pub unsafe fn from_io(io: I) -> Self {
        Self {
            clocks: clock::ClockControl::new(io.clone()),
            gpio: gpio::Gpio::new(io.clone()),
            uc4: uart::Uc4::new(io.clone()),
            adc: adc::Adcs::new(io.clone()),
            uc3_spi: spi::Uc3::new(io.clone()),
            uc2_i2c: i2c::Uc2::new(io.clone()),
            pwm: pwm::McPwms::new(io.clone()),
            mcan0: can::Mcan0::new(io.clone()),
            systick: timer::SysTick::new(io.clone()),
            flash: flash::FlashController::new(io),
        }
    }
}
