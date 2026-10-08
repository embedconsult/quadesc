// Copyright 2025 Bloxide, all rights reserved
//! Actual generic Cortex-M hooks with a fixed eight-alarm time driver.
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::{RefCell, UnsafeCell},
    sync::atomic::{AtomicUsize, Ordering},
    task::Waker,
};
use critical_section::Mutex;
use embassy_time_driver::Driver;

#[repr(align(16))]
struct Heap(UnsafeCell<[u8; 8192]>);
// Startup runs on one foreground executor; the atomic cursor assigns disjoint
// ranges. Once frozen every entry point halts before accessing storage.
unsafe impl Sync for Heap {}
static HEAP: Heap = Heap(UnsafeCell::new([0; 8192]));
static CURSOR: AtomicUsize = AtomicUsize::new(0);
#[used]
static FORBIDDEN_ATTEMPTS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
struct StartupAllocator;
fn check_frozen(operation: usize) {
    if super::FROZEN.load(Ordering::Acquire) {
        FORBIDDEN_ATTEMPTS[operation].fetch_add(1, Ordering::Relaxed);
        panic!("forbidden post-freeze allocator operation");
    }
}
unsafe impl GlobalAlloc for StartupAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        check_frozen(0);
        let base = HEAP.0.get().cast::<u8>() as usize;
        let mut cursor = CURSOR.load(Ordering::Relaxed);
        loop {
            let Some(aligned) = base
                .checked_add(cursor)
                .and_then(|p| p.checked_add(layout.align() - 1))
                .map(|p| p & !(layout.align() - 1))
            else {
                return core::ptr::null_mut();
            };
            let offset = aligned - base;
            let Some(end) = offset.checked_add(layout.size()).filter(|end| *end <= 8192) else {
                return core::ptr::null_mut();
            };
            match CURSOR.compare_exchange(cursor, end, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => return aligned as *mut u8,
                Err(next) => cursor = next,
            }
        }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        check_frozen(1);
        let ptr = self.alloc(layout);
        if !ptr.is_null() {
            ptr.write_bytes(0, layout.size());
        }
        ptr
    }
    unsafe fn realloc(&self, old: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        check_frozen(2);
        let Ok(new_layout) = Layout::from_size_align(size, layout.align()) else {
            return core::ptr::null_mut();
        };
        let new = self.alloc(new_layout);
        if !new.is_null() {
            new.copy_from_nonoverlapping(old, layout.size().min(size));
        }
        new
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {
        check_frozen(3);
        // Startup arena retains its fixed backing memory until MCU reset.
    }
}
#[global_allocator]
static ALLOCATOR: StartupAllocator = StartupAllocator;

struct Alarm {
    at: u64,
    waker: Waker,
}
struct TimeState {
    ticks: u64,
    alarms: [Option<Alarm>; 8],
}
struct TimeDriver {
    state: Mutex<RefCell<TimeState>>,
}
impl Driver for TimeDriver {
    fn now(&self) -> u64 {
        critical_section::with(|cs| self.state.borrow(cs).borrow().ticks)
    }
    fn schedule_wake(&self, at: u64, waker: &Waker) {
        let immediate = critical_section::with(|cs| {
            let mut state = self.state.borrow(cs).borrow_mut();
            if at <= state.ticks {
                return true;
            }
            if let Some(alarm) = state
                .alarms
                .iter_mut()
                .flatten()
                .find(|a| a.waker.will_wake(waker))
            {
                alarm.at = alarm.at.min(at);
                return false;
            }
            let slot = state
                .alarms
                .iter_mut()
                .find(|s| s.is_none())
                .expect("fixed alarm table exhausted");
            *slot = Some(Alarm {
                at,
                waker: waker.clone(),
            });
            false
        });
        if immediate {
            waker.wake_by_ref();
        }
    }
}
embassy_time_driver::time_driver_impl!(static DRIVER: TimeDriver = TimeDriver {
    state: Mutex::new(RefCell::new(TimeState { ticks: 0, alarms: [const { None }; 8] }))
});
pub fn initialize_time() -> cortex_m::peripheral::SYST {
    let mut core = cortex_m::Peripherals::take().expect("core peripherals already owned");
    core.SYST
        .set_clock_source(cortex_m::peripheral::syst::SystClkSource::Core);
    // Generic link fixture assumes a 32 MHz clock. Actual board integration
    // supplies and measures its clock, which this fixture cannot establish.
    core.SYST.set_reload(3199);
    core.SYST.clear_current();
    core.SYST.enable_interrupt();
    core.SYST.enable_counter();
    core.SYST
}
#[cortex_m_rt::exception]
fn SysTick() {
    let wake = critical_section::with(|cs| {
        let mut state = DRIVER.state.borrow(cs).borrow_mut();
        state.ticks += 1;
        let now = state.ticks;
        let mut wake: [Option<Waker>; 8] = [const { None }; 8];
        for (slot, alarm) in wake.iter_mut().zip(state.alarms.iter_mut()) {
            if alarm.as_ref().is_some_and(|a| a.at <= now) {
                *slot = alarm.take().map(|a| a.waker);
            }
        }
        wake
    });
    for waker in wake.into_iter().flatten() {
        waker.wake();
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    cortex_m::interrupt::disable();
    loop {
        cortex_m::asm::wfi();
    }
}
