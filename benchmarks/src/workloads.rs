//! Matched public-API profiles. No competitor implementation is used by zsys.
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::{hint::black_box, path::PathBuf};
use sysinfo::{CpuRefreshKind, DiskRefreshKind, ProcessRefreshKind, ProcessesToUpdate, UpdateKind};
use zsys::linux::{
    CpuCollector, DetailOptions, DiskIoCollector, Filesystem, MemoryCollector, Networks, Processes, ScanMode,
};

pub type Task = Box<dyn FnMut() -> (usize, usize)>;
#[derive(Clone, Default)]
pub struct Scope {
    pub mounts: Vec<PathBuf>,
    pub devices: Vec<(u32, u32)>,
    pub pid: u32,
}

pub fn device(path: &std::path::Path) -> Option<(u32, u32)> {
    let n = path.metadata().ok()?.rdev();
    if n == 0 {
        return None;
    }
    Some(((((n >> 8) & 0xfff) | ((n >> 32) & 0xfffff000)) as u32, ((n & 0xff) | ((n >> 12) & 0xffffff00)) as u32))
}
pub fn discover() {
    let disks = sysinfo::Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing()).unwrap();
    for disk in &disks {
        print!("mount\t");
        for byte in disk.mount_point().as_os_str().as_bytes() {
            print!("{byte:02x}");
        }
        println!();
        if let Some((major, minor)) = device(std::path::Path::new(disk.name())) {
            println!("device\t{major}:{minor}");
        }
    }
}
fn process_kind(cpu: bool, details: bool, list: bool) -> ProcessRefreshKind {
    let mut kind = ProcessRefreshKind::nothing().without_tasks();
    if !list {
        kind = kind.with_memory();
    }
    if cpu {
        kind = kind.with_cpu();
    }
    if details {
        kind = kind.with_cmd(UpdateKind::Always).with_exe(UpdateKind::Always).with_user(UpdateKind::Always);
    }
    kind
}
fn zprocess(p: &mut Processes, pid: Option<u32>, details: bool) -> (usize, usize) {
    if let Some(pid) = pid {
        p.refresh_pid(pid).unwrap();
        if details {
            black_box(p.details(pid, DetailOptions::all()).unwrap());
        }
        black_box(p.get(pid));
        (1, 0)
    } else {
        let changes = p.refresh().unwrap();
        let _ = black_box(p.iter());
        (changes.updated, changes.failed)
    }
}
fn disks(scope: &Scope) -> sysinfo::Disks {
    let disks = sysinfo::Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing()).unwrap();
    assert_eq!(
        disks.iter().filter(|d| scope.mounts.iter().any(|p| p == d.mount_point())).count(),
        scope.mounts.len(),
        "disk topology changed after scope discovery"
    );
    disks
}
fn sdisks(disks: &mut sysinfo::Disks, paths: &[PathBuf], io: bool) -> (usize, usize) {
    let kind = if io { DiskRefreshKind::nothing().with_io_usage() } else { DiskRefreshKind::nothing().with_storage() };
    let mut count = 0;
    let mut failed = 0;
    for disk in disks.iter_mut().filter(|disk| paths.iter().any(|path| path == disk.mount_point())) {
        if disk.refresh_specifics(kind) {
            count += 1;
        } else {
            failed += 1;
        }
        if io {
            black_box(disk.usage());
        } else {
            black_box((disk.total_space(), disk.available_space()));
        }
    }
    (count, failed)
}
pub fn task(implementation: &str, case: &str, scope: &Scope) -> Task {
    let z = implementation.starts_with("zsys");
    let scan = if implementation == "zsys-std" { ScanMode::Std } else { ScanMode::Native };
    let pid = scope.pid;
    match case {
        "memory" if z => {
            let mut c = MemoryCollector::new();
            Box::new(move || {
                black_box(c.refresh().unwrap());
                (1, 0)
            })
        }
        "memory" => {
            let mut c = sysinfo::System::new().unwrap();
            Box::new(move || {
                c.refresh_memory();
                black_box((c.total_memory(), c.available_memory(), c.free_memory(), c.total_swap(), c.free_swap()));
                (1, 0)
            })
        }
        "cpu" if z => {
            let mut c = CpuCollector::new();
            Box::new(move || {
                c.refresh().unwrap();
                black_box((c.aggregate_busy_percent(), c.cores()));
                (c.cores().len(), 0)
            })
        }
        "cpu" => {
            let mut c = sysinfo::System::new().unwrap();
            c.refresh_cpu_list(CpuRefreshKind::nothing());
            Box::new(move || {
                c.refresh_cpu_usage();
                black_box((c.global_cpu_usage(), c.cpus()));
                (c.cpus().len(), 0)
            })
        }
        "enumerate" if z => {
            let mut c = Processes::new().unwrap();
            c.set_scan_mode(scan);
            Box::new(move || {
                let pids = c.enumerate_pids().unwrap();
                black_box(pids);
                (pids.len(), 0)
            })
        }
        "process-one" | "process-all" | "process-one-cpu" | "process-all-cpu" | "process-list" | "process-details" => {
            let one = case.contains("one") || case == "process-details";
            let cpu = case.ends_with("cpu");
            let details = case == "process-details";
            if z {
                let mut c = Processes::new().unwrap();
                c.set_scan_mode(scan);
                Box::new(move || zprocess(&mut c, one.then_some(pid), details))
            } else {
                let mut c = sysinfo::System::new().unwrap();
                let ids = [sysinfo::Pid::from_u32(pid)];
                let kind = process_kind(cpu, details, case == "process-list");
                Box::new(move || {
                    let targets = if one { ProcessesToUpdate::Some(&ids) } else { ProcessesToUpdate::All };
                    let count = c.refresh_processes_specifics(targets, true, kind);
                    assert!(count != 0, "no processes refreshed");
                    black_box(c.processes());
                    (count, 0)
                })
            }
        }
        "network" | "network-counters" if z => {
            let mut c = Networks::new();
            let full = case == "network";
            Box::new(move || {
                if full {
                    c.refresh().unwrap();
                } else {
                    c.refresh_counters().unwrap();
                }
                let _ = black_box(c.iter());
                (c.iter().count(), 0)
            })
        }
        "network" => {
            let mut c = sysinfo::Networks::new().unwrap();
            Box::new(move || {
                c.refresh(true);
                black_box(&c);
                (c.len(), 0)
            })
        }
        "disk-space" if z => {
            assert!(!scope.mounts.is_empty(), "no comparable mounts; skip this case");
            let mut fs: Vec<_> = scope.mounts.iter().map(|p| Filesystem::new(p).unwrap()).collect();
            Box::new(move || {
                let mut failed = 0;
                for f in &mut fs {
                    if f.refresh().is_err() {
                        failed += 1;
                    }
                }
                black_box(&fs);
                (fs.len() - failed, failed)
            })
        }
        "disk-space" => {
            assert!(!scope.mounts.is_empty(), "no comparable mounts; skip this case");
            let mut d = disks(scope);
            let paths = scope.mounts.clone();
            Box::new(move || sdisks(&mut d, &paths, false))
        }
        "disk-io" | "disk-io-all" if z => {
            let mut c = DiskIoCollector::new();
            let devices = scope.devices.clone();
            let all = case == "disk-io-all";
            assert!(all || !devices.is_empty(), "no comparable block devices; skip this case");
            Box::new(move || {
                c.refresh().unwrap();
                if all {
                    return (black_box(c.iter().count()), 0);
                }
                for &(major, minor) in &devices {
                    black_box(c.get(major, minor).expect("block device disappeared"));
                }
                (devices.len(), 0)
            })
        }
        "disk-io" => {
            assert!(!scope.devices.is_empty(), "no comparable block devices; skip this case");
            let mut d = disks(scope);
            let devices = scope.devices.clone();
            let mut seen = Vec::new();
            let paths: Vec<_> = d
                .iter()
                .filter_map(|disk| {
                    let id = device(std::path::Path::new(disk.name()))?;
                    if !devices.contains(&id) || seen.contains(&id) {
                        return None;
                    }
                    seen.push(id);
                    Some(disk.mount_point().to_owned())
                })
                .collect();
            assert_eq!(seen.len(), devices.len(), "device scope changed");
            Box::new(move || sdisks(&mut d, &paths, true))
        }
        "basic" if z => {
            let mut cpu = CpuCollector::new();
            let mut memory = MemoryCollector::new();
            let mut processes = Processes::new().unwrap();
            let mut network = Networks::new();
            let mut filesystems: Vec<_> = scope.mounts.iter().map(|p| Filesystem::new(p).unwrap()).collect();
            Box::new(move || {
                cpu.refresh().unwrap();
                memory.refresh().unwrap();
                let changes = processes.refresh().unwrap();
                network.refresh().unwrap();
                let mut failed = changes.failed;
                for fs in &mut filesystems {
                    if fs.refresh().is_err() {
                        failed += 1;
                    }
                }
                black_box((&cpu, &memory, &processes, &network, &filesystems));
                (changes.updated, failed)
            })
        }
        "basic" => {
            let mut c = sysinfo::System::new().unwrap();
            c.refresh_cpu_list(CpuRefreshKind::nothing());
            let mut networks = sysinfo::Networks::new().unwrap();
            let mut d = disks(scope);
            let paths = scope.mounts.clone();
            Box::new(move || {
                c.refresh_cpu_usage();
                c.refresh_memory();
                let count =
                    c.refresh_processes_specifics(ProcessesToUpdate::All, true, process_kind(true, false, false));
                networks.refresh(true);
                let (_, failed) = sdisks(&mut d, &paths, false);
                black_box((&c, &networks, &d));
                (count, failed)
            })
        }
        _ => panic!("unsupported profile: {implementation}/{case}"),
    }
}

