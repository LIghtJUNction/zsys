#![cfg(all(feature = "std", target_os = "linux"))]
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use zsys::linux::{DiskIoCollector, Filesystem, Mounts};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zsys-disks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("self")).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn disk_deltas_hotplug_and_transactional_parse() {
    let f = Fixture::new();
    let stat = f.0.join("diskstats");
    fs::write(&stat, "8 0 sda 1 0 10 0 1 0 20 0 5 0 0\n").unwrap();
    let mut disks = DiskIoCollector::with_proc_root(&f.0);
    disks.refresh().unwrap();
    assert!(disks.get(8, 0).unwrap().delta.is_none());
    fs::write(&stat, "8 0 sda 2 0 12 0 3 0 25 0 0 0 0\n").unwrap();
    disks.refresh().unwrap();
    let d = disks.get(8, 0).unwrap().delta.unwrap();
    assert_eq!((d.read_bytes, d.written_bytes), (1024, 2560));
    let old = disks.sample();
    fs::write(&stat, "8 0 sda 2 0 12 0 3 0 25 0 0 0 0\nbad\n").unwrap();
    assert!(disks.refresh().is_err());
    assert_eq!(disks.sample(), old);
    fs::write(&stat, "8 1 sda1 1 0 1 0 1 0 1 0 0 0 0\n").unwrap();
    disks.refresh().unwrap();
    assert!(disks.get(8, 0).is_none());
    assert!(disks.get(8, 1).unwrap().delta.is_none());
}
#[test]
fn mount_discovery_keeps_escaped_paths_and_failure_state() {
    let f = Fixture::new();
    let path = f.0.join("self/mountinfo");
    fs::write(&path, "42 1 8:0 / /space\\040name rw shared:2 - ext4 /dev/sda rw\n").unwrap();
    let mut mounts = Mounts::with_proc_root(&f.0);
    mounts.refresh().unwrap();
    let mount = mounts.iter().next().unwrap();
    assert_eq!(mount.filesystem.path(), std::path::Path::new("/space name"));
    assert_eq!(mount.source, b"/dev/sda");
    fs::write(&path, "invalid").unwrap();
    assert!(mounts.refresh().is_err());
    assert_eq!(mounts.iter().count(), 1);
}
#[test]
#[cfg(target_pointer_width = "64")]
fn native_capacity_and_live_discovery() {
    let mut filesystem = Filesystem::new("/").unwrap();
    let result = filesystem.refresh().unwrap();
    assert!(result.total_bytes > 0);
    assert!(result.available_bytes <= result.free_bytes);
    assert!(result.free_bytes <= result.total_bytes);
    let mut mounts = Mounts::new();
    mounts.refresh().unwrap();
    assert!(mounts.iter().any(|mount| mount.filesystem.path() == std::path::Path::new("/")));
    let mut disks = DiskIoCollector::new();
    disks.refresh().unwrap(); // An empty device list is legal inside a restricted container.
    assert!(Filesystem::new("/definitely-missing-zsys-path").unwrap().refresh().is_err());
    assert!(Filesystem::new(std::path::Path::new("\0")).is_err());
}

#[test]
#[cfg(target_pointer_width = "64")]
fn capacity_errors_retain_timestamp_and_explicit_error() {
    let f = Fixture::new();
    let location = f.0.join("capacity-probe");
    fs::create_dir(&location).unwrap();
    let mut filesystem = Filesystem::new(&location).unwrap();
    let first = filesystem.refresh().unwrap();
    let sample = filesystem.sample();
    fs::remove_dir(&location).unwrap();
    assert!(filesystem.refresh().is_err());
    assert_eq!(filesystem.capacity(), Some(first));
    assert_eq!(filesystem.sample(), sample);
    assert_eq!(filesystem.last_error(), Some(std::io::ErrorKind::NotFound));
    fs::create_dir(&location).unwrap();
    filesystem.refresh().unwrap();
    assert_eq!(filesystem.last_error(), None);
    assert_eq!(filesystem.sample().sequence, sample.sequence + 1);
}

#[test]
fn device_rename_or_counter_reset_invalidates_delta() {
    let f = Fixture::new();
    let stat = f.0.join("diskstats");
    fs::write(&stat, "8 0 sda 10 0 10 0 10 0 20 0 0 0 0\n").unwrap();
    let mut disks = DiskIoCollector::with_proc_root(&f.0);
    disks.refresh().unwrap();
    fs::write(&stat, "8 0 replacement 20 0 20 0 20 0 40 0 0 0 0\n").unwrap();
    disks.refresh().unwrap();
    assert!(disks.get(8, 0).unwrap().delta.is_none());
    fs::write(&stat, "8 0 replacement 1 0 1 0 1 0 2 0 0 0 0\n").unwrap();
    disks.refresh().unwrap();
    assert!(disks.get(8, 0).unwrap().delta.is_none());
}
