//! UC4, 8N1/16x, polling primitives; no pin assignment or blocking formatter.
use crate::{
    clock::Clocks,
    io::{RegisterIo, reset_power},
};
const UC: usize = 0x4067_2000;
const UART: usize = 0x4064_1000;
const ERRORS: u32 = (1 << 17) | 0x1e;
// SPRUJF2B 29.5.2.6: raw EOT includes the final stop bit, unlike TXFE.
const EOT: u32 = 1 << 12;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Divisor {
    integer: u16,
    fractional: u8,
}
impl Divisor {
    pub fn calculate(clock: u32, baud: u32) -> Option<Self> {
        if baud == 0 {
            return None;
        }
        let scaled = (clock as u64 * 4 + baud as u64 / 2) / baud as u64;
        if !(64..=65535 * 64 + 63).contains(&scaled) {
            return None;
        }
        Some(Self {
            integer: (scaled / 64) as u16,
            fractional: (scaled % 64) as u8,
        })
    }
    pub const fn integer(self) -> u16 {
        self.integer
    }
    pub const fn fractional(self) -> u8 {
        self.fractional
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Baud,
    Power,
    Readback,
    Receive,
}
impl embedded_io::Error for Error {
    fn kind(&self) -> embedded_io::ErrorKind {
        embedded_io::ErrorKind::Other
    }
}
pub struct Uc4<I> {
    io: I,
}
pub struct DisabledUart<I> {
    io: I,
    divisor: Divisor,
}
pub struct Uart<I> {
    io: I,
    rx_errors: u32,
    tx_pending: bool,
}
impl<I: RegisterIo> Uc4<I> {
    pub(crate) fn new(io: I) -> Self {
        Self { io }
    }
    pub fn configure(self, clocks: Clocks, baud: u32) -> Result<DisabledUart<I>, Error> {
        let divisor = Divisor::calculate(clocks.uc4_hz(), baud).ok_or(Error::Baud)?;
        let io = &self.io;
        if !reset_power(io, UC) {
            return Err(Error::Power);
        }
        io.write(UC + 0x1100, 0);
        io.write(UART + 8, 8); // MCLKDIV2 source is SYSOSC32M here
        io.write(UART, 0); // /1
        io.write(UART + 0x100, 0x18);
        io.write(UART + 0x110, divisor.integer as u32);
        io.write(UART + 0x114, divisor.fractional as u32);
        io.write(UART + 0x104, 0x30);
        io.write(UART + 0x48, ERRORS);
        Ok(DisabledUart {
            io: self.io,
            divisor,
        })
    }
}
impl<I: RegisterIo> DisabledUart<I> {
    /// Board must configure UC4 TX/RX pads before enabling.
    pub fn enable(self) -> Result<Uart<I>, Error> {
        let io = &self.io;
        io.write(UART + 0x100, 0x19);
        if io.read(UART + 0x100) & 0x19 != 0x19
            || io.read(UART + 8) != 8
            || io.read(UART) != 0
            || io.read(UART + 0x110) != self.divisor.integer as u32
            || io.read(UART + 0x114) != self.divisor.fractional as u32
        {
            io.write(UART + 0x100, 0x18);
            return Err(Error::Readback);
        }
        Ok(Uart {
            io: self.io,
            rx_errors: 0,
            tx_pending: false,
        })
    }
}
impl<I: RegisterIo> Uart<I> {
    pub fn rx_errors(&self) -> u32 {
        self.rx_errors
    }
    pub fn try_read(&mut self) -> Result<Option<u8>, Error> {
        let errors = self.io.read(UART + 0x30) & ERRORS;
        if errors != 0 {
            self.io.write(UART + 0x48, errors);
            self.rx_errors = self.rx_errors.saturating_add(1);
            return Err(Error::Receive);
        }
        if self.io.read(UART + 0x108) & 4 != 0 {
            return Ok(None);
        }
        let data = self.io.read(UART + 0x124);
        if data & 0xf00 != 0 {
            self.io.write(UART + 0x48, ERRORS);
            self.rx_errors = self.rx_errors.saturating_add(1);
            return Err(Error::Receive);
        }
        Ok(Some(data as u8))
    }
    /// Admit one byte only after the previous byte's final stop bit.
    /// False admits nothing and leaves completion/error indications untouched.
    /// This intentionally does not pipeline bytes into the hardware FIFO.
    pub fn try_write(&mut self, byte: u8) -> bool {
        if self.io.read(UART + 0x108) & (1 << 6) != 0 || !self.is_tx_complete() {
            return false;
        }
        // No old TX can finish between this clear and TXDATA: the sole owner
        // has observed its EOT (or has never transmitted). Clearing after
        // TXDATA could instead lose this byte's EOT during a preemption.
        self.io.write(UART + 0x48, EOT);
        self.tx_pending = true;
        self.io.write(UART + 0x120, byte as u32);
        true
    }
    /// True when all admitted data, including its final stop bit, has left TX.
    /// An untouched transmitter is complete even while RX is busy. Polling
    /// latches completion in this owner; it never clears RX/error indications.
    pub fn is_tx_complete(&mut self) -> bool {
        if self.tx_pending && self.io.read(UART + 0x30) & EOT != 0 {
            self.tx_pending = false;
        }
        !self.tx_pending
    }
}
impl<I: RegisterIo> embedded_io::ErrorType for Uart<I> {
    type Error = Error;
}
#[cfg(feature = "embassy")]
impl<I: RegisterIo> embedded_io_async::Read for Uart<I> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if let Some(byte) = self.try_read()? {
                buf[0] = byte;
                return Ok(1);
            }
            embassy_time::Timer::after_ticks(1).await;
        }
    }
}
#[cfg(feature = "embassy")]
impl<I: RegisterIo> embedded_io_async::Write for Uart<I> {
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Error> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.try_write(buf[0]) {
                return Ok(1);
            }
            embassy_time::Timer::after_ticks(1).await;
        }
    }
    async fn flush(&mut self) -> Result<(), Error> {
        while !self.is_tx_complete() {
            embassy_time::Timer::after_ticks(1).await;
        }
        Ok(())
    }
}