/// Fresh collectors for every sample, without allocating a harness task in the timed region.
/// A cold CPU sample establishes a baseline; it cannot yield an intersample utilization.
pub fn cold(implementation: &str, case: &str, scope: &Scope) -> (usize, usize) {
    let z = implementation.starts_with("zsys");
    match (z, case) {
        (true, "memory") => {
            black_box(MemoryCollector::new().refresh().unwrap());
            (1, 0)
        }
        (false, "memory") => {
            let mut c = sysinfo::System::new().unwrap();
            c.refresh_memory();
            black_box((c.total_memory(), c.available_memory(), c.free_memory(), c.total_swap(), c.free_swap()));
            (1, 0)
        }
        (true, "process-all") => {
            let mut c = Processes::new().unwrap();
            if implementation == "zsys-std" {
                c.set_scan_mode(ScanMode::Std);
            }
            zprocess(&mut c, None, false)
        }
        (false, "process-all") => {
            let mut c = sysinfo::System::new().unwrap();
            let count = c.refresh_processes_specifics(ProcessesToUpdate::All, true, process_kind(false, false, false));
            black_box(c.processes());
            (count, 0)
        }
        (true, "network") => {
            let mut c = Networks::new();
            c.refresh().unwrap();
            black_box(&c);
            (c.iter().count(), 0)
        }
        (false, "network") => {
            let mut c = sysinfo::Networks::new().unwrap();
            c.refresh(true);
            black_box(&c);
            (c.len(), 0)
        }
        (true, "basic") => {
            let mut cpu = CpuCollector::new();
            let mut memory = MemoryCollector::new();
            let mut processes = Processes::new().unwrap();
            let mut network = Networks::new();
            let mut filesystems: Vec<_> = scope.mounts.iter().map(|p| Filesystem::new(p).unwrap()).collect();
            cpu.refresh().unwrap();
            memory.refresh().unwrap();
            let changes = processes.refresh().unwrap();
            network.refresh().unwrap();
            let mut failed = changes.failed;
            for fs in &mut filesystems {
                if fs.refresh().is_err() {
                    failed += 1;
                }
            }
            black_box((&cpu, &memory, &processes, &network, &filesystems));
            (changes.updated, failed)
        }
        (false, "basic") => {
            let mut c = sysinfo::System::new().unwrap();
            c.refresh_cpu_list(CpuRefreshKind::nothing());
            c.refresh_cpu_usage();
            c.refresh_memory();
            let count = c.refresh_processes_specifics(ProcessesToUpdate::All, true, process_kind(true, false, false));
            let mut network = sysinfo::Networks::new().unwrap();
            network.refresh(true);
            let mut d = disks(scope);
            let (_, failed) = sdisks(&mut d, &scope.mounts, false);
            black_box((&c, &network, &d));
            (count, failed)
        }
        _ => panic!("unsupported cold profile"),
    }
}
