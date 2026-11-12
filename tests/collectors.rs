#![cfg(all(feature = "std", target_os = "linux"))]
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use zsys::linux::{CpuCollector, MemoryCollector, Networks};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zsys-collectors-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("net")).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn cpu_hotplug_and_failed_reads_preserve_snapshots() {
    let f = Fixture::new();
    fs::write(f.0.join("stat"), "cpu 1 0 0 9\ncpu0 1 0 0 9\n").unwrap();
    let mut cpu = CpuCollector::with_roots(&f.0, &f.0);
    cpu.refresh().unwrap();
    assert_eq!(cpu.cores()[0].busy_percent, None);
    fs::write(f.0.join("stat"), "cpu 6 0 0 14\ncpu0 6 0 0 14\n").unwrap();
    cpu.refresh().unwrap();
    assert_eq!(cpu.aggregate_busy_percent(), Some(50.0));
    assert_eq!(cpu.cores()[0].busy_percent, Some(50.0));
    fs::write(f.0.join("stat"), "cpu 10 0 0 20\ncpu0 7 0 0 15\ncpu2 3 0 0 5\n").unwrap();
    cpu.refresh().unwrap();
    assert_eq!(cpu.aggregate_busy_percent(), None);
    assert_eq!(cpu.cores()[1].id, 2);
    assert_eq!(cpu.cores()[1].busy_percent, None);
    let previous = cpu.sample();
    fs::write(f.0.join("stat"), "cpu 1\n").unwrap();
    assert!(cpu.refresh().is_err());
    assert_eq!(cpu.sample(), previous);
    assert_eq!(cpu.cores().len(), 2);
}
#[test]
fn memory_estimates_are_explicit_and_failed_reads_do_not_become_zeros() {
    let f = Fixture::new();
    fs::write(f.0.join("meminfo"), "MemTotal: 1000 kB\nMemFree: 200 kB\n").unwrap();
    let mut memory = MemoryCollector::with_proc_root(&f.0);
    assert!(memory.refresh().unwrap().available_is_estimate);
    let previous = *memory.snapshot();
    fs::write(f.0.join("meminfo"), "corrupt").unwrap();
    assert!(memory.refresh().is_err());
    assert_eq!(*memory.snapshot(), previous);
}
fn net(rx: u64, tx: u64) -> String {
    format!(
        "Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n eth0: {rx} 1 0 0 0 0 0 0 {tx} 1 0 0 0 0 0 0\n"
    )
}
#[test]
fn network_resets_and_malformed_data() {
    let f = Fixture::new();
    fs::write(f.0.join("net/dev"), net(100, 200)).unwrap();
    let mut networks = Networks::with_proc_root(&f.0);
    networks.refresh_counters().unwrap();
    assert!(networks.get(b"eth0").unwrap().delta.is_none());
    fs::write(f.0.join("net/dev"), net(150, 300)).unwrap();
    networks.refresh_counters().unwrap();
    assert_eq!(networks.get(b"eth0").unwrap().delta.unwrap().received_bytes, 50);
    fs::write(f.0.join("net/dev"), net(1, 1)).unwrap();
    networks.refresh_counters().unwrap();
    assert!(networks.get(b"eth0").unwrap().delta.is_none());
    let previous = networks.sample();
    fs::write(f.0.join("net/dev"), "broken").unwrap();
    assert!(networks.refresh_counters().is_err());
    assert_eq!(networks.sample(), previous);
    assert!(networks.get(b"eth0").is_some());
}
#[test]
fn live_cpu_memory_and_network_addresses() {
    let mut cpu = CpuCollector::new();
    cpu.refresh().unwrap();
    assert!(!cpu.cores().is_empty());
    let mut memory = MemoryCollector::new();
    let m = memory.refresh().unwrap();
    assert!(m.counters.total_bytes > 0);
    let mut networks = Networks::new();
    networks.refresh().unwrap();
    let lo = networks.get(b"lo").unwrap();
    assert!(lo.details.as_ref().unwrap().is_loopback());
    assert!(lo.details.as_ref().unwrap().addresses.iter().any(|a| a.address.is_loopback()));
}
