//! Fixed table arithmetic independent of Embassy and MMIO.
use crate::{clock::Clocks, io::RegisterIo};
pub const TICK_HZ: u32 = 10_000;
pub const TICK_US: u64 = 100;
pub fn ticks_to_micros(ticks: u64) -> Option<u64> {
    ticks.checked_mul(TICK_US)
}
pub fn micros_to_ticks_ceil(us: u64) -> u64 {
    us / TICK_US + (!us.is_multiple_of(TICK_US)) as u64
}
#[derive(Debug, PartialEq, Eq)]
pub struct CapacityError;
/// One earliest outstanding timer per key. No heap, key Drop or waker callbacks.
pub struct Deadlines<K: Copy + PartialEq, const N: usize> {
    slots: [Option<(K, u64)>; N],
}
impl<K: Copy + PartialEq, const N: usize> Default for Deadlines<K, N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<K: Copy + PartialEq, const N: usize> Deadlines<K, N> {
    pub const fn new() -> Self {
        Self { slots: [None; N] }
    }
    pub fn schedule(&mut self, key: K, at: u64) -> Result<(), CapacityError> {
        for (k, deadline) in self.slots.iter_mut().flatten() {
            if *k == key {
                *deadline = (*deadline).min(at);
                return Ok(());
            }
        }
        let slot = self
            .slots
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(CapacityError)?;
        *slot = Some((key, at));
        Ok(())
    }
    pub fn expire(&mut self, now: u64) -> [Option<K>; N] {
        let mut ready = [None; N];
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if let Some((key, at)) = *slot
                && at <= now
            {
                ready[index] = Some(key);
                *slot = None;
            }
        }
        ready
    }
}
pub struct SysTick<I> {
    io: I,
}
impl<I: RegisterIo> SysTick<I> {
    pub(crate) fn new(io: I) -> Self {
        Self { io }
    }
    /// Consume the core timer once. Caller must install exactly one handler.
    pub fn start(self, clocks: Clocks) {
        self.io.write(0xe000_e010, 0);
        self.io.write(0xe000_e014, clocks.mclk_hz() / TICK_HZ - 1);
        self.io.write(0xe000_e018, 0);
        self.io.write(0xe000_ed04, 1 << 25); // discard inherited pending SysTick
        self.io.write(0xe000_e010, 7); // processor clock + IRQ + enable
    }
}

/// Coherent software counter; only accessed while holding the driver's CS.
#[cfg(any(feature = "embassy", test))]
pub(crate) struct Monotonic(u64);
#[cfg(any(feature = "embassy", test))]
impl Monotonic {
    pub(crate) const fn new() -> Self {
        Self(0)
    }
    pub(crate) fn now(&self) -> u64 {
        self.0
    }
    pub(crate) fn tick(&mut self) {
        self.0 = self.0.saturating_add(1);
    }
}
#[cfg(test)]
mod tests {
    use super::Monotonic;
    #[test]
    fn monotonic_carries_and_never_wraps_backwards() {
        let mut zero = Monotonic::new();
        zero.tick();
        assert_eq!(zero.now(), 1);
        let mut clock = Monotonic(u32::MAX as u64);
        clock.tick();
        assert_eq!(clock.now(), 1u64 << 32);
        let mut clock = Monotonic(u64::MAX - 1);
        clock.tick();
        clock.tick();
        assert_eq!(clock.now(), u64::MAX);
    }
}
