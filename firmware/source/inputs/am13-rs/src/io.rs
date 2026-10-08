//! Extracted from corrected diagnostic firmware 802aacd; see PROVENANCE.md.
use core::marker::PhantomData;
use core::sync::atomic::{AtomicUsize, Ordering};
static LAST_READ: AtomicUsize = AtomicUsize::new(0);
/// Fatal-path telemetry only; never authorizes or retries a register access.
pub fn last_read_address() -> usize {
    LAST_READ.load(Ordering::Relaxed)
}
/// Register backend for deterministic trace tests. Hardware backend is opaque.
pub trait RegisterIo: Clone {
    fn read(&self, address: usize) -> u32;
    fn write(&self, address: usize, value: u32);
    fn read16(&self, address: usize) -> u16 {
        self.read(address) as u16
    }
    /// Delay at least `cycles` CPU cycles on hardware (may take longer).
    /// Models must observe the requested count to verify settling/order.
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
        // Store before the load so a nonmaskable access fault retains its address.
        LAST_READ.store(address, Ordering::Relaxed);
        // SAFETY: only exclusive driver owners can obtain this backend.
        unsafe { core::ptr::read_volatile(address as *const u32) }
    }
    fn write(&self, address: usize, value: u32) {
        // SAFETY: drivers use documented word-aligned registers only.
        unsafe { core::ptr::write_volatile(address as *mut u32, value) }
    }
    fn read16(&self, address: usize) -> u16 {
        LAST_READ.store(address, Ordering::Relaxed);
        // SAFETY: ADC result registers are documented halfword registers.
        unsafe { core::ptr::read_volatile(address as *const u16) }
    }
    fn delay_cycles(&self, cycles: u32) {
        for _ in 0..cycles {
            // Each ARM iteration executes an observable NOP (not pure asm),
            // so LLVM cannot remove/coalesce the loop. Cortex-M33 executes at
            // least one cycle per NOP; loop overhead only lengthens the delay.
            #[cfg(target_arch = "arm")]
            unsafe {
                core::arch::asm!("nop", options(nomem, nostack, preserves_flags));
            }
            #[cfg(not(target_arch = "arm"))]
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
