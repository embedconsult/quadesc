// Copyright 2025 Bloxide, all rights reserved
#![no_std]
//! Generic Cortex-M33 link fixture. No board registers or electrical claims.
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use fixture_context::{Consumer, Producer, Slot};
use static_cell::StaticCell;
#[cfg(not(target_os = "none"))]
extern crate std;
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static FROZEN: AtomicBool = AtomicBool::new(false);
static OUTPUT: AtomicU32 = AtomicU32::new(0);
pub struct Resources {
    pub led_output: Producer,
    pub led_service: LedService,
}
/// Concrete platform-owned service resources; never exposed to the blox.
pub struct LedService {
    consumer: Consumer,
    hardware: Hardware,
}
struct Hardware {
    // Retain the exclusive Cortex-M timer token for the process lifetime.
    // The interrupt owns only the fixed software clock/alarm state.
    #[cfg(target_os = "none")]
    _systick: cortex_m::peripheral::SYST,
}
impl Hardware {
    fn record_output(&mut self, value: u32) {
        // Generic target has no selected GPIO. This is a software observation.
        OUTPUT.store(value, Ordering::Release);
    }
}
#[derive(Debug)]
pub enum StartupError {
    AlreadyInitialized,
    Injected,
}
pub fn initialize() -> Result<Resources, StartupError> {
    #[cfg(not(target_os = "none"))]
    if std::env::var("FIXTURE_FAIL").as_deref() == Ok("init") {
        return Err(StartupError::Injected);
    }
    if INITIALIZED.swap(true, Ordering::AcqRel) {
        return Err(StartupError::AlreadyInitialized);
    }
    let hardware = Hardware {
        #[cfg(target_os = "none")]
        _systick: target::initialize_time(),
    };
    static SLOT: StaticCell<Slot> = StaticCell::new();
    let (led_output, consumer) = SLOT.init(Slot::new()).split();
    Ok(Resources {
        led_output,
        led_service: LedService { consumer, hardware },
    })
}
pub fn startup_failed(_: StartupError) -> ! {
    assert!(!FROZEN.load(Ordering::Acquire));
    assert_eq!(OUTPUT.load(Ordering::Acquire), 0);
    panic!("platform startup failed before freeze and output")
}
pub fn freeze() {
    FROZEN.store(true, Ordering::Release);
}
pub fn start_led(
    spawner: embassy_executor::Spawner,
    resources: LedService,
) -> Result<(), StartupError> {
    #[cfg(not(target_os = "none"))]
    if std::env::var("FIXTURE_FAIL").as_deref() == Ok("service") {
        return Err(StartupError::Injected);
    }
    spawner.must_spawn(led_task(resources));
    Ok(())
}
#[embassy_executor::task]
async fn led_task(mut resources: LedService) {
    assert!(FROZEN.load(Ordering::Acquire));
    loop {
        // This software output establishes consumer ownership only. A real
        // board adapter replaces it with owned GPIO; it is not optical proof.
        resources
            .hardware
            .record_output(resources.consumer.commanded());
        embassy_time::Timer::after_micros(1000).await;
    }
}
#[cfg(target_os = "none")]
mod target;
