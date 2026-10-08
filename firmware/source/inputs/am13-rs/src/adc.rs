//! Sequential software-triggered raw ADC conversion. SDK `dl_adc`/`hw_adc`.
//! Factory trim is ROM-loaded; this module never resets an ADC.
use crate::io::RegisterIo;

/// Shared ADC reference selection; choose to match the board's VREFHI wiring.
/// Encodings follow SDK 26.01.00.03 `DL_SYSCTL_VREF_MODE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Reference {
    /// Internal 1.65 V on VREFHI, with 2x scaling: nominal 3.3 V input span.
    Internal3V3 = 0x000,
    /// Internal 2.5 V on VREFHI, without input scaling.
    Internal2V5 = 0x100,
    /// External 1.65 V on VREFHI, with 2x scaling.
    ExternalHalfScale = 0x001,
    /// External 2.5/3.3 V on VREFHI, without input scaling.
    ExternalFullScale = 0x101,
}
const ANAREFCTL: usize = 0x400b_0488;
const REFERENCE_MASK: u32 = 0x101;
// >=5 ms at MCLK <=32 MHz, even at only one cycle per loop iteration.
const SETTLE_CYCLES: u32 = 160_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    pub instance: u8,
    pub selector: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reading {
    pub channel: Channel,
    pub raw: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidChannel,
    NotReady,
    Timeout,
}
pub struct Adcs<I> {
    io: I,
}
impl<I: RegisterIo> Adcs<I> {
    pub(crate) fn new(io: I) -> Self {
        Self { io }
    }
    /// Configure at MCLK <=32 MHz; do not drive an externally supplied VREFHI
    /// with an internal reference. All three ADCs share this selection.
    pub fn initialize(self, reference: Reference) -> Result<ReadyAdcs<I>, Error> {
        // Match SDK DL_SYSCTL_setVREF: direct write, no ADC reset or trim reload.
        self.io.write(ANAREFCTL, reference as u32);
        if self.io.read(ANAREFCTL) & REFERENCE_MASK != reference as u32 {
            return Err(Error::NotReady);
        }
        for instance in 0..3 {
            let b = base(instance);
            self.io.write(b + 0x800, 0x2600_0001);
            self.io.delay_cycles(32);
            if self.io.read(b + 0x800) & 1 == 0 {
                return Err(Error::NotReady);
            }
            for seq in 0..4 {
                self.io.write(b + 0x1324 + seq * 4, 0);
            }
            if !self.io.wait(b + 0x1000, 1 << 13, 0) {
                return Err(Error::Timeout);
            }
            self.io.write(b + 0x1004, 2); // MCLK/2, 16 MHz ADCCLK
            self.io.write(b + 0x1010, 0x80); // INT1 from EOC0
            self.io.write(b + 0x1014, 0); // DMA disabled
            self.io.write(b + 0x1320, 0); // SEQ4 SOC0 only
            self.io.write(b + 0x1330, 0x8000_00bf); // 448-cycle sample window
            self.io.write(b + 0x1024, 0x000f_000f);
            self.io.write(b + 0x102c, 0x000f_000f);
            self.io.modify(b + 0x1000, 0, 0x84);
            if self.io.read(b + 0x1000) & 0x84 != 0x84 {
                return Err(Error::NotReady);
            }
        }
        // Start the entire settling interval after reference selection AND the
        // last core power-up/readback, before publishing a conversion-capable owner.
        self.io.delay_cycles(SETTLE_CYCLES);
        Ok(ReadyAdcs { io: self.io })
    }
}
pub struct ReadyAdcs<I> {
    io: I,
}
impl<I: RegisterIo> ReadyAdcs<I> {
    /// Diagnostic ownership is synchronous and exclusive. Always detach hardware
    /// trigger and restore legacy SEQ4 before returning, including timeout paths.
    /// 128/256/448 MCLK cycles = 4/8/14 us. No ADC reset or trim write.
    pub fn capture<F: FnMut(bool)>(
        &mut self,
        channel: Channel,
        cycles: u32,
        hardware: bool,
        mut trigger: F,
    ) -> Result<Reading, Error> {
        let acq = match cycles {
            128 => 0x5f,
            256 => 0x8f,
            448 => 0xbf,
            _ => return Err(Error::InvalidChannel),
        };
        if channel.instance >= 3 || channel.selector >= 32 {
            return Err(Error::InvalidChannel);
        }
        let b = base(channel.instance);
        let bounded = |a, mask, value| (0..1024).any(|_| self.io.read(a) & mask == value);
        trigger(false);
        self.io.write(b + 0x1330, 0);
        let result = (|| {
            if !bounded(b + 0x1000, 1 << 13, 0) {
                return Err(Error::Timeout);
            }
            self.io.write(b + 0x104c, (channel.selector as u32) << 15);
            self.io.write(b + 0x1024, 1);
            self.io.write(b + 0x102c, 1);
            if !bounded(b + 0x101c, 0x101, 0) {
                return Err(Error::Timeout);
            }
            let config = 0x8000_0000 | acq | if hardware { 6 << 20 } else { 0 };
            self.io.write(b + 0x1330, config);
            if self.io.read(b + 0x1330) != config {
                return Err(Error::NotReady);
            }
            if hardware {
                trigger(true);
            } else {
                self.io.write(b + 0x1330, config | (1 << 30));
            }
            if !bounded(b + 0x101c, 0x100, 0x100) {
                return Err(Error::Timeout);
            }
            trigger(false);
            if self.io.read(b + 0x1028) & 0x101 != 0 {
                return Err(Error::Timeout);
            }
            Ok(Reading {
                channel,
                raw: self
                    .io
                    .read16(0x4000_a000 + channel.instance as usize * 0x1000)
                    & 0xfff,
            })
        })();
        trigger(false);
        self.io.write(b + 0x1330, 0);
        let idle = bounded(b + 0x1000, 1 << 13, 0);
        // An active conversion on a timeout is never reconfigured. Legacy read
        // checks idle first and installs its own configuration on the next call.
        if idle {
            self.io.write(b + 0x1330, 0x8000_00bf);
        }
        self.io.write(b + 0x1024, 1);
        self.io.write(b + 0x102c, 1);
        if !idle { Err(Error::Timeout) } else { result }
    }
    pub fn read(&mut self, channel: Channel) -> Result<Reading, Error> {
        if channel.instance >= 3 || channel.selector >= 32 {
            return Err(Error::InvalidChannel);
        }
        let b = base(channel.instance);
        if !self.io.wait(b + 0x1000, 1 << 13, 0) {
            return Err(Error::Timeout);
        }
        self.io.write(b + 0x104c, (channel.selector as u32) << 15);
        self.io.write(b + 0x1024, 1);
        self.io.write(b + 0x102c, 1);
        if !self.io.wait(b + 0x101c, 0x101, 0) {
            return Err(Error::Timeout);
        }
        self.io.write(b + 0x1330, 0xc000_00bf);
        if !self.io.wait(b + 0x101c, 0x100, 0x100) {
            return Err(Error::Timeout);
        }
        let raw = self
            .io
            .read16(0x4000_a000 + channel.instance as usize * 0x1000)
            & 0xfff;
        self.io.write(b + 0x1024, 1);
        Ok(Reading { channel, raw })
    }
}
const fn base(instance: u8) -> usize {
    0x4000_0000 + instance as usize * 0x2000
}
