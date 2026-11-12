#![cfg(all(feature = "std", target_os = "linux"))]
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use zsys::linux::{Availability, Detail, DetailOptions, Processes, ScanMode};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p =
            std::env::temp_dir().join(format!("zsys-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn process(&self, pid: u32, start: u64, user: u64) {
        fs::create_dir_all(self.0.join(pid.to_string())).unwrap();
        // stat fields 3..24: state, ppid, nine skipped fields, user, system,
        // cutime,cstime,priority,nice,threads,itrealvalue,starttime,vsize,rss.
        fs::write(
            self.0.join(format!("{pid}/stat")),
            format!("{pid} (test (worker)) S 1 0 0 0 0 0 0 0 0 0 {user} 0 0 0 20 0 2 0 {start} 8192 3\n"),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn lifecycle_reuse_and_inaccessible_records_are_not_conflated() {
    let f = Fixture::new();
    f.process(42, 100, 10);
    let mut p = Processes::with_parameters(&f.0, 100, 4096).unwrap();
    assert_eq!(p.refresh().unwrap().added, 1);
    let record = p.get(42).unwrap();
    assert_eq!(record.memory_bytes, 12288);
    assert_eq!(record.threads, 2);
    assert_eq!(record.cpu_percent, None);
    assert_eq!(record.name, b"test (worker)");
    f.process(42, 100, 11);
    assert_eq!(p.refresh().unwrap().reused, 0);
    assert!(p.get(42).unwrap().cpu_percent.is_some());
    f.process(42, 200, 0);
    assert_eq!(p.refresh().unwrap().reused, 1);
    assert_eq!(p.get(42).unwrap().cpu_percent, None);
    fs::remove_file(f.0.join("42/stat")).unwrap();
    assert_eq!(p.refresh().unwrap().failed, 1);
    assert_eq!(p.get(42).unwrap().availability, Availability::Vanished);
    assert_eq!(p.get(42).unwrap().memory_bytes, 12288);
    fs::remove_dir(f.0.join("42")).unwrap();
    assert_eq!(p.refresh().unwrap().removed, 1);
    assert!(p.is_empty());
}
#[test]
fn native_and_std_enumerators_agree() {
    let f = Fixture::new();
    f.process(7, 10, 1);
    f.process(900, 20, 2);
    fs::create_dir(f.0.join("not-a-pid")).unwrap();
    let mut p = Processes::with_parameters(&f.0, 100, 4096).unwrap();
    p.set_scan_mode(ScanMode::Std);
    assert_eq!(p.refresh().unwrap().added, 2);
    if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
        p.set_scan_mode(ScanMode::Native);
        assert_eq!(p.refresh().unwrap().updated, 2);
        assert_eq!(p.len(), 2);
    }
}
#[test]
fn targeted_refresh_does_not_remove_other_processes() {
    let f = Fixture::new();
    f.process(1, 10, 1);
    f.process(2, 20, 1);
    let mut p = Processes::with_parameters(&f.0, 100, 4096).unwrap();
    p.refresh().unwrap();
    fs::remove_dir_all(f.0.join("2")).unwrap();
    p.refresh_pid(1).unwrap();
    assert!(p.get(2).is_some());
    assert!(p.refresh_pid(2).is_err());
    assert!(p.get(2).is_none());
}
#[test]
fn real_process_details_preserve_identity() {
    let mut p = Processes::new().unwrap();
    let pid = std::process::id();
    let id = p.refresh_pid(pid).unwrap().id;
    let details = p.details(pid, DetailOptions::all()).unwrap();
    assert_eq!(details.id, id);
    assert!(matches!(details.executable, Detail::Value(_)));
    assert!(matches!(details.command_line, Detail::Value(_)));
    assert!(matches!(details.user_ids, Detail::Value(_)));
}
#[test]
fn collectors_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Processes>();
    let threads: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(|| {
                let mut p = Processes::new().unwrap();
                for _ in 0..20 {
                    p.refresh_pid(std::process::id()).unwrap();
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().unwrap();
    }
}
#[test]
fn repeated_refresh_and_child_exit() {
    let mut p = Processes::new().unwrap();
    let mut child = std::process::Command::new("sleep").arg("60").spawn().unwrap();
    let pid = child.id();
    let result = p.refresh_pid(pid).map(|p| p.id.pid);
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(result.unwrap(), pid);
    for _ in 0..100 {
        p.refresh_pid(std::process::id()).unwrap();
    }
    assert!(p.refresh_pid(pid).is_err());
    assert!(p.get(pid).is_none());
}
