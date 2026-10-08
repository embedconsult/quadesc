//! MCPWM0–3 continuous bench pulse outputs. TI SDK `dl_mcpwm`/`hw_mcpwm`.
//! One timer period per three A/B channel pairs. Stop forces every pin low.
use crate::io::RegisterIo;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidModule,
    InvalidOutput,
    InvalidFrequency,
    InvalidWidth,
    Busy,
    Config,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effective {
    pub clock_hz: u32,
    pub divider: u32,
    pub period_ticks: u32,
    pub frequency_hz: u32,
    pub high_ticks: u32,
}
pub struct McPwms<I> {
    io: I,
}
pub struct Pwm<I> {
    io: I,
    module: u8,
    period_ticks: u32,
    divider: u32,
    shot: Option<u8>,
    widths: [u32; 6],
}
impl<I: RegisterIo> McPwms<I> {
    pub(crate) fn new(io: I) -> Self {
        Self { io }
    }
    /// Split the exclusive four-module resource without cloning it publicly.
    pub fn split(self) -> [Pwm<I>; 4] {
        // These motor-control IPs use SYSCTL power/reset, not per-IP GPRCM.
        self.io.write(0x400a_f000 + 0x142c, 0xb100_01e0); // clear reset sticky
        self.io.write(0x400a_f000 + 0x1428, 0xb100_01e0);
        self.io.delay_cycles(32);
        self.io.modify(0x400a_f000 + 0x1424, 0xff00_0000, 0x2600_01e0);
        self.io.delay_cycles(32);
        core::array::from_fn(|module| Pwm {
            io: self.io.clone(),
            module: module as u8,
            period_ticks: 0,
            divider: 1,
            shot: None,
            widths: [0; 6],
        })
    }
}
impl<I: RegisterIo> Pwm<I> {
    fn base(&self) -> usize {
        0x4001_0000 + self.module as usize * 0x1000
    }
    /// 32 MHz SYSOSC/MCLK. Shared period changes all six outputs in this module.
    pub fn configure_frequency(&mut self, hz: u32) -> Result<Effective, Error> {
        let (divider, ticks) = quantize(hz)?;
        if self.io.read(0x400a_f000 + 0x1424) & (1 << (5 + self.module)) == 0 {
            return Err(Error::Config);
        }
        self.stop();
        let b = self.base();
        self.io.modify(b + 0x10, 0x3, 2); // stop/freeze before reprogramming
        self.io.modify(
            b + 0x10,
            0x3c | 0x100,
            (divider.trailing_zeros() << 2) | 0x100,
        ); // active period
        self.io.modify(0x400a_f000 + 0x148c, 0, 1);
        if self.io.read(0x400a_f000 + 0x148c) & 1 == 0 { return Err(Error::Config); }
        // Explicitly initialize inherited state; no shadow loads or deadband surprises.
        self.io.write(b + 0x30, 0x0f0f_0f0f); // all compare loads frozen
        self.io.write(b + 0x50, 0x000f_0f0f); // all AQ loads frozen
        self.io.write(b + 0xf0, 0); // global shadow load disabled
        self.io.write(b + 0xc0, 0); // deadband bypass
        self.io.write(b + 0xa0, 0); // no external trip routing in bench profile
        self.io.write(b + 0x90, 0); // no interrupts
        self.io.write(b + 0x60, 0); // no ADC triggers
        self.io.write(b + 0x14, ticks - 1);
        self.io.write(b + 0x28, 0);
        for unit in 0..3 {
            let q = b + 0x120 + unit * 0x200;
            self.io.write(q, 0x12); // A: set at zero, clear at CMPA on up count
            self.io.write(q + 8, 0x102); // B: set at zero, clear at CMPB on up count
            self.io.write(q + 0x10, 0x55); // software one-time LOW action
            self.io.write(q + 0x14, 0x11); // trigger both AQ latches LOW
            self.io.delay_cycles(8);
            self.io.write(q + 0x10, 0x11); // continuous force LOW until explicit start
        }
        if self.io.read(b + 0x14) != ticks - 1 || self.io.read(b + 0x10) & 3 != 2 {
            self.stop(); return Err(Error::Config);
        }
        self.widths = [0; 6];
        self.period_ticks = ticks;
        self.divider = divider;
        Ok(Effective {
            clock_hz: 32_000_000,
            divider,
            period_ticks: ticks,
            frequency_hz: 32_000_000 / divider / ticks,
            high_ticks: 0,
        })
    }
    /// Output index 0..5 corresponds to 1A,1B,2A,2B,3A,3B.
    pub fn set_width_ticks(&mut self, output: u8, high_ticks: u32) -> Result<Effective, Error> {
        if output >= 6 {
            return Err(Error::InvalidOutput);
        }
        if self.period_ticks == 0 || high_ticks > self.period_ticks {
            return Err(Error::InvalidWidth);
        }
        let b = self.base();
        let unit = output as usize / 2;
        let is_b = output & 1 != 0;
        self.io.write(
            b + 0x100 + unit * 0x200 + if is_b { 8 } else { 0 },
            high_ticks,
        );
        self.widths[output as usize] = high_ticks;
        Ok(Effective {
            clock_hz: 32_000_000,
            divider: self.divider,
            period_ticks: self.period_ticks,
            frequency_hz: 32_000_000 / self.divider / self.period_ticks,
            high_ticks,
        })
    }
    /// Set duty in 0.01% steps (0..=10_000), rounded to the timer tick.
    pub fn set_duty_permyriad(&mut self, output: u8, duty: u16) -> Result<Effective, Error> {
        if duty > 10_000 || self.period_ticks == 0 {
            return Err(Error::InvalidWidth);
        }
        let ticks = (self.period_ticks * duty as u32 + 5_000) / 10_000;
        self.set_width_ticks(output, ticks)
    }
    /// Set pulse high width in ns, rounded to the timer tick.
    pub fn set_width_ns(&mut self, output: u8, width_ns: u32) -> Result<Effective, Error> {
        if self.period_ticks == 0 {
            return Err(Error::InvalidWidth);
        }
        let divisor = self.divider as u64 * 1_000_000_000;
        let ticks = ((width_ns as u64 * 32_000_000 + divisor / 2) / divisor) as u32;
        self.set_width_ticks(output, ticks)
    }
    pub fn start(&mut self) -> Result<(), Error> {
        if self.period_ticks == 0 {
            return Err(Error::InvalidFrequency);
        }
        self.io.modify(0x400a_f000 + 0x148c, 0, 1); // SYSCTL.PERCLKCR.TBCLKSYNC
        if self.io.read(0x400a_f000 + 0x148c) & 1 == 0 { self.stop(); return Err(Error::Config); }
        for output in 0..6 {
            let high = self.widths[output];
            let force = if high == 0 { 1 } else if high == self.period_ticks { 2 } else { 0 };
            let shift = (output & 1) * 4;
            self.io.modify(self.base() + 0x130 + (output / 2) * 0x200, 7 << shift, force << shift);
        }
        self.io.modify(self.base() + 0x10, 3, 0); // up count
        Ok(())
    }
    /// Hardware finite pulse: first ZERO sets, compare clears, PERIOD installs
    /// a clear-only AQ shadow. Subsequent wraps cannot raise the pin even if
    /// software never polls. TI SPRUJF2B 26.6.4; SDK AQ_LOAD_ON_CNTR_PERIOD=1.
    /// This reserves all six outputs of this module. Completion is observed
    /// from hardware AQ active readback, never inferred from a software delay.
    pub fn single_shot_ns(&mut self, output: u8, width_ns: u32) -> Result<Effective, Error> {
        if self.shot.is_some() { return Err(Error::Busy); }
        if output >= 6 { return Err(Error::InvalidOutput); }
        let mut selected = None;
        for exponent in 0..=15 {
            let divider = 1u32 << exponent;
            let ticks = (width_ns as u64 * 32_000_000 + divider as u64 * 500_000_000)
                / (divider as u64 * 1_000_000_000);
            if (1..=65_533).contains(&ticks) { selected = Some((divider, ticks as u32)); break; }
        }
        let (divider, high) = selected.ok_or(Error::InvalidWidth)?;
        self.configure_frequency(1000)?; // deterministic safe module initialization
        let b = self.base();
        self.divider = divider;
        self.period_ticks = high + 2;
        self.io.modify(b + 0x10, 0x3c, divider.trailing_zeros() << 2);
        self.io.write(b + 0x14, high + 1);
        let unit = output as usize / 2;
        let side = (output as usize & 1) * 8;
        let aq = b + 0x120 + unit * 0x200 + side;
        let clear = if output & 1 == 0 { 0x10 } else { 0x100 };
        self.io.write(aq, 2 | 4 | clear); // SET at zero; CLEAR at compare and period
        self.io.write(aq + 4, 1 | 4 | clear); // shadow NEVER sets
        let shift = unit * 8 + (output as usize & 1) * 2;
        self.io.modify(b + 0x50, 3 << shift, 1 << shift); // load at period only
        let effective = self.set_width_ticks(output, high)?;
        self.io.write(b + 0x28, 0);
        self.shot = Some(output);
        self.start()?;
        Ok(effective)
    }
    pub fn poll_single_shot(&mut self) -> bool {
        let Some(output) = self.shot else { return false; };
        let aq = self.base() + 0x120 + (output as usize / 2) * 0x200 + (output as usize & 1) * 8;
        if self.io.read(aq) & 3 == 1 { self.stop(); true } else { false }
    }
    /// TRM 26.4.4: one common TBCLKSYNC gate, identical dividers and
    /// writable frozen TBCTR. Caller must inhibit all physical driver inputs.
    /// Clear-only AQ runs for two observed wraps before force is released.
    /// Counter lags are module order PWM0=M3,1=M1,2=M4,3=M2.
    pub fn quadrature_prepare(all: &mut [Self;4]) -> Result<[u32;4], Error> {
        let io=all[0].io.clone();
        let result=(|| {
            for (m,p) in all.iter_mut().enumerate() {
                p.stop();
                if p.module as usize!=m || p.period_ticks!=640 || p.divider!=1
                    || p.widths.iter().any(|w|*w!=0 && *w!=320) {return Err(Error::Config);}
                for unit in 0..3 {
                    io.write(p.base()+0x120+unit*0x200,0x11); // zero/compare clear
                    io.write(p.base()+0x128+unit*0x200,0x101);
                }
                io.modify(p.base()+0x10,3,0);
            }
            io.modify(0x400b048c,0,1);
            // Observe actual timebase progress; no software timing approximation.
            // Each module gets two wraps, with outputs continuously forced low.
            for p in all.iter() {
                let mut prior=p.counter(); let mut wraps=0;
                for _ in 0..8192 {
                    let now=p.counter(); if now<prior {wraps+=1;} prior=now;
                    if wraps==2 {break;}
                }
                if wraps!=2 {return Err(Error::Config);}
            }
            io.modify(0x400b048c,1,0);
            if io.read(0x400b048c)&1!=0 {return Err(Error::Config);}
            let preload=[319,639,159,479];
            for (p,count) in all.iter_mut().zip(preload) {
                io.write(p.base()+0x28,count);
                // No phase reload or external sync may perturb the preload.
                io.modify(p.base()+0x10,1<<10,0);
                for unit in 0..3 {
                    io.write(p.base()+0x120+unit*0x200,0x12);
                    io.write(p.base()+0x128+unit*0x200,0x102);
                }
                if p.counter()!=count || io.read(p.base()+0x14)!=639
                    || io.read(p.base()+0x10)&0x43f!=0 {return Err(Error::Config);}
                for output in 0..6 {
                    let shift=(output&1)*4;
                    io.modify(p.base()+0x130+(output/2)*0x200,7<<shift,
                        if p.widths[output]==0 {1<<shift}else{0});
                }
            }
            Ok(preload)
        })();
        if result.is_err() {for p in all.iter_mut(){p.stop();}io.modify(0x400b048c,0,1);}
        result
    }
    /// Caller enables input gate while counters are frozen and AQ latches LOW.
    /// First rising events follow next zeros at1,161,321,481 TBCLKs for M1..M4.
    pub fn quadrature_release(&mut self) -> Result<(),Error> {
        self.io.modify(0x400b048c,0,1);
        if self.io.read(0x400b048c)&1==0 {return Err(Error::Config);}
        Ok(())
    }
    /// Independent CMPC does not alter A/B compare or output actions.
    pub fn adc_trigger_configure(&mut self, offset: u32) -> Result<(), Error> {
        self.adc_trigger_enable(false);
        if self.module != 0 || self.divider != 1 || ![640,1600].contains(&self.period_ticks)
            || offset >= self.period_ticks || self.io.read(self.base()+0x10)&3 != 0
            || self.io.read(self.base()) & 4 == 0 {return Err(Error::Config);}
        let b=self.base();
        self.io.modify(b+0x30, 3<<24, 3<<24); // freeze CMPC shadow loads
        self.io.write(b+0x40,offset);
        self.io.write(b+0x64,8); // SOCA on up-count CMPC
        self.io.write(b+0x68,1); // every event
        self.io.write(b+0x74,15);
        if self.io.read(b+0x40)!=offset || self.io.read(b+0x64)!=8 || self.io.read(b+0x68)!=1 {return Err(Error::Config);}
        Ok(())
    }
    pub fn adc_trigger_enable(&mut self, on: bool) {self.io.write(self.base()+0x60,on as u32);}
    pub fn adc_trigger_flags(&self) -> u32 {self.io.read(self.base()+0x70)}
    pub fn counter(&self) -> u32 {self.io.read(self.base()+0x28)}
    pub fn stop(&mut self) {
        self.adc_trigger_enable(false);
        self.shot = None;
        let b = self.base();
        for unit in 0..3 {
            self.io.write(b + 0x130 + unit * 0x200, 0x11);
        }
        self.io.modify(b + 0x10, 3, 2);
    }
}
fn quantize(hz: u32) -> Result<(u32, u32), Error> {
    if hz == 0 || hz > 16_000_000 {
        return Err(Error::InvalidFrequency);
    }
    for exponent in 0..=15 {
        let divider = 1u32 << exponent;
        let ticks = ((32_000_000u64 + (hz as u64 * divider as u64 / 2))
            / (hz as u64 * divider as u64)) as u32;
        if (2..=65_536).contains(&ticks) {
            return Ok((divider, ticks));
        }
    }
    Err(Error::InvalidFrequency)
}
