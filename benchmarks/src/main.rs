//! Isolated-process benchmarks using only the libraries' public interfaces.
use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

struct Counting;
static CALLS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);
static PEAK: AtomicU64 = AtomicU64::new(0);
static OBSERVED_PROCESSES: AtomicU64 = AtomicU64::new(0);
fn allocated(size: usize) {
    CALLS.fetch_add(1, Relaxed);
    BYTES.fetch_add(size as u64, Relaxed);
    let now = LIVE.fetch_add(size as u64, Relaxed) + size as u64;
    PEAK.fetch_max(now, Relaxed);
}
// SAFETY: original pointer/layout arguments are forwarded to System exactly once.
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

struct Config {
    samples: usize,
    interval: Duration,
    cold: bool,
    initial_heap: u64,
}
fn measure(mut collect: impl FnMut(), config: &Config) {
    let warmup = if config.cold {
        0
    } else if config.interval.is_zero() {
        50
    } else {
        2
    };
    for _ in 0..warmup {
        if !config.interval.is_zero() {
            std::thread::sleep(config.interval);
        }
        collect();
    }
    let retained_before = LIVE.load(Relaxed).saturating_sub(config.initial_heap);
    let mut rows = Vec::with_capacity(config.samples);
    for _ in 0..config.samples {
        if !config.interval.is_zero() {
            std::thread::sleep(config.interval);
        }
        let live = LIVE.load(Relaxed);
        PEAK.store(live, Relaxed);
        let calls = CALLS.load(Relaxed);
        let bytes = BYTES.load(Relaxed);
        let start = Instant::now();
        collect();
        let ns = start.elapsed().as_nanos();
        rows.push((
            ns,
            CALLS.load(Relaxed) - calls,
            BYTES.load(Relaxed) - bytes,
            PEAK.load(Relaxed).saturating_sub(live),
            LIVE.load(Relaxed).saturating_sub(live),
            OBSERVED_PROCESSES.load(Relaxed),
        ));
    }
    let retained_after = LIVE
        .load(Relaxed)
        .saturating_sub(config.initial_heap)
        .saturating_sub((rows.capacity() * std::mem::size_of_val(&rows[0])) as u64);
    eprintln!("retained_heap_before_bytes={retained_before}");
    eprintln!("retained_heap_after_bytes={retained_after}");
    eprintln!("warmup={warmup}");
    eprintln!("interval_ns={}", config.interval.as_nanos());
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let rss = status.lines().find(|s| s.starts_with("VmHWM:")).unwrap_or("VmHWM: unavailable");
    eprintln!("{rss}");
    eprintln!("process_count={}", OBSERVED_PROCESSES.load(Relaxed));
    println!(
        "sample,ns,allocations,allocated_bytes,peak_extra_live_bytes,retained_extra_live_bytes,observed_processes"
    );
    for (i, (ns, calls, bytes, peak, retained, processes)) in rows.into_iter().enumerate() {
        println!("{i},{ns},{calls},{bytes},{peak},{retained},{processes}");
    }
}
fn process_kind() -> sysinfo::ProcessRefreshKind {
    sysinfo::ProcessRefreshKind::nothing().without_tasks().with_cpu().with_memory()
}
fn observe_zsys_processes(c: &zsys::linux::Processes) {
    assert!(c.failures().is_empty(), "partial process scan is not comparable");
    OBSERVED_PROCESSES.store(c.len() as u64, Relaxed);
    assert!(!c.is_empty(), "empty collection is not a successful measurement");
    for p in c.iter() {
        black_box((
            p.id,
            p.parent_pid,
            p.name.as_slice(),
            p.state,
            p.memory_bytes,
            p.counters.virtual_bytes,
            p.counters.user_ticks,
            p.counters.system_ticks,
        ));
    }
}
fn observe_sysinfo_processes(c: &sysinfo::System) {
    OBSERVED_PROCESSES.store(c.processes().len() as u64, Relaxed);
    assert!(!c.processes().is_empty(), "empty collection is not a successful measurement");
    for p in c.processes().values() {
        black_box((
            p.pid(),
            p.parent(),
            p.name(),
            p.status(),
            p.memory(),
            p.virtual_memory(),
            p.accumulated_cpu_time(),
            p.start_time(),
        ));
    }
}
fn zsys_processes(std_scan: bool) -> zsys::linux::Processes {
    let mut c = zsys::linux::Processes::new().unwrap();
    if std_scan {
        c.set_scan_mode(zsys::linux::ScanMode::Std);
    }
    c
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let implementation = args.get(1).expect("zsys or sysinfo");
    let case = args.get(2).map(String::as_str).unwrap_or("memory");
    let samples: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(1000);
    assert!((1..=1_000_000).contains(&samples));
    if case == "validate_process" {
        validate_process().unwrap();
        println!("process_scope_verified=true");
        return;
    }
    if case == "validate_network" {
        validate_network().unwrap();
        println!("network_scope_verified=true");
        return;
    }
    if case == "probe" {
        probe();
        return;
    }
    let cold = case.starts_with("cold_");
    let interval = if matches!(case, "cpu" | "basic" | "full") {
        sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(10)
    } else {
        Duration::ZERO
    };
    let config = Config { samples, interval, cold, initial_heap: LIVE.load(Relaxed) };
    match (implementation.as_str(), case) {
        ("zsys", "memory") => {
            let mut c = zsys::linux::MemoryCollector::new();
            measure(
                || {
                    let m = c.refresh().unwrap().counters;
                    assert!(m.total_bytes > 0 && m.available_bytes <= m.total_bytes);
                    black_box((
                        m.total_bytes,
                        m.available_bytes,
                        m.free_bytes,
                        m.used_bytes(),
                        m.swap_total_bytes,
                        m.swap_free_bytes,
                        m.swap_used_bytes(),
                    ));
                },
                &config,
            );
        }
        ("sysinfo", "memory") => {
            let mut c = sysinfo::System::new().unwrap();
            measure(
                || {
                    c.refresh_memory();
                    assert!(c.total_memory() > 0 && c.available_memory() <= c.total_memory());
                    black_box((
                        c.total_memory(),
                        c.available_memory(),
                        c.free_memory(),
                        c.used_memory(),
                        c.total_swap(),
                        c.free_swap(),
                        c.used_swap(),
                    ));
                },
                &config,
            );
        }
        ("zsys", "cpu" | "cpu_hot") => {
            let mut c = zsys::linux::CpuCollector::new();
            measure(
                || {
                    c.refresh().unwrap();
                    assert!(!c.cores().is_empty());
                    black_box((c.aggregate_busy_percent(), c.cores()));
                },
                &config,
            );
        }
        ("sysinfo", "cpu" | "cpu_hot") => {
            let mut c = sysinfo::System::new().unwrap();
            measure(
                || {
                    c.refresh_cpu_usage();
                    assert!(!c.cpus().is_empty());
                    black_box((c.global_cpu_usage(), c.cpus()));
                },
                &config,
            );
        }
        ("zsys", "process_single" | "cold_process_single") => {
            OBSERVED_PROCESSES.store(1, Relaxed);
            let pid = std::process::id();
            if cold {
                measure(
                    || {
                        let mut c = zsys_processes(false);
                        black_box(c.refresh_pid(pid).unwrap());
                    },
                    &config,
                );
            } else {
                let mut c = zsys_processes(false);
                measure(
                    || {
                        black_box(c.refresh_pid(pid).unwrap());
                    },
                    &config,
                );
            }
        }
        ("sysinfo", "process_single" | "cold_process_single") => {
            OBSERVED_PROCESSES.store(1, Relaxed);
            let pids = [sysinfo::Pid::from_u32(std::process::id())];
            if cold {
                measure(
                    || {
                        let mut c = sysinfo::System::new().unwrap();
                        c.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&pids), true, process_kind());
                        black_box(c.process(pids[0]).expect("self process"));
                    },
                    &config,
                );
            } else {
                let mut c = sysinfo::System::new().unwrap();
                measure(
                    || {
                        c.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&pids), true, process_kind());
                        black_box(c.process(pids[0]).expect("self process"));
                    },
                    &config,
                );
            }
        }
        ("zsys", "process_all" | "process_all_std" | "cold_process_all") => {
            if cold {
                measure(
                    || {
                        let mut c = zsys_processes(false);
                        c.refresh().unwrap();
                        observe_zsys_processes(&c);
                    },
                    &config,
                );
            } else {
                let mut c = zsys_processes(case.ends_with("_std"));
                measure(
                    || {
                        c.refresh().unwrap();
                        observe_zsys_processes(&c);
                    },
                    &config,
                );
            }
        }
        ("sysinfo", "process_all" | "cold_process_all") => {
            if cold {
                measure(
                    || {
                        let mut c = sysinfo::System::new().unwrap();
                        c.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, process_kind());
                        observe_sysinfo_processes(&c);
                    },
                    &config,
                );
            } else {
                let mut c = sysinfo::System::new().unwrap();
                measure(
                    || {
                        c.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, process_kind());
                        observe_sysinfo_processes(&c);
                    },
                    &config,
                );
            }
        }
        ("zsys", "process_inventory") => {
            let mut c = zsys_processes(false);
            measure(
                || {
                    c.refresh().unwrap();
                    assert!(!c.is_empty());
                    assert!(c.failures().is_empty());
                    OBSERVED_PROCESSES.store(c.len() as u64, Relaxed);
                    for p in c.iter() {
                        black_box((p.id.pid, p.parent_pid, &p.name, p.id.start_time_ticks));
                    }
                },
                &config,
            );
        }
        ("sysinfo", "process_inventory") => {
            let mut c = sysinfo::System::new().unwrap();
            measure(
                || {
                    c.refresh_processes_specifics(
                        sysinfo::ProcessesToUpdate::All,
                        true,
                        sysinfo::ProcessRefreshKind::nothing().without_tasks(),
                    );
                    assert!(!c.processes().is_empty());
                    OBSERVED_PROCESSES.store(c.processes().len() as u64, Relaxed);
                    for p in c.processes().values() {
                        black_box((p.pid(), p.parent(), p.name(), p.start_time()));
                    }
                },
                &config,
            );
        }
        ("zsys", "enumerate" | "enumerate_std") => {
            let mut c = zsys_processes(case.ends_with("_std"));
            measure(
                || {
                    let pids = c.enumerate_pids().unwrap();
                    OBSERVED_PROCESSES.store(pids.len() as u64, Relaxed);
                    black_box(pids);
                },
                &config,
            );
        }
        ("zsys", "network_counters") => {
            let mut c = zsys::linux::Networks::new();
            measure(
                || {
                    c.refresh_counters().unwrap();
                    assert!(c.iter().next().is_some());
                    black_box(c.iter().count());
                },
                &config,
            );
        }
        ("zsys", "network") => {
            let mut c = zsys::linux::Networks::new();
            measure(
                || {
                    c.refresh().unwrap();
                    assert!(c.iter().next().is_some());
                    for n in c.iter() {
                        black_box((&n.name, n.counters, &n.details));
                    }
                },
                &config,
            );
        }
        ("sysinfo", "network") => {
            let mut c = sysinfo::Networks::new().unwrap();
            measure(
                || {
                    c.refresh(true);
                    assert!(!c.is_empty());
                    for (name, n) in &c {
                        black_box((
                            name,
                            n.total_received(),
                            n.total_transmitted(),
                            n.total_packets_received(),
                            n.total_packets_transmitted(),
                            n.ip_networks(),
                            n.mac_address(),
                            n.mtu(),
                        ));
                    }
                },
                &config,
            );
        }
        ("zsys", "disk_io") => {
            let mut c = zsys::linux::DiskIoCollector::new();
            measure(
                || {
                    c.refresh().unwrap();
                    black_box(c.iter().count());
                },
                &config,
            );
        }
        ("zsys", "disk") => {
            let disks = sysinfo::Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing()).unwrap();
            let mut c: Vec<_> = disks.iter().map(|d| zsys::linux::Filesystem::new(d.mount_point()).unwrap()).collect();
            assert!(!c.is_empty(), "no sysinfo-visible mount points");
            eprintln!("mount_count={}", c.len());
            for d in &c {
                eprintln!("mount={}", d.path().display());
            }
            drop(disks);
            measure(
                || {
                    for d in &mut c {
                        black_box(d.refresh().unwrap());
                    }
                },
                &config,
            );
        }
        ("sysinfo", "disk") => {
            let mut c = sysinfo::Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing()).unwrap();
            assert!(!c.is_empty(), "no sysinfo-visible mount points");
            eprintln!("mount_count={}", c.len());
            for d in &c {
                eprintln!("mount={}", d.mount_point().display());
            }
            measure(
                || {
                    for d in c.iter_mut() {
                        assert!(d.refresh_specifics(sysinfo::DiskRefreshKind::nothing().with_storage()));
                        black_box((d.total_space(), d.available_space()));
                    }
                },
                &config,
            );
        }
        ("zsys", "basic" | "basic_hot" | "cold_basic") => {
            if cold {
                measure(
                    || {
                        let mut cpu = zsys::linux::CpuCollector::new();
                        let mut memory = zsys::linux::MemoryCollector::new();
                        let mut processes = zsys_processes(false);
                        cpu.refresh().unwrap();
                        memory.refresh().unwrap();
                        processes.refresh().unwrap();
                        black_box((cpu.cores(), memory.snapshot()));
                        observe_zsys_processes(&processes);
                    },
                    &config,
                );
            } else {
                let mut cpu = zsys::linux::CpuCollector::new();
                let mut memory = zsys::linux::MemoryCollector::new();
                let mut processes = zsys_processes(false);
                measure(
                    || {
                        cpu.refresh().unwrap();
                        memory.refresh().unwrap();
                        processes.refresh().unwrap();
                        black_box((cpu.cores(), memory.snapshot()));
                        observe_zsys_processes(&processes);
                    },
                    &config,
                );
            }
        }
        ("sysinfo", "basic" | "basic_hot" | "cold_basic") => {
            if cold {
                measure(
                    || {
                        let mut c = sysinfo::System::new().unwrap();
                        c.refresh_cpu_usage();
                        c.refresh_memory();
                        c.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, process_kind());
                        black_box((
                            c.cpus(),
                            c.total_memory(),
                            c.available_memory(),
                            c.free_memory(),
                            c.used_memory(),
                            c.total_swap(),
                            c.free_swap(),
                            c.used_swap(),
                        ));
                        observe_sysinfo_processes(&c);
                    },
                    &config,
                );
            } else {
                let mut c = sysinfo::System::new().unwrap();
                measure(
                    || {
                        c.refresh_cpu_usage();
                        c.refresh_memory();
                        c.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, process_kind());
                        black_box((
                            c.cpus(),
                            c.total_memory(),
                            c.available_memory(),
                            c.free_memory(),
                            c.used_memory(),
                            c.total_swap(),
                            c.free_swap(),
                            c.used_swap(),
                        ));
                        observe_sysinfo_processes(&c);
                    },
                    &config,
                );
            }
        }
        ("zsys", "full") => {
            let mut cpu = zsys::linux::CpuCollector::new();
            let mut memory = zsys::linux::MemoryCollector::new();
            let mut processes = zsys_processes(false);
            let mut networks = zsys::linux::Networks::new();
            let disks = sysinfo::Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing()).unwrap();
            let mut filesystems: Vec<_> =
                disks.iter().map(|d| zsys::linux::Filesystem::new(d.mount_point()).unwrap()).collect();
            assert!(!filesystems.is_empty());
            eprintln!("mount_count={}", filesystems.len());
            for d in &filesystems {
                eprintln!("mount={}", d.path().display());
            }
            drop(disks);
            measure(
                || {
                    cpu.refresh().unwrap();
                    memory.refresh().unwrap();
                    processes.refresh().unwrap();
                    networks.refresh().unwrap();
                    assert!(networks.iter().next().is_some());
                    for d in &mut filesystems {
                        black_box(d.refresh().unwrap());
                    }
                    for n in networks.iter() {
                        black_box((&n.name, n.counters, &n.details));
                    }
                    black_box((cpu.cores(), memory.snapshot()));
                    observe_zsys_processes(&processes);
                },
                &config,
            );
        }
        ("sysinfo", "full") => {
            let mut c = sysinfo::System::new().unwrap();
            let mut networks = sysinfo::Networks::new().unwrap();
            let mut disks =
                sysinfo::Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing()).unwrap();
            assert!(!disks.is_empty());
            eprintln!("mount_count={}", disks.len());
            for d in &disks {
                eprintln!("mount={}", d.mount_point().display());
            }
            measure(
                || {
                    c.refresh_cpu_usage();
                    c.refresh_memory();
                    c.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, process_kind());
                    networks.refresh(true);
                    assert!(!networks.is_empty());
                    for d in disks.iter_mut() {
                        assert!(d.refresh_specifics(sysinfo::DiskRefreshKind::nothing().with_storage()));
                        black_box((d.total_space(), d.available_space()));
                    }
                    for (name, n) in &networks {
                        black_box((
                            name,
                            n.total_received(),
                            n.total_transmitted(),
                            n.total_packets_received(),
                            n.total_packets_transmitted(),
                            n.ip_networks(),
                            n.mac_address(),
                            n.mtu(),
                        ));
                    }
                    black_box((
                        c.cpus(),
                        c.total_memory(),
                        c.available_memory(),
                        c.free_memory(),
                        c.used_memory(),
                        c.total_swap(),
                        c.free_swap(),
                        c.used_swap(),
                    ));
                    observe_sysinfo_processes(&c);
                },
                &config,
            );
        }
        _ => panic!("unsupported benchmark: {implementation} {case}"),
    }
}
fn probe() {
    println!("minimum_cpu_interval_ns={}", sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.as_nanos());
    let mut s = sysinfo::System::new().unwrap();
    s.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, process_kind());
    println!("process_count={}", s.processes().len());
    match sysinfo::Networks::new_with_refreshed_list() {
        Ok(n) => {
            for (name, data) in &n {
                println!(
                    "network={name:?} ip={:?} mac={} mtu={} rx={:?} tx={:?}",
                    data.ip_networks(),
                    data.mac_address(),
                    data.mtu(),
                    data.receive_link_speed(),
                    data.transmit_link_speed()
                );
            }
        }
        Err(e) => println!("network_error={e:?}"),
    }
    match sysinfo::Disks::new_with_refreshed_list_specifics(sysinfo::DiskRefreshKind::nothing().with_storage()) {
        Ok(d) => {
            for disk in &d {
                println!("mount={:?} type={:?} total={}", disk.mount_point(), disk.file_system(), disk.total_space());
            }
        }
        Err(e) => println!("disk_error={e:?}"),
    }
}

