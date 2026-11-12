# Changelog

## 0.1.0

- Portable `no_std` CPU, memory, process, disk and network models, parsers, delta calculations, and caller-driven cadence.
- Independent Linux CPU, memory, retained process table, filesystem capacity, mount discovery, block I/O and network collectors.
- PID/start-time identity, explicit stale/permission states, targeted refreshes, and on-demand process metadata.
- Optional DMI, thermal/hwmon and DRM/driver sysfs hardware information.
- Lazy `System` composition and executable examples.
- Independent benchmarks pinned to sysinfo `=0.40.3`, including raw latency, allocation and memory evidence.
- Fixture, lifecycle, concurrency and resource tests; native ABI checks and cross-platform core CI.
- Release workflow with SemVer bump choices, synchronized lockfiles, full candidate validation, crates.io checksum verification and GitHub Release generation.

Publication is recorded by the version tag, the registry entry and the GitHub Release. This entry records the candidate implementation without asserting that publication has completed.
