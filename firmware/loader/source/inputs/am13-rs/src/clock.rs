//! SYSOSC selection preserved from corrected 802aacd; TI SPRUJF2B §3.4.1.
use crate::io::RegisterIo;
const SYSCTL: usize = 0x400a_f000;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clocks {
    reset_cause: u8,
}
impl Clocks {
    pub const fn mclk_hz(self) -> u32 {
        32_000_000
    }
    /// MCLKDIV2 domain divider is bypassed on SYSOSC. It is NOT 16 MHz.
    pub const fn uc4_hz(self) -> u32 {
        32_000_000
    }
    pub const fn reset_cause(self) -> u8 {
        self.reset_cause
    }
}
#[derive(Debug, PartialEq, Eq)]
pub struct ClockError;
pub struct ClockControl<I> {
    io: I,
}
impl<I: RegisterIo> ClockControl<I> {
    pub(crate) fn new(io: I) -> Self {
        Self { io }
    }
    pub fn sysosc_32mhz(self) -> Result<Clocks, ClockError> {
        let io = &self.io;
        let reset_cause = (io.read(SYSCTL + 0x1220) & 0x1f) as u8;
        io.write(SYSCTL + 0x1200, 0);
        io.modify(SYSCTL + 0x1104, 1 << 16, 0);
        if !io.wait(SYSCTL + 0x1204, 1 << 4, 0) {
            return Err(ClockError);
        }
        io.modify(SYSCTL + 0x1100, 3, 0);
        io.modify(SYSCTL + 0x1104, 7 << 24, 7 << 24);
        if !io.wait(SYSCTL + 0x1204, 0x13, 0)
            || io.read(SYSCTL + 0x1104) & ((7 << 24) | (1 << 16)) != 7 << 24
        {
            return Err(ClockError);
        }
        Ok(Clocks { reset_cause })
    }
}