/// A speed ratio is permitted only after the metadata and interface sets match.
fn validate_network() -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let mut reference = sysinfo::Networks::new_with_refreshed_list().map_err(|e| e.to_string())?;
    if reference.is_empty() {
        return Err("sysinfo returned no interfaces; empty collection is not comparable".into());
    }
    let mut candidate = zsys::linux::Networks::new();
    candidate.refresh().map_err(|e| e.to_string())?;
    let mut before: Vec<_> = reference
        .iter()
        .map(|(name, n)| (name.as_bytes().to_vec(), n.total_received(), n.total_transmitted()))
        .collect();
    before.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    reference.refresh(true);
    let mut candidate_names: Vec<_> = candidate.iter().map(|n| n.name.as_slice()).collect();
    let mut reference_names: Vec<_> = reference.keys().map(|name| name.as_bytes()).collect();
    candidate_names.sort_unstable();
    reference_names.sort_unstable();
    if candidate_names != reference_names {
        return Err("network interface sets differ".into());
    }
    for (name, n) in &reference {
        let c = candidate.get(name.as_bytes()).ok_or("missing interface")?;
        let details = c.details.as_ref().ok_or("missing interface details")?;
        if details.mac_address != Some(n.mac_address().0) || details.mtu != Some(n.mtu()) {
            return Err(format!("MAC/MTU differs for {name:?}"));
        }
        let prefix = |mask: std::net::IpAddr| -> u8 {
            match mask {
                std::net::IpAddr::V4(v) => u32::from(v).count_ones() as u8,
                std::net::IpAddr::V6(v) => u128::from(v).count_ones() as u8,
            }
        };
        let mut ca: Vec<_> = details.addresses.iter().map(|a| (a.address, a.netmask.map(prefix))).collect();
        let mut ra: Vec<_> = n.ip_networks().iter().map(|a| (a.addr, Some(a.prefix))).collect();
        ca.sort_unstable();
        ca.dedup();
        ra.sort_unstable();
        ra.dedup();
        if ca != ra {
            return Err(format!("IP/netmask sets differ for {name:?}: zsys={ca:?}, sysinfo={ra:?}"));
        }
        for speed in [n.receive_link_speed(), n.transmit_link_speed()] {
            if speed != details.link_speed_bits_per_second {
                return Err(format!("link speed differs for {name:?}"));
            }
        }
        let old = before.iter().find(|a| a.0 == name.as_bytes()).ok_or("interface appeared during validation")?;
        if !(old.1..=n.total_received()).contains(&c.counters.received_bytes)
            || !(old.2..=n.total_transmitted()).contains(&c.counters.transmitted_bytes)
        {
            return Err(format!("counters outside bracketing sysinfo samples for {name:?}"));
        }
    }
    Ok(())
}

