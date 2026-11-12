//! Compiled only in the allocation-instrumented binary, never in latency runs.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
struct Counting;
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
fn allocated(size: usize) {
    CALLS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size as u64, Relaxed);
    let now = LIVE.fetch_add(size as u64, Relaxed) + size as u64;
    PEAK.fetch_max(now, Relaxed);
}
// SAFETY: allocation pointers and layouts are forwarded unchanged to System.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            allocated(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            allocated(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size() as u64, Relaxed);
        unsafe { System.dealloc(p, layout) };
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(p, layout, new_size) };
        if !new.is_null() {
            LIVE.fetch_sub(layout.size() as u64, Relaxed);
            allocated(new_size);
        }
        new
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;
pub fn start() -> (u64, u64, u64) {
    let live = LIVE.load(Relaxed);
    PEAK.store(live, Relaxed);
    (CALLS.load(Relaxed), BYTES.load(Relaxed), live)
}
pub fn finish(old: (u64, u64, u64)) -> (u64, u64, u64) {
    (CALLS.load(Relaxed) - old.0, BYTES.load(Relaxed) - old.1, PEAK.load(Relaxed).saturating_sub(old.2))
}
