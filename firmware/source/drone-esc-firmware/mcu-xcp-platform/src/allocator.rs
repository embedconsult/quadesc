//! Fixed startup arena and irreversible process-wide four-operation guard.
//! Adapted from pinned T06 embedded fixture target.rs (2b4ec83); direct
//! negative-control behavior is separately recorded by prep-allocator-fixture.
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    ptr,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

const HEAP_BYTES: usize = 8192;
#[repr(align(16))]
struct Heap(UnsafeCell<[u8; HEAP_BYTES]>);
unsafe impl Sync for Heap {}
static HEAP: Heap = Heap(UnsafeCell::new([0; HEAP_BYTES]));
static CURSOR: AtomicUsize = AtomicUsize::new(0);
static FROZEN: AtomicBool = AtomicBool::new(false);
#[used]
static ATTEMPTS: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
#[used]
static HIGH_WATER: AtomicUsize = AtomicUsize::new(0);

pub fn freeze() {
    FROZEN.store(true, Ordering::Release);
}
pub fn frozen() -> bool {
    FROZEN.load(Ordering::Acquire)
}
pub fn high_water() -> usize {
    HIGH_WATER.load(Ordering::Acquire)
}
pub fn attempts() -> [usize; 4] {
    core::array::from_fn(|i| ATTEMPTS[i].load(Ordering::Acquire))
}
fn check(operation: usize) {
    if frozen() {
        ATTEMPTS[operation].fetch_add(1, Ordering::Relaxed);
        super::halt();
    }
}
struct StartupAllocator;
unsafe impl GlobalAlloc for StartupAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        check(0);
        let base = HEAP.0.get().cast::<u8>() as usize;
        let mut cursor = CURSOR.load(Ordering::Relaxed);
        loop {
            let Some(aligned) = base
                .checked_add(cursor)
                .and_then(|p| p.checked_add(layout.align() - 1))
                .map(|p| p & !(layout.align() - 1))
            else {
                return ptr::null_mut();
            };
            let end = aligned - base + layout.size();
            if end > HEAP_BYTES {
                return ptr::null_mut();
            }
            match CURSOR.compare_exchange(cursor, end, Ordering::AcqRel, Ordering::Relaxed) {
                Ok(_) => {
                    HIGH_WATER.fetch_max(end, Ordering::AcqRel);
                    return aligned as *mut u8;
                }
                Err(next) => cursor = next,
            }
        }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        check(1);
        let ptr = unsafe { self.alloc(layout) };
        if !ptr.is_null() {
            unsafe { ptr.write_bytes(0, layout.size()) };
        }
        ptr
    }
    unsafe fn realloc(&self, old: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        check(2);
        let Ok(next_layout) = Layout::from_size_align(size, layout.align()) else {
            return ptr::null_mut();
        };
        let next = unsafe { self.alloc(next_layout) };
        if !next.is_null() {
            unsafe { next.copy_from_nonoverlapping(old, layout.size().min(size)) };
        }
        next
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {
        check(3);
        // Startup objects live until reset. There is no post-freeze deallocation.
    }
}
#[global_allocator]
static ALLOCATOR: StartupAllocator = StartupAllocator;
