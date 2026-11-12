# zsys versus sysinfo 0.40.3 — measured results

Measured on 2026-11-12. These are observations on a shared Linux container, not a hard-real-time guarantee or a claim that every workload is faster. The fixed competitor is **sysinfo 0.40.3**, without its default features and with `system`, `network`, and `disk` enabled. No sysinfo implementation source was used by zsys.

## Evidence and reproduction

The [measurement contract](benchmarks/PROTOCOL.md) defines every requested field, cold/hot boundaries, pacing, unmatched profiles and instrumentation limits. Run:

```sh
python3 benchmarks/run.py --output benchmark-results
python3 scripts/benchmark_report.py benchmark-results --output benchmark-analysis
```

The runner retains individual latency samples, separate allocation samples, per-child stderr, device/mount scope, compiler flags, binary hashes, process counts, failures and skipped cases. The independent report script reopens every CSV and checks all recorded summary statistics before aggregating it. Percentiles use nearest rank. Tables below use **pooled samples across three repetitions**, not the best repetition. The archive also retains the median of repetition medians; this explains small differences from development progress updates.

[Owner delivery archive](https://drive.google.com/file/d/1YiHh8lXTpOLto-G4XgsQvN3ZP5JuQE-C/view) contains 666 measured child runs, exact source snapshots, raw samples, complete aggregate CSV/JSON, SHA-256 manifests and resource-test logs. Drive access is required; this link does not grant public sharing. Archive SHA-256: `cfb5643778d33e4003622851a321407447c298292b15afba8176c7a5a39257dd`. Repository CI independently uploads raw benchmark artifacts, retained for 90 days, under [Benchmark runs](https://github.com/LIghtJUNction/zsys/actions/workflows/benchmark.yml). Different runners are different datasets; do not combine their distributions.

Three source-bound datasets are preserved:

| Dataset | Source commit | Purpose |
| --- | --- | --- |
| formal-d2aa352 | `d2aa35203b9d13a6141957853b2327fc78deb161` | First complete matrix; stable 101/501-process comparisons |
| formal-545b5ab | `545b5ab0563494fc047853311ad3b8e913b79c0a` | Complete matrix after reusable process-detail storage and hardware/resource tests |
| network-ce5363e | `ce5363e4198058b8cddd28f1a03612579053a588` | Changed network and process-detail profiles after bounded sysfs-buffer reuse |

The complete matrices each contain 312 child measurements and four explicitly recorded skips. The last targeted matrix contains 42 measurements and no skips. Code, compiler and binary identities are retained in each directory; later documentation/release commits must not be mislabeled as the measured commit.

## Host and method

The host reports **AMD EPYC 9V74 80-Core Processor**, five visible logical CPUs, and a cgroup CPU quota of four CPUs. `/proc/meminfo` reports 6,090,668 KiB total memory; the container memory limit is 4 GiB. The operating system is Debian GNU/Linux 13.3 (trixie), Linux 6.18.57. Compiler: rustc 1.99.1 (`58cf8f9c3`, 2026-10-13), LLVM 23.1.1. Both implementations use release builds, thin LTO and one codegen unit. Full flags and kernel identification are in `environment.json`.

Each ordinary profile has 1,000 timed samples per repetition; CPU-paced profiles have 100. There are three alternating-order repetitions. Allocation measurements run separately, with 100 samples per repetition, or ten for paced profiles. Each implementation/profile/repetition is a fresh process. CPU comparisons pace **both** implementations at sysinfo's minimum update interval plus 5 ms, outside the timed region. Inverse mean execution time is throughput capacity, not the actual paced delivery rate. Cold tests construct new collectors but do not flush OS caches.

This was not an isolated latency laboratory: no CPU affinity, frequency lock or exclusive host was available, and other development processes sometimes ran. Process-count drift is explicitly recorded. A matching count is necessary but not proof of identical internal work; see the field contract. P99 from 300 paced observations has only three tail observations and is a weak estimate. Maximum latency and worse P99 results are retained, not discarded.

## Matched hot-path results

All times are microseconds. Except the marked network row, results are from `formal-545b5ab`; the network row uses `network-ce5363e`. These rows have equal observed unit counts across both implementations and no reported collection failures. The host process profiles here observed 18 processes; the single-process target was the same owned sleeping process. CPU covers five logical CPUs, network covers two interfaces, and filesystem space covers the same single mount `/`.

| Profile | zsys median | sysinfo median | Median ratio, sysinfo/zsys | zsys P99 | sysinfo P99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Memory | 2.534 | 5.018 | 1.98x | 5.057 | 9.043 |
| CPU utilization, paced | 31.438 | 53.445 | 1.70x | 477.519 | 1206.904 |
| Full network, optimized | 27.982 | 54.417 | 1.94x | 170.006 | 175.384 |
| Filesystem space, one mount | 0.311 | 0.400 | 1.29x | 0.461 | 0.531 |
| One process, memory profile | 1.862 | 9.635 | 5.17x | 3.455 | 56.415 |
| All-process memory profile | 41.532 | 158.108 | 3.81x | 137.316 | 457.899 |
| All-process metadata list | 41.423 | 130.341 | 3.15x | 167.933 | 337.678 |
| One process plus CPU, paced | 38.688 | 103.772 | 2.68x | 158.699 | 893.636 |
| All processes plus CPU, paced | 198.569 | 455.627 | 2.29x | 687.686 | 1287.599 |
| One process with fresh details | 11.107 | 12.168 | 1.10x | **55.304** | **30.456** |

The last row is a counterexample to an unconditional win: its median is lower, but zsys's measured P99 is worse. The optimized network's maximum was **5500.935 us**, versus **404.889 us** for sysinfo, despite its lower median and slightly lower P99. A faster median does not establish a stable worst-case bound.

Mean, median, P95, P99, maximum, inverse-mean throughput, allocation counts/bytes, extra live allocation peak and process peak RSS for **every** measured profile are in the archive's `analysis/*/aggregate.csv` and `.json`, independently recomputed from raw rows. The paired table is not a substitute for those distributions.

## Process scale and native-directory ablation

The first matrix maintained equal counts throughout its controlled live-process experiments. Requested scales of 100 and 500 yielded **101 and 501 visible processes**, including the measurement/helper processes. These are not relabeled as exactly 100/500. No process details, task enumeration or process disk I/O are secretly omitted on one side of the declared memory profile.

| Actual visible processes | zsys all-process median | sysinfo median | Ratio | zsys P99 | sysinfo P99 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 101 | 214.328 us | 890.186 us | 4.15x | 464.950 us | 1539.419 us |
| 501 | 1128.525 us | 4748.783 us | 4.21x | 1834.483 us | 8068.552 us |

The later full matrix also exercised these scales, but some rows experienced count drift while other validation processes started. Those raw results remain in the archive with pairing warnings; they are not substituted into the strictly matched table.

The native `getdents64` and standard-library enumeration paths use the same zsys parsing and output. In the first matrix, the median of repetition medians at 501 processes was 57.447 us versus 78.918 us for PID enumeration, and 1130.573 us versus 1155.200 us for complete process-memory refresh. Thus native directory traversal helped enumeration substantially but only modestly improved the full workload, where per-process file access dominates. This is an ablation, not a sysinfo comparison.

Live 1,000/5,000/10,000-process experiments were **skipped**, because the conservative safety budget allowed only 845 additional children at that point. The policy reserves 64 PID slots and budgets at most half of available memory at 2 MiB per child. The independent 10,000-process fixture validates reconciliation, exits and PID reuse; it is not a live procfs performance result.

## Memory costs and optimization evidence

Counts below are successful Rust allocator/reallocator requests. Native C allocations, including those inside `getifaddrs`, are not captured. Peak RSS is the maximum `VmHWM` among fresh **uninstrumented** children, including binary/runtime, collector state and the sample vector; it is not exclusive library heap size.

| Profile | zsys allocations/op | sysinfo allocations/op | zsys requested bytes/op | sysinfo requested bytes/op | zsys peak RSS, bytes | sysinfo peak RSS, bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Memory | 0 | 1 | 0 | 16635 | 1363968 | 1376256 |
| CPU | 0 | 8 | 0 | 9366 | 1327104 | 1449984 |
| One process | 0 | 13 | 0 | 33354 | **1482752** | **1454080** |
| All processes, 18 visible | 0 | 169 | 0 | 41405 | 1462272 | 1495040 |
| Filesystem space | 0 | 2 | 0 | 9 | 1355776 | 1503232 |
| Full network after optimization | 29 | 73 | 1318 | 6821 | 1433600 | 1544192 |
| All processes, 501 visible, first matrix | 0 | 3550 | 0 | 268898 | 1642496 | 2473984 |

Zero hot Rust allocations do not imply universally smaller process RSS: the single-process zsys child had a slightly higher peak RSS in this run. Topology changes, owned metadata and cold initialization can allocate.

Before network optimization, zsys requested **25,894 bytes** in 35 allocations per full refresh, worse in bytes than sysinfo's 6,821. A reused, lazily allocated, bounded sysfs buffer reduced that to **1,318 bytes in 29 allocations**, a **94.9% byte reduction**, without dropping any requested interface fields. The maximum extra live Rust bytes per hot operation became 920, versus sysinfo's 4292. The fresh-collector network profile still requested 12,365 bytes versus sysinfo's 7545; initialization is not a memory win in every profile.

Reusing the command-line scratch buffer reduced the one-process detail path from three allocations/4363 bytes to two allocations/267 bytes. Its output remains owned raw NUL-separated data, with executable and user/group queries unchanged. These byte-count changes are direct instrumented evidence; small timing differences across sessions are not clean causal estimates because the host was shared.

## Missing pairs, cold paths and targets

The host had no accessible common mounted block device, so matched `disk-io` was skipped rather than timed as empty work. zsys's unmatched all-block-device profile read 26 devices: median 9.564 us and P99 51.027 us in the second matrix. Counter-only network and PID-only enumeration are also unmatched; they must not be advertised as speedups over sysinfo's broader refresh APIs.

Cold-memory median was 4.286 us versus 10.570 us. Cold network after the optimization was 32.058 us versus 53.811 us. Combined `basic` in the second matrix had medians 409.692 us and 662.232 us, but process counts varied from 18 to 20; cold combined/process experiments had further drift. They are retained as workload observations, **not strict matched speedup claims**. The combined profile includes full network metadata and mounted capacity and is broader than the library's `System::refresh_basic()` convenience method.

The project latency targets are research goals, not acceptance numbers invented after testing. CPU did not meet 5 us. Memory met 3 us at the median but not at P99. Full network met 30 us at the median after optimization but not at P99. A single mount's capacity query met 10 us; this does not prove complete disk discovery/I/O does. A hot single-process memory sample met 20 us, but paced process-CPU sampling did not. Full refresh at 501 visible processes did not meet 1 ms. The small host combined profile was below 1 ms at median/P99, without establishing the same result at larger scales.

## Resource stability and next bottlenecks

An isolated 60-second release-mode resource test completed **23,885,007 single-process refreshes**, asserting zero hot Rust allocations, unchanged live requested bytes and unchanged file-descriptor count. It also checked 100 collector construction/drop cycles for complete resource release. This is bounded evidence for that path, not proof of leak freedom for every API or an hours-long production soak. The original libtest-based 60-second attempt observed allocations from the test framework's progress message; both its failure log and the corrected standalone test's success log are retained. Assertions were not weakened.

Next priorities are bounded per-PID descriptor caching with PID-reuse/descriptor-pressure tests; reducing full interface discovery work without stale fields; profiling CPU procfs generation separately from parsing; longer all-module hotplug/permission soak tests; and dedicated-host repeated tail-latency measurements. Native Windows/macOS backends and universal vendor GPU telemetry remain separate unfinished work. Do not infer production certification, complete cross-platform support or a worst-case latency guarantee from these results.
