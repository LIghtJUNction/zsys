#![cfg(all(feature = "std", target_os = "linux"))]
//! Isolated allocation and descriptor accounting; one test avoids other test workers.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use zsys::linux::Processes;

struct Counting;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

// SAFETY: pointers and layouts are forwarded unchanged to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            ALLOCATIONS.fetch_add(1, Relaxed);
            LIVE_BYTES.fetch_add(layout.size(), Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        if !replacement.is_null() {
            ALLOCATIONS.fetch_add(1, Relaxed);
            LIVE_BYTES.fetch_sub(layout.size(), Relaxed);
            LIVE_BYTES.fetch_add(new_size, Relaxed);
        }
        replacement
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn descriptors() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}

#[test]
fn repeated_sampling_has_no_hot_allocations_or_retained_resources() {
    let pid = std::process::id();
    // Initialize the standard library and kernel paths before measuring owned resources.
    let mut warm = Processes::new().unwrap();
    warm.refresh_pid(pid).unwrap();
    drop(warm);
    let baseline_fds = descriptors();
    let baseline_bytes = LIVE_BYTES.load(Relaxed);
    for _ in 0..100 {
        let mut collector = Processes::new().unwrap();
        collector.refresh_pid(pid).unwrap();
        drop(collector);
    }
    assert_eq!(LIVE_BYTES.load(Relaxed), baseline_bytes, "collector allocations must be released on drop");
    assert_eq!(descriptors(), baseline_fds, "collector descriptors must be closed on drop");

    let mut collector = Processes::new().unwrap();
    collector.refresh_pid(pid).unwrap();
    let active_fds = descriptors();
    let calls = ALLOCATIONS.load(Relaxed);
    let bytes = LIVE_BYTES.load(Relaxed);
    for _ in 0..20_000 {
        let process = collector.refresh_pid(pid).unwrap();
        std::hint::black_box(process.counters);
    }
    assert_eq!(ALLOCATIONS.load(Relaxed), calls, "a stable PID must reuse its buffers");
    assert_eq!(LIVE_BYTES.load(Relaxed), bytes, "resident allocation size must remain stable");
    assert_eq!(descriptors(), active_fds, "repeated reads must not accumulate file descriptors");
    drop(collector);
    assert_eq!(LIVE_BYTES.load(Relaxed), baseline_bytes);
    assert_eq!(descriptors(), baseline_fds);
}
