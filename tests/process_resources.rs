#![cfg(all(feature = "std", target_os = "linux"))]
//! Isolated allocation and descriptor accounting; one test avoids other test workers.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};
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
    let soak = std::env::var("ZSYS_SOAK_SECONDS").ok().map(|value| {
        let seconds: u64 = value.parse().expect("ZSYS_SOAK_SECONDS must be a positive integer");
        assert!(seconds > 0, "ZSYS_SOAK_SECONDS must be positive");
        Duration::from_secs(seconds)
    });
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
    let bytes = LIVE_BYTES.load(Relaxed);
    let started = Instant::now();
    let mut samples = 0u64;
    loop {
        let batch = if soak.is_some() { 1024 } else { (20_000 - samples).min(1024) };
        let calls = ALLOCATIONS.load(Relaxed);
        for _ in 0..batch {
            let process = collector.refresh_pid(pid).unwrap();
            std::hint::black_box(process.counters);
        }
        samples += batch;
        assert_eq!(ALLOCATIONS.load(Relaxed), calls, "a stable PID must reuse its buffers");
        assert_eq!(LIVE_BYTES.load(Relaxed), bytes, "resident allocation size must remain stable");
        // Directory inspection allocates temporary std storage; it is outside each
        // measured batch. The same collector stays alive throughout the soak.
        assert_eq!(descriptors(), active_fds, "repeated reads must not accumulate file descriptors");
        assert_eq!(LIVE_BYTES.load(Relaxed), bytes, "descriptor checks must release their temporary storage");
        if soak.map_or(samples >= 20_000, |duration| started.elapsed() >= duration) {
            break;
        }
    }
    let elapsed = started.elapsed();
    drop(collector);
    assert_eq!(LIVE_BYTES.load(Relaxed), baseline_bytes);
    assert_eq!(descriptors(), baseline_fds);
    eprintln!(
        "process resource soak: samples={samples}, elapsed_seconds={:.3}, samples_per_second={:.0}, hot_allocations=0, retained_growth_bytes=0, descriptor_growth=0",
        elapsed.as_secs_f64(),
        samples as f64 / elapsed.as_secs_f64()
    );
}
