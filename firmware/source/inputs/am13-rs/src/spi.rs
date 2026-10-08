//! UC3 external SPI controller. CS is a separate board GPIO.
use crate::io::{RegisterIo, reset_power};
const UC: usize = 0x4067_0000;
const SPI: usize = 0x4065_8000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Power,
    Timeout,
    Config,
}
pub struct Uc3<I> {
    io: I,
}
pub struct Spi<I> {
    io: I,
}
impl<I: RegisterIo> Uc3<I> {
    pub(crate) fn new(io: I) -> Self {
        Self { io }
    }
    /// DRV8323 mode 1, MSB first, 16 bits, nominal 100 kHz from 32 MHz MCLK.
    pub fn configure(self) -> Result<Spi<I>, Error> {
        if !reset_power(&self.io, UC) {
            return Err(Error::Power);
        }
        self.io.write(UC + 0x1100, 1); // SPI IP mode
        self.io.write(SPI + 8, 8); // BUSCLK 32 MHz
        self.io.write(SPI, 0); // BUSCLK undivided = 32 MHz
        self.io.write(SPI + 0x100, 0x20f); // 16-bit mode 1
        self.io.write(SPI + 0x110, 159); // 32 MHz / (2 * 160)
        self.io.write(SPI + 0x14c, 0x14); // controller, disabled
        self.io.write(SPI + 0x14c, 0x15);
        if self.io.read(SPI + 0x100) & 0x3ff != 0x20f || self.io.read(SPI + 0x14c) & 0x15 != 0x15 {
            return Err(Error::Config);
        }
        Ok(Spi { io: self.io })
    }
}
impl<I: RegisterIo> Spi<I> {
    /// Wait at least 1 us at 32 MHz between board chip-select assertions.
    pub fn inter_frame_gap(&self) { self.io.delay_cycles(32); }
    /// A single full-duplex frame. Caller asserts exactly one chip select.
    pub fn transfer_word(&mut self, word: u16) -> Result<u16, Error> {
        if !self.io.wait(SPI + 0x108, 0x100, 0) {
            return Err(Error::Timeout);
        }
        // Drain stale receive frame before a new transfer.
        if self.io.read(SPI + 0x108) & 4 == 0 {
            let _ = self.io.read(SPI + 0x124);
        }
        if !self.io.wait(SPI + 0x108, 1 << 6, 0) {
            return Err(Error::Timeout);
        }
        self.io.write(SPI + 0x120, word as u32);
        if !self.io.wait(SPI + 0x108, 4, 0) {
            return Err(Error::Timeout);
        }
        let reply = self.io.read(SPI + 0x124) as u16;
        if !self.io.wait(SPI + 0x108, 0x100, 0) {
            return Err(Error::Timeout);
        }
        Ok(reply)
    }
}