fn validate_process() -> Result<(), String> {
    use std::collections::BTreeSet;
    use std::os::unix::ffi::OsStrExt;
    let mut reference = sysinfo::System::new().map_err(|e| e.to_string())?;
    let mut candidate = zsys::linux::Processes::new().map_err(|e| e.to_string())?;
    reference.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, process_kind());
    candidate.refresh().map_err(|e| e.to_string())?;
    let a: BTreeSet<_> = reference.processes().keys().map(|pid| pid.as_u32()).collect();
    let b: BTreeSet<_> = candidate.iter().map(|p| p.id.pid).collect();
    if a != b || !candidate.failures().is_empty() {
        return Err(format!(
            "process inventories differ: reference_only={:?}, candidate_only={:?}, failures={:?}",
            a.difference(&b),
            b.difference(&a),
            candidate.failures()
        ));
    }
    println!("verified_process_count={}", a.len());
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let child = ChildGuard(std::process::Command::new("/bin/sleep").arg("60").spawn().map_err(|e| e.to_string())?);
    std::thread::sleep(Duration::from_millis(30));
    let id = sysinfo::Pid::from_u32(child.0.id());
    reference.refresh_processes_specifics(sysinfo::ProcessesToUpdate::Some(&[id]), true, process_kind());
    let c = candidate.refresh_pid(child.0.id()).map_err(|e| e.to_string())?.clone();
    let r = reference.process(id).ok_or("controlled process missing from sysinfo")?;
    if c.id.pid != r.pid().as_u32()
        || Some(c.parent_pid) != r.parent().map(|p| p.as_u32())
        || c.name != r.name().as_bytes()
        || c.counters.virtual_bytes != r.virtual_memory()
        || c.memory_bytes != r.memory()
    {
        return Err(format!(
            "controlled process common fields differ: zsys=pid{} ppid{} name{:?} virtual{} rss{}; sysinfo=pid{} ppid{:?} name{:?} virtual{} rss{}",
            c.id.pid,
            c.parent_pid,
            c.name,
            c.counters.virtual_bytes,
            c.memory_bytes,
            r.pid(),
            r.parent(),
            r.name(),
            r.virtual_memory(),
            r.memory()
        ));
    }
    let ticks = u128::from(c.counters.user_ticks) + u128::from(c.counters.system_ticks);
    let millis = ticks * 1000 / u128::from(candidate.ticks_per_second());
    if millis.abs_diff(u128::from(r.accumulated_cpu_time())) > (1000 / candidate.ticks_per_second()).max(1) as u128 {
        return Err("CPU milliseconds differ outside one tick tolerance".into());
    }
    let boot = sysinfo::System::boot_time().map_err(|e| e.to_string())?;
    if (boot + c.id.start_time_ticks / candidate.ticks_per_second()).abs_diff(r.start_time()) > 1 {
        return Err("process start time differs outside one-second conversion tolerance".into());
    }
    Ok(())
}
