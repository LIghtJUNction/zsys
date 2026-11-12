# zsys

A low-overhead, incremental system information engine for Rust.

**Status: early Linux foundation.** This first milestone establishes reusable procfs reads, allocation-free parsing, core counter types, and a correct cumulative CPU-delta model. Full process, disk, network, and hardware collection is not implemented yet.

## Principles

- Zero third-party runtime dependencies.
- Core types compile with no_std; alloc is an optional capability.
- Linux OS access is isolated behind std and a Linux target gate.
- CPU utilization is computed from differences between cumulative kernel counters, not refresh-call duration.
- Process identity includes PID plus process start time to detect PID reuse.

## Example

~~~rust
let mut collector = zsys::linux::LinuxCollector::new();
let memory = collector.memory()?;
let cpu = collector.cpu_times()?;
# Ok::<(), zsys::linux::LinuxError>(())
~~~

## Checks

~~~sh
cargo test
cargo check --no-default-features
cargo clippy --all-targets --all-features -- -D warnings
~~~

See BENCHMARKS.md for the comparison protocol. No performance advantage is claimed without reproducible measurements.
