# zsys

A dependency-free Rust system-information engine with independent, incremental Linux collectors. It is designed for repeated sampling, explicit freshness and predictable storage reuse, not sysinfo API compatibility.

**Scope:** the Linux backend implements CPU, memory, processes, network, mounted filesystem capacity and block I/O. Optional read-only sysfs collectors expose temperature sensors, GPU inventory/driver-exported readings and non-identifying DMI information. Windows and macOS currently expose the portable core only; a successful cross-platform build does **not** mean a native collector exists. The pre-1.0 API can evolve.

## Use

```toml
[dependencies]
zsys = "0.1"
```

```rust
# #[cfg(all(feature = "std", target_os = "linux"))]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
let mut system = zsys::System::new(); // no I/O and no collector allocation
system.refresh_cpu()?;
system.refresh_memory()?;
system.refresh_processes()?;

let memory = system.memory().expect("refreshed");
println!("{} / {} bytes", memory.counters.used_bytes(), memory.counters.total_bytes);
for process in system.processes() {
    println!("{} {:?} {} bytes {:?}", process.id.pid, process.name,
             process.memory_bytes, process.availability);
}
# Ok(())
# }
# #[cfg(not(all(feature = "std", target_os = "linux")))]
# fn main() {}
```

CPU utilization needs two successful observations. The first sample is `None`, not a fabricated zero. The library never sleeps or silently skips a requested refresh. The caller chooses the interval; `sampling::Cadence` is an optional no-thread scheduling primitive. See `examples/basic.rs` for a two-sample example.

Collectors are independently usable: `CpuCollector`, `MemoryCollector`, `Processes`, `Networks`, `DiskIoCollector`, `Mounts` and `Filesystem`. `System` lazily composes them. `Networks::refresh_counters()` does not fetch addresses or link metadata; `refresh()` does both. Mount discovery never silently samples every remote filesystem, because a remote `statvfs` call can block. Select individual `Filesystem` objects for high-frequency capacity sampling.

Process enumeration and details are separate. `Processes::enumerate_pids()` returns a borrowed sorted PID list without reading stat records. `refresh_pid(pid)` only updates that PID; `refresh()` reconciles the complete visible table. Expensive metadata is explicit:

```rust
# #[cfg(all(feature = "std", target_os = "linux"))]
# fn main() -> Result<(), Box<dyn std::error::Error>> {
use zsys::linux::{Detail, DetailOptions, Processes};
let mut processes = Processes::new()?;
let pid = std::process::id();
processes.refresh_pid(pid)?;
let details = processes.details(pid, DetailOptions::all())?;
if let Detail::Value(arguments) = details.command_line {
    // Raw NUL-separated bytes preserve empty and non-UTF-8 arguments.
    println!("{} argument bytes", arguments.len());
}
# Ok(())
# }
# #[cfg(not(all(feature = "std", target_os = "linux")))]
# fn main() {}
```

## Sampling contract

Counters and parsers use `core`; no allocator is required. The `std` feature enables Linux I/O and implies `alloc`. There are **zero third-party normal/runtime crate dependencies**. Native Linux C ABI calls use the system C runtime, not a Rust `libc` crate. The isolated benchmark workspace may use development-only dependencies.

```rust
use zsys::cpu::CpuTimes;
let before = CpuTimes { user: 100, idle: 900, ..CpuTimes::default() };
let after = CpuTimes { user: 150, idle: 950, ..CpuTimes::default() };
assert_eq!(after.busy_percent_since(before), Some(50.0));
```

CPU percentages use cumulative counter differences, never the refresh function's execution duration. Process percentages use CPU ticks divided by elapsed time **between successful observations**, with one logical CPU equal to 100%; multithreaded processes can exceed 100%. PID identity is `(pid, start_time_ticks)`. Reuse, stale observations, counter rollback and zero intervals invalidate baselines. Process start time is boot-relative, not a Unix timestamp.

Values are not an atomic system-wide snapshot. Every collector owns a monotonic clock origin and sample sequence; compare timestamps only within that collector. A failed CPU/memory/counter parse preserves its last successful snapshot. Process failures retain explicitly stale records and report their reasons. Filesystem errors preserve capacity with an explicit `last_error`. Optional hardware readings instead store the current `Value`, `Unavailable` or `Failed` state. Inspect freshness/error metadata before using values.

## Platform and capability boundaries

| Capability | Current support |
| --- | --- |
| Portable models, counter arithmetic, parsers, cadence | `no_std`, with optional `alloc`; no OS calls |
| Linux CPU/memory/process/network | Native collection; fixtures and live smoke tests |
| Linux PID directory fast path | x86-64 and AArch64 LP64 `getdents64`; standard-library alternative |
| Filesystem capacity | Linux LP64 glibc/musl `statvfs`; other widths report unsupported |
| Thermal | hwmon `tempN_input` and optional labels/critical values; no calibration or control writes |
| GPU | DRM card inventory, vendor/device IDs; utilization/VRAM only where the driver exports the sysfs attributes |
| Hardware inventory | DMI vendor/product/BIOS version; no serial numbers or machine UUIDs |
| Windows/macOS collectors; FreeBSD/Android/iOS | Not implemented; do not infer support from core compilation |

Full NVIDIA/Intel/AMD vendor telemetry, universal GPU process accounting, driver control and hard-real-time latency guarantees are not provided. A sensor's kernel label is not proof of its physical placement. See [API semantics](https://github.com/LIghtJUNction/zsys/blob/main/docs/API.md) for failure and identity details.

## Validate and benchmark

```sh
cargo test --locked --all-features
cargo test --locked --no-default-features
cargo test --locked --no-default-features --features alloc
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
python3 -m unittest discover -s scripts -p 'test_*.py'
python3 benchmarks/run.py --output benchmark-results
```

The isolated benchmark pins **sysinfo 0.40.3**. Latency and allocation instrumentation run in separate binaries. CPU comparisons respect the competitor's minimum sampling interval for both libraries; no cached no-op is presented as fresh sampling. Shared field contracts, cold/hot definitions, process-scale safety limits and unpaired profiles are documented in [the protocol](https://github.com/LIghtJUNction/zsys/blob/main/benchmarks/PROTOCOL.md). [BENCHMARKS.md](https://github.com/LIghtJUNction/zsys/blob/main/BENCHMARKS.md) reports measured results and limitations, not target numbers disguised as observations.

## Release

The Release workflow supports `none`, `patch`, `minor` and `major`. `none` publishes the checked-in initial version. Use the GitHub Secret **`CARGO_REGISTY_TOKEN`** as requested; the conventional `CARGO_REGISTRY_TOKEN` Secret is also accepted. Both are mapped to Cargo's official `CARGO_REGISTRY_TOKEN` environment variable only in credential-requiring steps.

A release validates the CI matrix, updates both lockfiles and the manifest, generates a changelog, commits clean source, packages/publishes, verifies the registry archive checksum, then creates the matching tag and GitHub Release. No token belongs in a tracked file. A maintainer can also commit `.release/request.json` on `main` containing `{"bump":"none","base":"<parent commit SHA>"}`; its parent lease must match. Release failures are not reported as successful publication.

Licensed under MIT OR Apache-2.0.
