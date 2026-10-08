#![cfg(feature = "embassy")]
use embassy_executor::raw::Executor;
#[unsafe(export_name = "__pender")]
fn pender(_: *mut ()) {}
#[embassy_executor::task(pool_size = 17)]
async fn sleeper() {
    embassy_time::Timer::after_ticks(1000).await;
}
#[test]
fn seventeenth_task_trips_latched_capacity_fault() {
    let executor = Box::leak(Box::new(Executor::new(core::ptr::null_mut())));
    for _ in 0..17 {
        executor.spawner().spawn(sleeper()).unwrap();
    }
    // Host catch is test-only. Target panic handler halts without allocation.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        executor.poll();
    }));
    assert!(result.is_err());
    assert!(am13_rs::time::capacity_faulted());
}
