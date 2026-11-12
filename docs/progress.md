# Development progress

## Current milestone: Linux foundation

- [x] Empty repository initialized with a dependency-free crate.
- [x] Core snapshot, CPU, memory, and process identity models.
- [x] no_std compilation path for core data types.
- [x] Allocation-free parsers for selected /proc records.
- [x] Reusable-buffer Linux CPU and memory readers.
- [x] Initial process enumeration with PID start-time identity.
- [x] CI: stable toolchain, MSRV, no_std check, tests, Clippy, docs, Linux/Windows/macOS build matrix.
- [x] Release workflow: patch/minor/major bump, CARGO_REGISTRY_TOKEN, package, publish, tag, GitHub Release.
- [ ] Execute and inspect CI on GitHub Actions.
- [ ] Complete process metadata, lifecycle diffing, CPU delta, permissions and PID-reuse integration tests.
- [ ] Implement disk and network collection.
- [ ] Implement native Linux directory enumeration/openat path and benchmark against std path.
- [ ] Build reproducible sysinfo 0.40.3 comparison harness and allocation/RSS measurements.
- [ ] Add fixtures, stress tests, long-running tests and distro matrix.
- [ ] Publish initial crate only after CI and package verification.

## Known limitations

The current process scanner uses std directory traversal and opens one stat file per process. It does not yet collect command line, executable, user, thread count or per-process CPU percentage. Disk, network, GPU, thermal and hardware modules are not implemented. No performance claims or benchmark numbers have been recorded.
