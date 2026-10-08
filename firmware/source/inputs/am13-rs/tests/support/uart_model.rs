//! Fixed storage UART model: buffer -> data -> stop -> EOT, W1C raw flags.
//! Addresses/semantics independently transcribed from SPRUJF2B ch29 and SDK.
#![allow(dead_code)]
use am13_rs::{Peripherals, io::RegisterIo, uart::Uart};
use std::cell::Cell;
#[derive(Default)]
pub struct Model {
    regs: [Cell<u32>; 5],
    pub phase: Cell<u8>, // 0 idle, 1 holding buffer, 2 shifting data, 3 stop bit
    pub ris: Cell<u32>,
    pub rx_busy: Cell<bool>,
    pub full: Cell<bool>,
    pub writes: Cell<usize>,
    pub clears: Cell<usize>,
    pub accesses: Cell<usize>,
    pub finish_on_access: Cell<usize>,
    pub complete_in_write: Cell<bool>,
}
impl Model {
    pub fn uart(&self) -> Uart<&Self> {
        let p = unsafe { Peripherals::from_io(self) };
        let c = p.clocks.sysosc_32mhz().unwrap();
        p.uc4.configure(c, 38400).unwrap().enable().unwrap()
    }
    pub fn advance(&self) {
        match self.phase.get() {
            1 => self.phase.set(2),
            2 => self.phase.set(3),
            3 => {
                self.phase.set(0);
                self.ris.set(self.ris.get() | 0x1000);
            }
            _ => panic!("no active transmitter"),
        }
    }
    pub fn finish(&self) {
        while self.phase.get() != 0 {
            self.advance();
        }
    }
    fn access(&self) {
        let n = self.accesses.get() + 1;
        self.accesses.set(n);
        if n == self.finish_on_access.get() {
            self.finish();
        }
    }
}
fn index(a: usize) -> Option<usize> {
    [0x40641000, 0x40641008, 0x40641100, 0x40641110, 0x40641114]
        .iter()
        .position(|x| *x == a)
}
impl RegisterIo for &Model {
    fn read(&self, a: usize) -> u32 {
        self.access();
        if let Some(i) = index(a) {
            return self.regs[i].get();
        }
        match a {
            0x400b0104 => 7 << 24,
            0x40672800 => 1,
            0x40641108 => {
                u32::from(self.rx_busy.get() || self.phase.get() != 0)
                | 4 // RXFE; RX shifting does not imply a buffered character
                | if self.phase.get() == 1 { 0 } else { 0x20 }
                | if self.full.get() { 0x40 } else { 0 }
            }
            0x40641030 => self.ris.get(),
            _ => 0,
        }
    }
    fn write(&self, a: usize, v: u32) {
        self.access();
        if let Some(i) = index(a) {
            self.regs[i].set(v);
        }
        match a {
            0x40641048 => {
                // Clearing EOT with an outstanding serializer would introduce
                // a race. The admitted-byte invariant must prevent this.
                if v & 0x1000 != 0 {
                    assert_eq!(self.phase.get(), 0);
                    self.clears.set(self.clears.get() + 1);
                }
                self.ris.set(self.ris.get() & !v);
            }
            0x40641120 => {
                assert!(!self.full.get());
                assert_eq!(self.phase.get(), 0); // no pipelining under this policy
                assert_eq!(self.ris.get() & 0x1000, 0); // stale EOT cleared
                self.phase.set(1);
                self.writes.set(self.writes.get() + 1);
                if self.complete_in_write.get() {
                    self.finish();
                }
            }
            _ => {}
        }
    }
    fn delay_cycles(&self, _: u32) {}
}
