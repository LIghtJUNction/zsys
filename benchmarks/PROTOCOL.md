# Measurement contract

The competitor is pinned to **sysinfo 0.40.3** in this independent workspace. The zsys crate has no normal dependencies. Only public sysinfo APIs are used. A result is evidence for the specified profile and host, not a universal speed guarantee.

## Common requested output

| Profile | Scope and requested fields |
| --- | --- |
| memory | Host total, free, available, used-by-difference, swap total/free/used. |
| cpu | Aggregate and every online logical CPU utilization; no frequency or per-task collection. |
| process-list | PID, parent PID, name, state, start time for all visible processes. |
| process-one / process-all | The preceding fields plus resident and virtual memory, for one fixed owned PID / all visible PIDs. |
| process-one-cpu / process-all-cpu | The corresponding memory profile plus CPU usage. |
| process-details | One fixed PID, memory profile plus refreshed command line, executable, user/group IDs. No environment, cwd, root directory, task enumeration or process disk I/O. |
| network | Every visible interface: traffic bytes/packets/errors, IP addresses/netmasks, link address, MTU, operational state and link speed when available. |
| disk-space | Total and available space for exactly the same discovered mount points. |
| disk-io | Read/written bytes for exactly the same accessible mounted block devices, deduplicated by major/minor. |
| basic | CPU, memory, all-process memory+CPU, full network and the same mount capacities. This is intentionally broader than `System::refresh_basic()` and does not time that convenience method. |

The public APIs do not expose identical internal work. zsys additionally parses thread counts and raw CPU ticks with process stat records and more network counters; sysinfo may refresh internal metadata. These extras are not removed just to win. RSS is each library's documented procfs estimate, not an exact shared-memory census. zsys reads all diskstats before selecting common device IDs. sysinfo's tasks feature is explicitly disabled for every process profile. CPU samples are separated by sysinfo's minimum update interval plus 5 ms **for both libraries**, outside measured execution. First samples warm up the baseline and are never reported as valid utilization deltas.

`enumerate`, `network-counters` and `disk-io-all` are separate, **unmatched** profiles: sysinfo does not expose the corresponding PID-only/counter-only/all-block-device collection contract. Never compute cross-library speedups for these. `zsys-std` is an ablation using standard-library directory enumeration with the same zsys parsing and output. Native `getdents64` is not assumed to be faster.

## Timing and allocation isolation

Each implementation/profile/repetition runs in a fresh child process. Three repetitions alternate implementation order. The latency binary uses the normal allocator without allocation-counter atomics. A separate `count-alloc` binary measures successful Rust allocation/reallocation requests, requested bytes, and the maximum additional live requested bytes during an operation. Native C allocator calls are **not** counted; in particular, `getifaddrs` can allocate independently. An uninstrumented zero in a raw allocation column means **not measured**, not zero allocations.

Both binaries exclude printing, sample-vector growth and pacing sleeps from the timed region. Cold profiles construct collectors and take one sample per iteration, then drop them; they exclude task-wrapper creation. "Cold" means fresh library state, **not** flushed kernel/page caches or a reboot. Cold CPU baselines do not have meaningful utilization. `VmHWM` is measured per fresh latency child and includes library state, binary/runtime and harness/sample storage. It is not the library's exclusive heap size. Instrumented timings are retained for auditing but must not be used as headline latency comparisons.

Raw rows, stderr, binary hashes, compiler/kernel/hardware/cgroup details, scope, source state, failure counts, summaries and pair-count validation are retained. Percentiles use nearest rank; small-sample P99 estimates are explicitly weak. A pair whose observed unit counts differ, or which has failures, must be flagged rather than presented as strictly equivalent work. Scope discovery is outside hot timing, and empty disk/device scopes are skipped rather than timed as zero work.

## Process scales and reproduction

Run `python3 benchmarks/run.py --output benchmark-results`. The default requests 100, 500, 1000, 5000 and 10000 visible processes. Additional processes are owned `sleep` children and are terminated/reaped in a `finally` block. The launcher reserves 64 PID slots and budgets at most half of available memory at 2 MiB per additional child. Denied or unsafe scales are recorded as skips, never relabeled as completed. Actual visible counts can differ slightly because benchmark/helper processes exist and the OS is live.

Use `--cases`, `--samples`, `--paced-samples`, `--allocation-samples`, `--repeats`, `--scales` and `--scale-cases` to select a run. `--scales` with no values skips scale experiments. `--source-commit` is optional and must refer to the verified remote source; local Git status and diff hash are always captured. Build logs and lockfiles fix the compiler configuration and dependency resolution. No registry token is required to benchmark.
