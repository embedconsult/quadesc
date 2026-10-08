#![no_std]
//! SoM wiring and resource construction, no actor state or protocol.
pub mod peripherals;
pub mod pins;
use am13_rs::{
    Peripherals,
    clock::Clocks,
    gpio::{self, Output, Pull},
    io::{Mmio, RegisterIo},
    timer::SysTick,
    uart,
};
#[derive(Debug, PartialEq, Eq)]
pub enum StartupError {
    Gpio(gpio::Error),
    Clock,
    Uart(uart::Error),
}
/// Only resource fields intended for downstream ownership are exposed.
pub struct Resources<I: RegisterIo = Mmio> {
    pub led: Led<I>,
    pub uart: uart::Uart<I>,
    pub systick: SysTick<I>,
    pub clocks: Clocks,
    /// Unique NVMNW controller handle; application policy decides admission.
    pub flash: am13_rs::flash::FlashController<I>,
}
/// Exclusive active-low LED. GPIO readback success is not optical evidence.
pub struct Led<I> {
    output: Output<I>,
}
impl<I: RegisterIo> Led<I> {
    pub fn set_on(&mut self, on: bool) -> Result<(), gpio::Error> {
        self.output.set_level(!on)
    }
    pub fn verify(&mut self, on: bool) -> Result<(), gpio::Error> {
        self.output.verify(!on)
    }
}
impl<I: RegisterIo> embedded_hal::digital::ErrorType for Led<I> {
    type Error = gpio::Error;
}
impl<I: RegisterIo> embedded_hal::digital::OutputPin for Led<I> {
    fn set_low(&mut self) -> Result<(), gpio::Error> {
        self.output.set_level(false)
    }
    fn set_high(&mut self) -> Result<(), gpio::Error> {
        self.output.set_level(true)
    }
}
/// Acquire Peripherals once at reset; this consumes all configuration capabilities.
/// `uart_baud` is checked by the chip UART configurator before UC4 programming.
/// Returns with IRQs still masked on hardware. Start SysTick/enable interrupts only
/// after the application has built resources and frozen its allocator.
pub fn initialize<I: RegisterIo>(
    device: Peripherals<I>,
    uart_baud: u32,
) -> Result<Resources<I>, StartupError> {
    let Peripherals {
        mut gpio,
        clocks,
        uc4,
        systick,
        flash,
        ..
    } = device;
    gpio.power().map_err(StartupError::Gpio)?;
    for pin in [pins::DRV_ENABLE, pins::INL_ENABLE]
        .into_iter()
        .chain(pins::MOTOR_OUTPUTS)
    {
        gpio.output(pin, false).map_err(StartupError::Gpio)?;
    }
    for pin in pins::DRIVER_CS {
        gpio.output(pin, true).map_err(StartupError::Gpio)?;
    }
    let led = Led {
        output: gpio
            .output(pins::STATUS_LED, true)
            .map_err(StartupError::Gpio)?,
    };
    for pin in pins::NFAULT {
        gpio.input(pin, Pull::None).map_err(StartupError::Gpio)?;
    }
    for pin in pins::DSHOT_INPUTS {
        gpio.input(pin, Pull::Down).map_err(StartupError::Gpio)?;
    }
    gpio.input(pins::BSL_INVOKE, Pull::Down)
        .map_err(StartupError::Gpio)?;
    let clocks = clocks.sysosc_32mhz().map_err(|_| StartupError::Clock)?;
    let uart = uc4
        .configure(clocks, uart_baud)
        .map_err(StartupError::Uart)?;
    gpio.alternate(pins::BSL_UART_TX, 7, false, Pull::None)
        .map_err(StartupError::Gpio)?;
    gpio.alternate(pins::BSL_UART_RX, 7, true, Pull::Up)
        .map_err(StartupError::Gpio)?;
    let uart = uart.enable().map_err(StartupError::Uart)?;
    // GPIO configurator is consumed here: motor/CAN/debug cannot be remuxed by services.
    Ok(Resources {
        led,
        uart,
        systick,
        clocks,
        flash,
    })
}

/// Terminal panic/fault path only. Caller masks IRQs and never resumes any owner.
/// No termination-pad write. GPIO gate inhibits precede timer force-low.
#[cfg(target_arch="arm")]
pub unsafe fn emergency_safe_off() {
 unsafe {
  core::ptr::write_volatile((pins::DRV_ENABLE.base()+0x12a0) as *mut u32,(1<<pins::DRV_ENABLE.bit())|(1<<pins::INL_ENABLE.bit()));
  for module in 0..4 {let b=0x40010000+module*0x1000;
   for unit in 0..3 {core::ptr::write_volatile((b+0x130+unit*0x200) as *mut u32,0x11);}
   let p=(b+0x10) as *mut u32;let v=core::ptr::read_volatile(p);core::ptr::write_volatile(p,(v&!3)|2);
  }
 }
}
