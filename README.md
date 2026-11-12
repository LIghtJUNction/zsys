# zsys

A composable system-information engine for Rust, with **zero third-party runtime dependencies**. Collect only the information you need, retain sampling state, and read snapshots through borrowed views.

The current native backend is **Linux**. CPU, memory, processes, filesystems, block I/O, networks, and optional sysfs hardware information are implemented. Windows, macOS, and other targets currently expose the portable counter models and parsers; their native collectors remain future work. See [platform support](docs/platforms.md).

## Run it

From a checkout, with Rust **1.95 or newer**:

```sh
cargo run --release --example basic
cargo run --release --example cadence
```

For a local application, add `zsys = { path = "../zsys" }` under `[dependencies]`. Published versions and release artifacts are listed on [crates.io](https://crates.io/crates/zsys) and [GitHub Releases](https://github.com/LIghtJUNction/zsys/releases).

```rust
use std::time::Duration;
use zsys::System;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut system = System::new();
    system.refresh_cpu()?;
    system.refresh_memory()?;
    system.refresh_processes()?;

    std::thread::sleep(Duration::from_millis(250));
    system.refresh_cpu()?;
    system.refresh_processes()?;

    println!("CPU: {:?}%", system.cpu().and_then(|c| c.aggregate_busy_percent()));
    if let Some(memory) = system.memory() {
        println!("Available memory: {} bytes", memory.counters.available_bytes);
    }
    for process in system.processes() {
        println!(
            "{} {} RSS={} CPU={:?}% {:?}",
            process.id.pid,
            String::from_utf8_lossy(&process.name),
            process.memory_bytes,
            process.cpu_percent,
            process.availability,
        );
    }
    Ok(())
}
```

The first CPU observation establishes a baseline and reports `None` for usage. The sleep belongs to the application: collectors create no background threads and impose no hidden refresh interval. Detailed timing, error, and counter semantics are documented in [sampling semantics](docs/semantics.md).

## Choose a collector

Each type below can be used without constructing `System`. Linux collectors live in `zsys::linux`.

| Collector | Explicit operations | Returned information |
|---|---|---|
| `CpuCollector` | `refresh`, `refresh_frequencies` | Aggregate and online logical CPU counters, utilization, optional scaling frequency |
| `MemoryCollector` | `refresh`, `snapshot` | RAM and swap counters, freshness, legacy availability-estimate flag |
| `Processes` | `enumerate_pids`, `refresh`, `refresh_pid`, `details` | Retained PID table, identity, lifecycle changes, CPU/RSS, optional executable/arguments/UIDs/GIDs |
| `DiskIoCollector` | `refresh`, `iter`, `get` | Device and partition counters, byte deltas, I/O timings |
| `Filesystem` | `refresh`, `capacity`, `last_error` | Capacity and inode counters for one path |
| `Mounts` | `refresh`, `iter_mut`, `refresh_capacities` | Mount discovery, decoded paths, filesystem types, explicitly requested capacities |
| `Networks` | `refresh_counters`, `refresh_details`, `refresh` | Traffic counters; separately refreshed addresses, flags, MAC, MTU, optional link speed |
| `Hardware` | `refresh_inventory`, `refresh_info`, `refresh_temperatures`, `refresh_gpus` | DMI descriptions, thermal/hwmon readings, DRM identification and available driver telemetry |

For example, process details are an independent operation:

```rust
use zsys::linux::{Detail, DetailOptions, Processes};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut processes = Processes::new()?;
    let pid = std::process::id();
    let identity = processes.refresh_pid(pid)?.id;
    let details = processes.details(pid, DetailOptions::all())?;
    if details.id == identity {
        if let Detail::Value(path) = details.executable {
            println!("{}", path.display());
        }
    }
    Ok(())
}
```

`System` lazily owns the CPU, memory, process, network, disk-I/O, and mount collectors. Its `refresh_basic` samples CPU, RAM, processes, network counters, and block I/O in sequence. Each module keeps its own freshness; a combined refresh is not an atomic kernel snapshot. Hardware and potentially blocking filesystem-capacity queries remain explicit.

Collectors reuse retained storage as the sampling workload stabilizes. New objects, larger files or fields, discovery, and owned detail results can allocate. Retained capacity reflects earlier workload peaks until the collector is dropped; allocation measurements must state their warm-up and refresh scope.

## Portable core

```toml
[dependencies]
zsys = { path = "../zsys", default-features = false }
```

| Feature | Meaning |
|---|---|
| No features | `no_std` counter types, delta calculations, procfs parsers, `Sample`, `SystemSnapshot`, and `Cadence`; no allocator required |
| `alloc` | `no_std` configuration with the allocation capability enabled; it does not enable OS access or add separate collectors |
| `std` — default | Enables `alloc` and, on Linux, native collectors |

Core APIs include `cpu::CpuTimes`, `memory::Memory`, `process::{ProcessId, ProcessCounters, ProcessState}`, `disk` and `network` counter/parser types, and `sampling::Cadence`. Parsing operates on caller-provided bytes; the core does not open files. Generate the complete API reference with `cargo doc --no-deps --all-features --open`.

## Measure before making a claim

[**BENCHMARKS.md**](BENCHMARKS.md) contains the comparison scope, measurements, raw-data locations, and limitations. The independent [benchmark package](benchmarks/) pins **sysinfo `=0.40.3`** as a benchmark dependency. The library itself does not depend on sysinfo, libc, Rayon, or a procfs wrapper crate.

```sh
cargo build --locked --release --manifest-path benchmarks/Cargo.toml
python3 benchmarks/run.py --output benchmark-results/local --samples 1000
```

Use a new output directory for each run. The harness retains raw samples, environment metadata, distribution statistics, Rust allocation accounting, and process peak RSS. CPU samples with a real sampling interval and rapid calls that may return cached sysinfo values are separate cases. Large-process tests depend on the host's PID and memory limits; consult the report for the scales actually executed.

## Validation and release

```sh
cargo fmt --all -- --check
cargo test --locked --all-features
cargo test --locked --no-default-features
cargo check --locked --no-default-features --features alloc
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo doc --locked --no-deps --all-features
python3 scripts/check-package.py
```

The Python scripts require Python 3.11 or newer. [CI](.github/workflows/ci.yml) defines stable/MSRV checks, bare-metal `no_std`, WASM core, Linux x86_64/aarch64, glibc/musl distributions, Windows/macOS core builds, native C-ABI checks, and benchmark builds. [Resource soak](.github/workflows/soak.yml) keeps a process collector alive for the requested duration. Current execution results belong to [Actions](https://github.com/LIghtJUNction/zsys/actions), not to a static support claim.

The [Release workflow](.github/workflows/release.yml) runs on `main`. Manual dispatch accepts `none`, `patch`, `minor`, or `major`; changes to the package manifest or release workflow on `main` also start validation for the checked-in version. Configure repository secret **`CARGO_REGISTRY_TOKEN`**; the workflow accepts **`CARGO_REGISTY_TOKEN`** as a fallback and passes the selected value to Cargo under the canonical name. Tokens are never stored in source files.

Release preparation synchronizes the crate version in both lockfiles without upgrading benchmark dependencies, then runs the full CI matrix for the exact candidate commit. A preparation retry reuses an already pushed bump commit only when its parent and file tree match the candidate. Publishing checks the crate checksum in the registry before creating its matching `vVERSION` tag and GitHub Release with generated notes. To recover a failed publication, rerun the failed jobs so they reuse the prepared commit; dispatch `bump=none` only when the checked-in version is the intended candidate.

## Read further

- [Sampling semantics and complete API map](docs/semantics.md)
- [Platform support and native-interface boundaries](docs/platforms.md)
- [Architecture and evidence](docs/architecture.md)
- [Benchmarks](BENCHMARKS.md)
- [Progress and acceptance record](docs/progress.md)
- [Changelog](CHANGELOG.md)

Licensed under MIT OR Apache-2.0.
