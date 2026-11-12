//! One library/profile per process. Timings never include sleeps or printing.
#[cfg(feature = "count-alloc")]
mod counting;
mod workloads;
use std::os::unix::ffi::OsStringExt;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};
use workloads::Scope;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "scope") {
        workloads::discover();
        return;
    }
    let implementation = args.get(1).expect("zsys, zsys-std or sysinfo");
    assert!(["zsys", "zsys-std", "sysinfo"].contains(&implementation.as_str()));
    let case = args.get(2).expect("profile");
    let samples: usize = args.get(3).expect("sample count").parse().unwrap();
    assert!((1..=1_000_000).contains(&samples));
    let scope_path = args.get(4).expect("scope TSV file");
    let mut scope =
        Scope { pid: args.get(5).map(|s| s.parse().unwrap()).unwrap_or(std::process::id()), ..Scope::default() };
    for line in std::fs::read_to_string(scope_path).unwrap().lines() {
        let (kind, value) = line.split_once('\t').expect("scope record");
        match kind {
            "mount" => {
                assert!(value.len() % 2 == 0 && value.is_ascii());
                let bytes: Vec<_> =
                    (0..value.len()).step_by(2).map(|i| u8::from_str_radix(&value[i..i + 2], 16).unwrap()).collect();
                scope.mounts.push(std::ffi::OsString::from_vec(bytes).into());
            }
            "device" => {
                let (major, minor) = value.split_once(':').unwrap();
                let id = (major.parse().unwrap(), minor.parse().unwrap());
                if !scope.devices.contains(&id) {
                    scope.devices.push(id);
                }
            }
            _ => panic!("unknown scope record"),
        }
    }
    scope.mounts.sort();
    scope.mounts.dedup();
    let cold = case.starts_with("cold-");
    let profile = case.strip_prefix("cold-").unwrap_or(case);
    // sysinfo may throttle CPU updates. Pace BOTH implementations outside timing.
    let paced = !cold && (profile == "cpu" || profile == "basic" || profile.ends_with("-cpu"));
    let interval = if paced { sysinfo::MINIMUM_CPU_UPDATE_INTERVAL + Duration::from_millis(5) } else { Duration::ZERO };
    let mut collect: workloads::Task = if cold {
        let implementation = implementation.clone();
        let profile = profile.to_string();
        Box::new(move || workloads::cold(&implementation, &profile, &scope))
    } else {
        workloads::task(implementation, profile, &scope)
    };
    let warmup = if cold {
        0
    } else if paced {
        2
    } else {
        50
    };
    for _ in 0..warmup {
        black_box(collect());
        if paced {
            std::thread::sleep(interval);
        }
    }
    let mut rows = Vec::with_capacity(samples);
    for _ in 0..samples {
        if paced {
            std::thread::sleep(interval);
        }
        #[cfg(feature = "count-alloc")]
        let before = counting::start();
        let start = Instant::now();
        let (units, failed) = black_box(collect());
        let ns = start.elapsed().as_nanos();
        #[cfg(feature = "count-alloc")]
        let (calls, bytes, peak) = counting::finish(before);
        #[cfg(not(feature = "count-alloc"))]
        let (calls, bytes, peak) = (0, 0, 0);
        assert!(units > 0, "empty work is not a benchmark result");
        rows.push((ns, calls, bytes, peak, units, failed));
    }
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    eprintln!("{}", status.lines().find(|s| s.starts_with("VmHWM:")).unwrap_or("VmHWM: unavailable"));
    eprintln!("instrumented={} interval_ns={} warmup={warmup}", cfg!(feature = "count-alloc"), interval.as_nanos());
    println!("sample,ns,allocations,allocated_bytes,peak_extra_live_bytes,units,failed");
    for (i, (ns, calls, bytes, peak, units, failed)) in rows.into_iter().enumerate() {
        println!("{i},{ns},{calls},{bytes},{peak},{units},{failed}");
    }
}
