# Changelog

## Unreleased

- Share Windows handle-based private file inspection and protection through `windows::privacy`, under the existing `std` feature and without new dependencies.
- Preserve separate read-only checks, DACL-only reprotection and owner initialization for exclusively created objects. Make directory inheritance an explicit caller policy.
- Add native Windows contract tests for both consumer policies, read-only rejection and inherited children. Validate SIDs before querying their length.

## 0.1.1

- Add the opt-in, allocation-free `dylib` system interface for Linux, macOS and Windows.
- Keep loaded images pinned when handles are dropped; require explicit unsafe close after native work has stopped.
- Preserve UTF-16 Windows paths, native reference counts and explicit unsupported-target errors.
- Exercise real independently compiled libraries, missing symbols, repeated close and mapping lifetimes in the native CI matrix.

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
