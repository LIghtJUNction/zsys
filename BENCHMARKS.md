# Benchmark protocol

Fixed comparison target: sysinfo 0.40.3. No performance superiority is claimed until both libraries collect equivalent fields on the same host and workload.

Record CPU model, memory, kernel/distribution, Rust version, compiler flags, feature flags, process count, and sampling configuration. Report mean, median, P95, P99, maximum, throughput, allocation count/bytes, and peak RSS.

Required workloads: CPU, memory, network, disk, one process, full enumeration, full refresh, combined refresh, cold start, and repeated hot-path sampling. Test 100/500/1,000/5,000/10,000 processes where available.

No measured comparison data exists yet. Target figures in the project brief are goals, not results.
