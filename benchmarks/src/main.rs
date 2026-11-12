//! Each invocation measures one implementation in a fresh OS process.
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::Instant;

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

// SAFETY: every operation forwards the original pointer/layout to System.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() { allocated(layout.size()); }
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

fn measure(mut collect: impl FnMut(), samples: usize) {
    for _ in 0..50 { collect(); }
    let mut rows = Vec::with_capacity(samples);
    for _ in 0..samples {
        let live = LIVE.load(Relaxed);
        PEAK.store(live, Relaxed);
        let a = CALLS.load(Relaxed);
        let b = BYTES.load(Relaxed);
        let start = Instant::now();
        collect();
        let ns = start.elapsed().as_nanos();
        rows.push((ns, CALLS.load(Relaxed) - a, BYTES.load(Relaxed) - b,
            PEAK.load(Relaxed).saturating_sub(live)));
    }
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let rss = status.lines().find(|s| s.starts_with("VmHWM:")).unwrap_or("VmHWM: unavailable");
    eprintln!("{rss}");
    println!("sample,ns,allocations,allocated_bytes,peak_extra_live_bytes");
    for (i, (ns, calls, bytes, peak)) in rows.into_iter().enumerate() {
        println!("{i},{ns},{calls},{bytes},{peak}");
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let implementation = args.get(1).expect("zsys or sysinfo");
    let case = args.get(2).map(String::as_str).unwrap_or("memory");
    let samples: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(1000);
    assert!((10..=1_000_000).contains(&samples));
    match (implementation.as_str(), case) {
        ("zsys", "memory") => {
            let mut c = zsys::linux::LinuxCollector::new();
            measure(|| {
                let m = c.memory().unwrap();
                assert!(m.total_bytes > 0 && m.available_bytes <= m.total_bytes);
                black_box((m.total_bytes, m.available_bytes, m.swap_total_bytes, m.swap_free_bytes));
            }, samples);
        }
        ("sysinfo", "memory") => {
            let mut c = sysinfo::System::new().unwrap();
            measure(|| {
                c.refresh_memory();
                assert!(c.total_memory() > 0 && c.available_memory() <= c.total_memory());
                black_box((c.total_memory(), c.available_memory(), c.total_swap(), c.free_swap()));
            }, samples);
        }
        _ => panic!("unsupported benchmark: {implementation} {case}"),
    }
}
