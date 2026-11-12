# Changelog

## 0.1.0 - 2026-11-12

Initial Linux implementation: independently refreshable CPU, memory, process lifecycle, block I/O, mount capacity and network collectors. Optional read-only hwmon, DRM sysfs and non-identifying DMI inventory. Portable no_std core and alloc feature; no third-party runtime crates.

Adds matched sysinfo 0.40.3 benchmarks, raw latency and allocation evidence, native/standard enumeration ablation, process scale launcher, PID reuse/permission fixtures, 10,000-record reconciliation and isolated allocation/file-descriptor soak checks. Includes native ABI validation, MSRV, embedded no_std and multi-OS core build CI, checked version bumps and checksum-verified publication.

Windows/macOS native collection and universal GPU telemetry are not implemented. Performance targets are workload-dependent and are not all met; see BENCHMARKS.md for measured scope and limits.
