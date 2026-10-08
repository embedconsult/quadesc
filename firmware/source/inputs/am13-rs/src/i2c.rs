//! UC2 7-bit I²C controller at nominal 100 kHz, bounded polling.
use crate::io::{reset_power, RegisterIo};
const UC: usize = 0x4063_4000;
const I2C: usize = 0x4060_a000;
const RIS: usize = I2C + 0x30;
const ICLR: usize = I2C + 0x48;
const RX_DONE: u32 = 1;
const TX_DONE: u32 = 2;
const STOP: u32 = 1 << 9;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Power,
    Config,
    Invalid,
    Busy,
    AddressNack,
    DataNack,
    Arbitration,
    Protocol,
    Timeout,
}
pub struct Uc2<I> {
    io: I,
}
pub struct I2c<I> {
    io: I,
}
impl<I: RegisterIo> Uc2<I> {
    pub(crate) fn new(io: I) -> Self {
        Self { io }
    }
    pub fn configure(self) -> Result<I2c<I>, Error> {
        if !reset_power(&self.io, UC) {
            return Err(Error::Power);
        }
        self.io.write(UC + 0x1100, 2); // I2C controller IP mode
        self.io.write(I2C + 8, 8); // BUSCLK 32 MHz
        self.io.write(I2C, 0); // BUSCLK undivided = 32 MHz
        self.io.write(I2C + 0x104, 0);
        self.io.write(I2C + 0x110, 31); // 32 MHz/(10*(31+1))
        self.io.write(I2C + 0x104, 5); // controller, clock stretching
        if self.io.read(I2C + 0x104) & 5 != 5 || self.io.read(I2C + 0x110) != 31 {
            return Err(Error::Config);
        }
        Ok(I2c { io: self.io })
    }
}
impl<I: RegisterIo> I2c<I> {
    fn check(status: u32) -> Result<(), Error> {
        if status & 16 != 0 {
            Err(Error::Arbitration)
        } else if status & 4 != 0 {
            Err(Error::AddressNack)
        } else if status & 8 != 0 {
            Err(Error::DataNack)
        } else if status & 2 != 0 {
            Err(Error::Protocol)
        } else {
            Ok(())
        }
    }
    fn await_status(&self, mask: u32, value: u32) -> Result<(), Error> {
        for _ in 0..100_000 {
            let status = self.io.read(I2C + 0x108);
            if let Err(error) = Self::check(status) {
                self.io.write(I2C + 0x100, 0);
                return Err(error);
            }
            if status & mask == value {
                return Ok(());
            }
        }
        self.io.write(I2C + 0x100, 0);
        Err(Error::Timeout)
    }
    fn await_completion(&self, done: u32, stop: bool) -> Result<(), Error> {
        let wanted = done | if stop { STOP } else { 0 };
        for _ in 0..100_000 {
            let status = self.io.read(I2C + 0x108);
            if let Err(error) = Self::check(status) {
                self.io.write(I2C + 0x100, 0);
                return Err(error);
            }
            if self.io.read(RIS) & wanted == wanted {
                return Ok(());
            }
        }
        self.io.write(I2C + 0x100, 0);
        Err(Error::Timeout)
    }
    fn start(&self, address: u8, read: bool, len: usize, stop: bool) -> Result<(), Error> {
        if !(0x08..=0x77).contains(&address) || len == 0 || len > 4095 {
            return Err(Error::Invalid);
        }
        // Discard stale transaction flags before programming the new burst.
        self.io
            .write(ICLR, RX_DONE | TX_DONE | STOP | (1 << 7) | (1 << 10));
        self.io
            .write(I2C + 0x14c, ((address as u32) << 1) | read as u32);
        self.io.write(
            I2C + 0x100,
            ((len as u32) << 16) | 0x3 | if stop { 4 } else { 0 },
        );
        Ok(())
    }
    pub fn write(&mut self, address: u8, data: &[u8]) -> Result<(), Error> {
        self.write_part(address, data, true)
    }
    fn write_part(&mut self, address: u8, data: &[u8], stop: bool) -> Result<(), Error> {
        if data.is_empty() || data.len() > 4095 || !(0x08..=0x77).contains(&address) {
            return Err(Error::Invalid);
        }
        if self.io.read(I2C + 0x108) & 1 != 0 {
            return Err(Error::Busy);
        }
        self.io.write(I2C + 0x120, data[0] as u32);
        self.start(address, false, data.len(), stop)?;
        for &byte in &data[1..] {
            self.await_status(1 << 14, 0)?; // TX FIFO has room
            self.io.write(I2C + 0x120, byte as u32);
        }
        self.await_completion(TX_DONE, stop)
    }
    pub fn read(&mut self, address: u8, data: &mut [u8]) -> Result<(), Error> {
        if data.is_empty() {
            return Err(Error::Invalid);
        }
        if self.io.read(I2C + 0x108) & 1 != 0 {
            return Err(Error::Busy);
        }
        self.start(address, true, data.len(), true)?;
        for byte in data.iter_mut() {
            self.await_status(1 << 11, 0)?; // RX FIFO nonempty
            *byte = self.io.read(I2C + 0x124) as u8;
        }
        self.await_completion(RX_DONE, true)
    }
    /// Write command bytes, repeated START, then read. No STOP between phases.
    pub fn write_read(
        &mut self,
        address: u8,
        command: &[u8],
        data: &mut [u8],
    ) -> Result<(), Error> {
        if command.is_empty() || data.is_empty() {
            return Err(Error::Invalid);
        }
        self.write_part(address, command, false)?;
        self.read(address, data)
    }
}
