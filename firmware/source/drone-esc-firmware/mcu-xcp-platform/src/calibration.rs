//! Whole LED + ADC calibration, alternating the two allocated CAL sectors.
//! The supervised LED persistence service remains the sole writer. The board
//! owner stages one validated snapshot before admitting that service's Save.
#[path = "calibration_record.rs"]
mod record;
use am13_rs::{flash::{FlashController, ReservedRecordWriter, Slot}, io::Mmio};
use core::cell::RefCell;
use critical_section::Mutex;
use led_messages::LedConfig;
use record::{AdcCalibration, Record, RECORD_BYTES};

struct Store {
    driver: Option<ReservedRecordWriter<Mmio>>,
    sequence: u64,
    selected: Option<Slot>,
    staged_adc: AdcCalibration,
}
// SAFETY: unique handle installed once; all accesses masked on the one MCU core.
unsafe impl Send for Store {}
static STORE: Mutex<RefCell<Store>> = Mutex::new(RefCell::new(Store {
    driver: None, sequence: 0, selected: None, staged_adc: [[1.0, 0.0]; 30],
}));

/// Read both reserved prefixes, select the newest whole record, or migrate the
/// old A LED-only record in RAM using board-supplied ADC defaults. No flash writes.
pub fn initialize_with_defaults(flash: &mut FlashController<Mmio>, defaults: AdcCalibration) -> LedConfig {
    assert!(record::valid_adc(&defaults));
    let a = flash.read_reserved_record::<RECORD_BYTES>(Slot::A);
    let b = flash.read_reserved_record::<RECORD_BYTES>(Slot::B);
    let selected = match (&a, &b) {
        (Ok(a), Ok(b)) => record::select(a, b, defaults),
        (Ok(a), Err(_)) => record::select(a, &[255; RECORD_BYTES], defaults),
        (Err(_), Ok(b)) => record::select(&[255; RECORD_BYTES], b, defaults),
        _ => None,
    };
    critical_section::with(|cs| {
        let mut store = STORE.borrow(cs).borrow_mut();
        assert!(store.driver.is_none());
        if let Some((slot, r)) = selected {
            store.sequence = r.sequence;
            store.selected = Some(if slot == 0 { Slot::A } else { Slot::B });
            store.staged_adc = r.adc;
            LedConfig::new(r.period_ms, r.duty_permille).expect("validated record")
        } else {
            store.sequence = 0;
            store.selected = None;
            store.staged_adc = defaults;
            LedConfig::defaults()
        }
    })
}
/// Compatibility entry point for a platform without board-specific scaling.
pub fn initialize(flash: &mut FlashController<Mmio>) -> LedConfig {
    initialize_with_defaults(flash, [[1.0, 0.0]; 30])
}
pub fn load_adc() -> AdcCalibration {
    critical_section::with(|cs| STORE.borrow(cs).borrow().staged_adc)
}
/// Freeze all ADC trims together at Save admission. This updates RAM only.
/// Caller serializes board updates until the existing LED Save has completed.
pub fn stage_adc(adc: AdcCalibration) -> bool {
    if !record::valid_adc(&adc) { return false; }
    critical_section::with(|cs| STORE.borrow(cs).borrow_mut().staged_adc = adc);
    true
}
pub fn install(flash: FlashController<Mmio>) {
    critical_section::with(|cs| {
        let mut store = STORE.borrow(cs).borrow_mut();
        assert!(store.driver.is_none());
        store.driver = Some(flash.into_reserved_writer());
    });
}
pub fn store(config: LedConfig, revision: u32) -> bool {
    critical_section::with(|cs| {
        let mut store = STORE.borrow(cs).borrow_mut();
        let Some(sequence) = store.sequence.checked_add(1) else { return false; };
        let r = Record { sequence, revision, period_ms: config.period_ms(),
            duty_permille: config.duty_permille(), adc: store.staged_adc };
        let Some(bytes) = r.encode() else { return false; };
        // Legacy A survives the first full-calibration Save into B. Later saves
        // alternate, erasing only the older record after its successor verified.
        let target = store.selected.map_or(Slot::A, Slot::other);
        let Some(driver) = store.driver.as_mut() else { return false; };
        if driver.write(target, &bytes).is_err() { return false; }
        // HAL already compared every media word to these exact encoded bytes.
        // Only then advance the selected durable generation.
        store.sequence = sequence;
        store.selected = Some(target);
        true
    })
}
