#![no_std]
//! SoM wiring and resource construction, no actor state or protocol.
pub mod pins;
use am13_rs::{
    clock::Clocks,
    gpio::{self, Output, Pull},
    io::{Mmio, RegisterIo},
    timer::SysTick,
    uart, Peripherals,
};
#[derive(Debug, PartialEq, Eq)]
pub enum StartupError {
    Gpio(gpio::Error),
    Clock,
    Can(am13_rs::can::Error),
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

/// Minimal CAN profile: LED and XCP, with UART unused and gates held inactive.
pub struct CanResources<I: RegisterIo = Mmio> {
    pub led: Led<I>,
    pub uart: uart::Uart<I>,
    pub systick: SysTick<I>,
    pub clocks: Clocks,
    pub flash: am13_rs::flash::FlashController<I>,
    pub can: am13_rs::can::Can<I>,
}
pub fn initialize_xcp_can<I: RegisterIo>(
    device: Peripherals<I>,
    uart_baud: u32,
) -> Result<CanResources<I>, StartupError> {
    let Peripherals {
        mut gpio,
        clocks,
        uc4,
        systick,
        flash,
        mcan0,
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
    gpio.alternate(pins::BSL_CAN_TX, 10, false, Pull::None).map_err(StartupError::Gpio)?;
    gpio.alternate(pins::BSL_CAN_RX, 10, true, Pull::None).map_err(StartupError::Gpio)?;
    // Leave default-on termination untouched. CPU explicitly leaves boot 80 MHz PLL.
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
    let can = mcan0.configure(500_000).map_err(StartupError::Can)?;
    Ok(CanResources {
        can,
        led,
        uart,
        systick,
        clocks,
        flash,
    })
}
