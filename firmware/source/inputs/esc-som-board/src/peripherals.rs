//! Explicit board-characterization resource profile. No automatic gate enable.
use crate::{Led, pins};
use am13_rs::{
    Peripherals, adc, can,
    clock::Clocks,
    gpio::{self, Input, Output, Pull},
    i2c,
    io::{Mmio, RegisterIo},
    pwm, spi,
    timer::SysTick,
    uart,
};

#[derive(Debug)]
pub enum StartupError {
    Gpio(gpio::Error),
    Clock,
    Uart(uart::Error),
    Adc(adc::Error),
    Spi(spi::Error),
    I2c(i2c::Error),
}
pub struct Resources<I: RegisterIo = Mmio> {
    pub led: Led<I>,
    pub uart: uart::Uart<I>,
    pub systick: SysTick<I>,
    pub clocks: Clocks,
    pub flash: am13_rs::flash::FlashController<I>,
    pub analog: Analog<I>,
    pub pwm: [pwm::Pwm<I>; 4],
    pub drivers: GateDrivers<I>,
    pub i2c: i2c::I2c<I>,
    /// MCAN stays unconfigured until application chooses termination and bitrate.
    pub mcan0: can::Mcan0<I>,
}
pub struct CanTermination<I> {
    output: Output<I>,
    requested_enabled: bool,
}
impl<I: RegisterIo> CanTermination<I> {
    /// TS5A3166 IN high closes NO-COM switch. This reports requested GPIO state only.
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), gpio::Error> {
        self.output.set_level(enabled)?;
        self.requested_enabled = enabled;
        Ok(())
    }
    pub fn requested_enabled(&self) -> bool {
        self.requested_enabled
    }
}
pub struct GateDrivers<I> {
    spi: spi::Spi<I>,
    enable: Output<I>,
    inl_enable: Output<I>,
    cs: [Output<I>; 4],
    faults: [Input<I>; 4],
}
#[derive(Debug)]
pub enum DriverError {
    Index,
    Register,
    Gpio(gpio::Error),
    Spi(spi::Error),
}
impl<I: RegisterIo> GateDrivers<I> {
    /// Common DRV_ENABLE, initially low. Application is responsible for safe PWM state.
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), DriverError> {
        self.enable.set_level(enabled).map_err(DriverError::Gpio)
    }
    pub fn set_inl_enabled(&mut self, enabled: bool) -> Result<(), DriverError> {
        self.inl_enable
            .set_level(enabled)
            .map_err(DriverError::Gpio)
    }
    pub fn fault_asserted(&self, motor: usize) -> Result<bool, DriverError> {
        self.faults
            .get(motor)
            .map(|pin| !pin.is_high())
            .ok_or(DriverError::Index)
    }
    pub fn transfer(&mut self, motor: usize, word: u16) -> Result<u16, DriverError> {
        self.spi.inter_frame_gap();
        let cs = self.cs.get_mut(motor).ok_or(DriverError::Index)?;
        if let Err(error) = cs.set_level(false) {
            let _ = cs.set_level(true);
            return Err(DriverError::Gpio(error));
        }
        let result = self.spi.transfer_word(word).map_err(DriverError::Spi);
        let release = cs.set_level(true).map_err(DriverError::Gpio);
        release?;
        result
    }
    /// DRV8323 16-bit frame: R/W at B15, 4-bit address B14..B11, 11-bit data.
    pub fn read_register(&mut self, motor: usize, address: u8) -> Result<u16, DriverError> {
        if address > 0xf {
            return Err(DriverError::Register);
        }
        Ok(self.transfer(motor, 0x8000 | ((address as u16) << 11))? & 0x7ff)
    }
    /// Returns the previous register value shifted out during the write.
    pub fn write_register(
        &mut self,
        motor: usize,
        address: u8,
        data: u16,
    ) -> Result<u16, DriverError> {
        if address > 0xf || data > 0x7ff {
            return Err(DriverError::Register);
        }
        Ok(self.transfer(motor, ((address as u16) << 11) | data)? & 0x7ff)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalogInput {
    pub name: &'static str,
    pub pin: gpio::Pin,
    pub channel: adc::Channel,
}
// All 30 assigned SoM analog pads in v3. Mainboard's older 29-signal logical table
// is not substituted for these assignments; semantic net validation is still needed.
pub const ANALOG: [AnalogInput; 30] = [
    analog("PA3/A0_4", gpio::Port::A, 3, 0, 4),
    analog("PB3/A0_9", gpio::Port::B, 3, 0, 9),
    analog("PA28/A0_11", gpio::Port::A, 28, 0, 11),
    analog("PA17/A0_12", gpio::Port::A, 17, 0, 12),
    analog("PA16/A0_15", gpio::Port::A, 16, 0, 15),
    analog("PC14/A0_18", gpio::Port::C, 14, 0, 18),
    analog("PC15/A0_19", gpio::Port::C, 15, 0, 19),
    analog("PB24/A0_21", gpio::Port::B, 24, 0, 21),
    analog("PB27/A0_27", gpio::Port::B, 27, 0, 27),
    analog("PB4/A1_5", gpio::Port::B, 4, 1, 5),
    analog("PB1/A1_7", gpio::Port::B, 1, 1, 7),
    analog("PA18/A1_12", gpio::Port::A, 18, 1, 12),
    analog("PA5/A1_13", gpio::Port::A, 5, 1, 13),
    analog("PC10/A1_16", gpio::Port::C, 10, 1, 16),
    analog("PA8/A1_18", gpio::Port::A, 8, 1, 18),
    analog("PA9/A1_19", gpio::Port::A, 9, 1, 19),
    analog("PB25/A1_22", gpio::Port::B, 25, 1, 22),
    analog("PC8/A1_23", gpio::Port::C, 8, 1, 23),
    analog("PC11/A1_24", gpio::Port::C, 11, 1, 24),
    analog("PC12/A1_25", gpio::Port::C, 12, 1, 25),
    analog("PB30/A1_30", gpio::Port::B, 30, 1, 30),
    analog("PC9/A2_2", gpio::Port::C, 9, 2, 2),
    analog("PC7/A2_4", gpio::Port::C, 7, 2, 4),
    analog("PA29/A2_5", gpio::Port::A, 29, 2, 5),
    analog("PB26/A2_7", gpio::Port::B, 26, 2, 7),
    analog("PB29/A2_10", gpio::Port::B, 29, 2, 10),
    analog("PA7/A2_22", gpio::Port::A, 7, 2, 22),
    analog("PB5/A2_24", gpio::Port::B, 5, 2, 24),
    analog("PA2/A2_25", gpio::Port::A, 2, 2, 25),
    analog("PB31/A2_29", gpio::Port::B, 31, 2, 29),
];
const fn analog(
    name: &'static str,
    port: gpio::Port,
    bit: u8,
    instance: u8,
    selector: u8,
) -> AnalogInput {
    AnalogInput {
        name,
        pin: gpio::Pin::new(port, bit),
        channel: adc::Channel { instance, selector },
    }
}
pub struct Analog<I> {
    adc: adc::ReadyAdcs<I>,
}
impl<I: RegisterIo> Analog<I> {
    pub fn read(&mut self, index: usize) -> Result<adc::Reading, adc::Error> {
        let input = ANALOG.get(index).ok_or(adc::Error::InvalidChannel)?;
        self.adc.read(input.channel)
    }
    pub fn capture<F: FnMut(bool)>(&mut self, index: usize, cycles: u32, hardware: bool, trigger: F) -> Result<adc::Reading, adc::Error> {
        let input = ANALOG.get(index).ok_or(adc::Error::InvalidChannel)?;
        self.adc.capture(input.channel, cycles, hardware, trigger)
    }
    /// Sequential raw snapshot; each element retains its own channel/result status.
    pub fn scan_all(&mut self) -> [Result<adc::Reading, adc::Error>; 30] {
        core::array::from_fn(|index| self.read(index))
    }
}
/// Application opts into this profile; legacy LED/UART `initialize` remains separate.
pub fn initialize<I: RegisterIo>(
    device: Peripherals<I>,
    uart_baud: u32,
) -> Result<Resources<I>, StartupError> {
    let Peripherals {
        mut gpio,
        clocks,
        uc4,
        adc,
        uc3_spi,
        uc2_i2c,
        pwm,
        mcan0,
        systick,
        flash,
    } = device;
    gpio.power().map_err(StartupError::Gpio)?;
    let mut driver_enable = gpio
        .output(pins::DRV_ENABLE, false)
        .map_err(StartupError::Gpio)?;
    driver_enable.set_level(false).map_err(StartupError::Gpio)?;
    let inl_enable = gpio
        .output(pins::INL_ENABLE, false)
        .map_err(StartupError::Gpio)?;
    let cs = [
        gpio.output(pins::DRIVER_CS[0], true)
            .map_err(StartupError::Gpio)?,
        gpio.output(pins::DRIVER_CS[1], true)
            .map_err(StartupError::Gpio)?,
        gpio.output(pins::DRIVER_CS[2], true)
            .map_err(StartupError::Gpio)?,
        gpio.output(pins::DRIVER_CS[3], true)
            .map_err(StartupError::Gpio)?,
    ];
    let faults = [
        gpio.input_owned(pins::NFAULT[0], Pull::None)
            .map_err(StartupError::Gpio)?,
        gpio.input_owned(pins::NFAULT[1], Pull::None)
            .map_err(StartupError::Gpio)?,
        gpio.input_owned(pins::NFAULT[2], Pull::None)
            .map_err(StartupError::Gpio)?,
        gpio.input_owned(pins::NFAULT[3], Pull::None)
            .map_err(StartupError::Gpio)?,
    ];
    // Assembled SOM default-on termination: PB9 deliberately untouched.
    let led = Led {
        output: gpio
            .output(pins::STATUS_LED, true)
            .map_err(StartupError::Gpio)?,
    };
    gpio.input(pins::BSL_INVOKE, Pull::Down)
        .map_err(StartupError::Gpio)?;
    for input in ANALOG {
        gpio.analog(input.pin).map_err(StartupError::Gpio)?;
    }
    let mut pwm = pwm.split();
    for module in &mut pwm {
        module.stop();
    }
    for (pin, mux, _, _) in pins::PWM_ROUTES {
        gpio.alternate(pin, mux, false, Pull::None)
            .map_err(StartupError::Gpio)?;
    }
    gpio.alternate(pins::SPI_MOSI, 6, false, Pull::None)
        .map_err(StartupError::Gpio)?;
    gpio.alternate(pins::SPI_MISO, 6, true, Pull::Up)
        .map_err(StartupError::Gpio)?;
    gpio.alternate(pins::SPI_SCLK, 6, false, Pull::None)
        .map_err(StartupError::Gpio)?;
    gpio.alternate(pins::I2C_SDA, 4, true, Pull::None)
        .map_err(StartupError::Gpio)?;
    gpio.alternate(pins::I2C_SCL, 4, true, Pull::None)
        .map_err(StartupError::Gpio)?;
    gpio.alternate(pins::BSL_CAN_TX, 10, false, Pull::None)
        .map_err(StartupError::Gpio)?;
    gpio.alternate(pins::BSL_CAN_RX, 10, true, Pull::None)
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
    // Assembled SoM: R5/R1 DNP, internal 1.65 V VREFHI / nominal 3.3 V span.
    let adc = adc
        .initialize(adc::Reference::Internal3V3)
        .map_err(StartupError::Adc)?;
    let spi = uc3_spi.configure().map_err(StartupError::Spi)?;
    let i2c = uc2_i2c.configure().map_err(StartupError::I2c)?;
    Ok(Resources {
        led,
        uart,
        systick,
        clocks,
        flash,
        analog: Analog { adc },
        pwm,
        drivers: GateDrivers {
            spi,
            enable: driver_enable,
            inl_enable,
            cs,
            faults,
        },
        i2c,
        mcan0,
    })
}
