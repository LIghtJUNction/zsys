#![cfg(all(feature = "std", target_os = "linux"))]
//! Per-thread allocation and descriptor accounting for one long-lived collector.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::{Duration, Instant};
use zsys::linux::Processes;

struct Counting;
#[derive(Clone, Copy, Default)]
struct Counters {
    calls: u64,
    bytes: i128,
}
thread_local! {
    // A const, destructor-free TLS value does not allocate. libtest's background
    // long-test notifications must not be counted as collector allocations.
    static COUNTERS: Cell<Counters> = const { Cell::new(Counters { calls: 0, bytes: 0 }) };
}
fn allocated(size: usize) {
    let _ = COUNTERS.try_with(|slot| {
        let old = slot.get();
        slot.set(Counters { calls: old.calls.wrapping_add(1), bytes: old.bytes + size as i128 });
    });
}
fn deallocated(size: usize) {
    let _ = COUNTERS.try_with(|slot| {
        let old = slot.get();
        slot.set(Counters { bytes: old.bytes - size as i128, ..old });
    });
}
fn counters() -> Counters {
    COUNTERS.with(Cell::get)
}

// SAFETY: pointers and layouts are forwarded unchanged to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            allocated(layout.size());
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        deallocated(layout.size());
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        if !replacement.is_null() {
            deallocated(layout.size());
            allocated(new_size);
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
    for limit in [0, 1] {
        exercise(soak, limit);
    }
}

fn exercise(soak: Option<Duration>, limit: usize) {
    let pid = std::process::id();
    // Initialize the standard library and kernel paths before measuring owned resources.
    let mut warm = Processes::new().unwrap();
    warm.refresh_pid(pid).unwrap();
    drop(warm);
    let baseline_fds = descriptors();
    let baseline_bytes = counters().bytes;
    for _ in 0..100 {
        let mut collector = Processes::new().unwrap();
        collector.set_directory_cache_limit(limit);
        collector.refresh_pid(pid).unwrap();
        drop(collector);
    }
    assert_eq!(counters().bytes, baseline_bytes, "collector allocations must be released on drop");
    assert_eq!(descriptors(), baseline_fds, "collector descriptors must be closed on drop");

    let mut collector = Processes::new().unwrap();
    collector.set_directory_cache_limit(limit);
    collector.refresh_pid(pid).unwrap();
    assert_eq!(collector.cached_directory_count(), limit);
    let active_fds = descriptors();
    // One proc-root descriptor and at most one retained PID directory.
    assert_eq!(active_fds, baseline_fds + 1 + limit);
    let bytes = counters().bytes;
    let started = Instant::now();
    let mut samples = 0u64;
    loop {
        let batch = if soak.is_some() { 1024 } else { (20_000 - samples).min(1024) };
        let calls = counters().calls;
        for _ in 0..batch {
            let process = collector.refresh_pid(pid).unwrap();
            std::hint::black_box(process.counters);
        }
        samples += batch;
        assert_eq!(counters().calls, calls, "a stable PID must reuse its buffers");
        assert_eq!(counters().bytes, bytes, "resident allocation size must remain stable");
        // Directory inspection allocates temporary std storage; it is outside each
        // measured batch. The same collector stays alive throughout the soak.
        assert_eq!(descriptors(), active_fds, "repeated reads must not accumulate file descriptors");
        assert_eq!(counters().bytes, bytes, "descriptor checks must release their temporary storage");
        if soak.map_or(samples >= 20_000, |duration| started.elapsed() >= duration) {
            break;
        }
    }
    let elapsed = started.elapsed();
    collector.set_directory_cache_limit(0);
    assert_eq!(descriptors(), baseline_fds + 1);
    drop(collector);
    assert_eq!(counters().bytes, baseline_bytes);
    assert_eq!(descriptors(), baseline_fds);
    eprintln!(
        "process resource soak: directory_cache_limit={limit}, samples={samples}, elapsed_seconds={:.3}, samples_per_second={:.0}, hot_allocations=0, retained_growth_bytes=0, descriptor_growth=0",
        elapsed.as_secs_f64(),
        samples as f64 / elapsed.as_secs_f64()
    );
}
