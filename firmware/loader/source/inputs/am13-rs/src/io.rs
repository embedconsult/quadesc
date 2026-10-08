//! Extracted from corrected diagnostic firmware 802aacd; see PROVENANCE.md.
use core::marker::PhantomData;
/// Register backend for deterministic trace tests. Hardware backend is opaque.
pub trait RegisterIo: Clone {
    fn read(&self, address: usize) -> u32;
    fn write(&self, address: usize, value: u32);
    fn delay_cycles(&self, cycles: u32);
    fn modify(&self, address: usize, clear: u32, set: u32) {
        self.write(address, (self.read(address) & !clear) | set);
    }
    fn wait(&self, address: usize, mask: u32, value: u32) -> bool {
        (0..100_000).any(|_| self.read(address) & mask == value)
    }
}
/// Opaque register accessor; cannot be constructed or obtained through safe API.
/// Remains !Send/!Sync. Drivers clone it only into disjoint peripheral/pin owners.
#[derive(Clone)]
pub struct Mmio(PhantomData<*mut ()>);
impl Mmio {
    #[cfg(target_arch = "arm")]
    pub(crate) unsafe fn new() -> Self {
        Self(PhantomData)
    }
}
impl RegisterIo for Mmio {
    fn read(&self, address: usize) -> u32 {
        // SAFETY: only exclusive driver owners can obtain this backend.
        unsafe { core::ptr::read_volatile(address as *const u32) }
    }
    fn write(&self, address: usize, value: u32) {
        // SAFETY: drivers use documented word-aligned registers only.
        unsafe { core::ptr::write_volatile(address as *mut u32, value) }
    }
    fn delay_cycles(&self, cycles: u32) {
        for _ in 0..cycles {
            core::hint::spin_loop();
        }
    }
}
pub(crate) fn reset_power(io: &impl RegisterIo, base: usize) -> bool {
    io.write(base + 0x804, 0xb100_0003);
    io.write(base + 0x800, 0x2600_0001);
    io.delay_cycles(32);
    io.read(base + 0x800) & 1 == 1
}
#[cfg(target_arch = "arm")]
pub(crate) fn quiesce(io: &impl RegisterIo) {
    io.write(0xe000_e010, 0);
    for bank in 0..8 {
        io.write(0xe000_e180 + bank * 4, u32::MAX);
        io.write(0xe000_e280 + bank * 4, u32::MAX);
    }
    io.write(0xe000_ed04, (1 << 25) | (1 << 27));
}
