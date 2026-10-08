//! Pin-level ownership and atomic GPIO latch writes; no board assignments.
use crate::io::{RegisterIo, reset_power};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Port {
    A,
    B,
    C,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pin {
    port: Port,
    bit: u8,
}
impl Pin {
    pub const fn new(port: Port, bit: u8) -> Self {
        assert!(bit < 32);
        Self { port, bit }
    }
    pub const fn port(self) -> Port {
        self.port
    }
    pub const fn bit(self) -> u8 {
        self.bit
    }
    pub const fn base(self) -> usize {
        0x400f_0000 + self.port as usize * 0x2000
    }
    pub const fn pad(self) -> usize {
        0x400c_c000 + (self.port as usize * 32 + self.bit as usize) * 4
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Power,
    AlreadyOwned,
    Readback,
    InvalidMux,
}
impl embedded_hal::digital::Error for Error {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}
#[derive(Clone, Copy)]
pub enum Pull {
    None,
    Up,
    Down,
}
impl Pull {
    fn flags(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Up => 1 << 17,
            Self::Down => 1 << 16,
        }
    }
}
pub struct Gpio<I> {
    io: I,
    claimed: [u32; 3],
    powered: bool,
    attempted: bool,
}
impl<I: RegisterIo> Gpio<I> {
    pub(crate) fn new(io: I) -> Self {
        Self {
            io,
            claimed: [0; 3],
            powered: false,
            attempted: false,
        }
    }
    pub fn power(&mut self) -> Result<(), Error> {
        if self.attempted {
            return Err(Error::AlreadyOwned);
        }
        // Mark consumed before writes: even a failed initialization cannot reset owned pads.
        self.attempted = true;
        let a = reset_power(&self.io, 0x400f_0000);
        let b = reset_power(&self.io, 0x400f_2000);
        let c = reset_power(&self.io, 0x400f_4000);
        self.powered = a && b && c;
        if self.powered {
            Ok(())
        } else {
            Err(Error::Power)
        }
    }
    fn claim(&mut self, pin: Pin) -> Result<(), Error> {
        if !self.powered {
            return Err(Error::Power);
        }
        let mask = 1 << pin.bit;
        let claimed = &mut self.claimed[pin.port as usize];
        if *claimed & mask != 0 {
            return Err(Error::AlreadyOwned);
        }
        *claimed |= mask;
        Ok(())
    }
    pub fn output(&mut self, pin: Pin, high: bool) -> Result<Output<I>, Error> {
        self.claim(pin)?;
        set(&self.io, pin, high); // latch before DOE and mux
        self.io.write(pin.base() + 0x12d0, 1 << pin.bit);
        self.io
            .write(pin.pad(), 0x40081 | if high { 1 << 17 } else { 1 << 16 });
        let mut output = Output {
            io: self.io.clone(),
            pin,
        };
        output.verify(high)?;
        Ok(output)
    }
    pub fn input(&mut self, pin: Pin, pull: Pull) -> Result<(), Error> {
        self.claim(pin)?;
        self.io.write(pin.base() + 0x12e0, 1 << pin.bit);
        self.io.write(pin.pad(), 0x40081 | pull.flags());
        Ok(())
    }
    /// Reserve a pad permanently for a peripheral; only the board chooses wiring.
    pub fn alternate(&mut self, pin: Pin, mux: u8, input: bool, pull: Pull) -> Result<(), Error> {
        if !(2..=20).contains(&mux) {
            return Err(Error::InvalidMux);
        }
        self.claim(pin)?;
        self.io.write(
            pin.pad(),
            0x80 | mux as u32 | if input { 1 << 18 } else { 0 } | pull.flags(),
        );
        Ok(())
    }
}
fn set(io: &impl RegisterIo, pin: Pin, high: bool) {
    io.write(
        pin.base() + if high { 0x1290 } else { 0x12a0 },
        1 << pin.bit,
    );
}
/// Exclusive output. Dropping it never resets/reacquires a peripheral.
pub struct Output<I> {
    io: I,
    pin: Pin,
}
impl<I: RegisterIo> Output<I> {
    pub fn set_level(&mut self, high: bool) -> Result<(), Error> {
        set(&self.io, self.pin, high);
        self.verify(high)
    }
    pub fn verify(&mut self, high: bool) -> Result<(), Error> {
        let pin = self.pin;
        let bit = 1 << pin.bit;
        if self.io.read(pin.pad()) & 0x400ff == 0x40081
            && self.io.read(pin.base() + 0x12c0) & bit != 0
            && (self.io.read(pin.base() + 0x1280) & bit != 0) == high
            && (self.io.read(pin.base() + 0x1380) & bit != 0) == high
        {
            Ok(())
        } else {
            Err(Error::Readback)
        }
    }
}
impl<I: RegisterIo> embedded_hal::digital::ErrorType for Output<I> {
    type Error = Error;
}
impl<I: RegisterIo> embedded_hal::digital::OutputPin for Output<I> {
    fn set_low(&mut self) -> Result<(), Error> {
        self.set_level(false)
    }
    fn set_high(&mut self) -> Result<(), Error> {
        self.set_level(true)
    }
}
