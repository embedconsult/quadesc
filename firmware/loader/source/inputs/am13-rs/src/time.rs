//! Embassy-only, fixed 16-task table; no generic Waker ownership or allocator.
use crate::timer::{Deadlines, Monotonic};
use core::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
    task::Waker,
};
use critical_section::Mutex;
use embassy_executor::raw::{TaskRef, task_from_waker, wake_task};
use embassy_time_driver::Driver;
pub const CAPACITY: usize = 16;
struct State {
    ticks: Monotonic,
    deadlines: Deadlines<TaskRef, CAPACITY>,
}
static STATE: Mutex<RefCell<State>> = Mutex::new(RefCell::new(State {
    ticks: Monotonic::new(),
    deadlines: Deadlines::new(),
}));
static CAPACITY_FAULT: AtomicBool = AtomicBool::new(false);
pub fn capacity_faulted() -> bool {
    CAPACITY_FAULT.load(Ordering::Relaxed)
}
struct TimeDriver;
embassy_time_driver::time_driver_impl!(static DRIVER: TimeDriver = TimeDriver);
impl Driver for TimeDriver {
    fn now(&self) -> u64 {
        critical_section::with(|cs| STATE.borrow(cs).borrow().ticks.now())
    }
    fn schedule_wake(&self, at: u64, waker: &Waker) {
        let task = task_from_waker(waker); // validates Embassy's static task waker
        let result = critical_section::with(|cs| {
            let mut state = STATE.borrow(cs).borrow_mut();
            if at <= state.ticks.now() {
                Ok(true)
            } else {
                state.deadlines.schedule(task, at).map(|()| false)
            }
        });
        match result {
            Ok(true) => wake_task(task),
            Ok(false) => (),
            Err(_) => {
                CAPACITY_FAULT.store(true, Ordering::Relaxed);
                panic!("AM13 timer capacity exhausted");
            }
        }
    }
}
/// Call exactly once per SysTick IRQ. Also used to drive deterministic host executor tests.
/// # Safety
/// Must be called only by the sole time source; additional calls falsify elapsed time.
pub unsafe fn on_tick() {
    let ready = critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).borrow_mut();
        state.ticks.tick();
        let now = state.ticks.now();
        state.deadlines.expire(now)
    });
    for task in ready.into_iter().flatten() {
        wake_task(task);
    }
}
#[cfg(target_arch = "arm")]
#[cortex_m_rt::exception]
fn SysTick() {
    unsafe { on_tick() }
}
