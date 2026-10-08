// Copyright 2025 Bloxide, all rights reserved
#![no_std]
//! Minimal ownership seam fixture, not the LED reservation/calibration protocol.
use core::sync::atomic::{AtomicU32, Ordering};
use fixture_messages::SetLevel;
pub struct Slot {
    value: AtomicU32,
}
impl Default for Slot {
    fn default() -> Self {
        Self::new()
    }
}
impl Slot {
    pub const fn new() -> Self {
        Self {
            value: AtomicU32::new(0),
        }
    }
    pub fn split(&'static mut self) -> (Producer, Consumer) {
        (Producer { slot: self }, Consumer { slot: self })
    }
}
pub struct Producer {
    slot: &'static Slot,
}
pub struct Consumer {
    slot: &'static Slot,
}
impl Consumer {
    pub fn commanded(&self) -> u32 {
        self.slot.value.load(Ordering::Acquire)
    }
}
pub fn set_level(output: &mut Producer, level: &SetLevel) {
    output.slot.value.store(level.value, Ordering::Release);
}
