# Acceptance progress — 2026-11-12

## Implemented and exercised

- Independent Linux CPU, memory, process, mounted filesystem, block I/O and network collectors; lazy `System` composition.
- Correct first-sample/rollback/hotplug CPU semantics; monotonic inter-observation process CPU deltas.
- PID/start-time identity, incremental table reconciliation, targeted refresh, inaccessible/stale status and explicit owned process details.
- Native LP64 x86-64/AArch64 directory enumeration with a standard-library comparison path; reused bounded buffers.
- Network traffic, IPv4/IPv6/netmasks, link address, MTU, optional speed and operational state; interface-index baseline invalidation.
- Escaped mountinfo, independently requested capacity, major/minor block I/O and reset-aware differences.
- Optional read-only hwmon temperature, DRM GPU inventory/driver-exported counters and non-identifying DMI inventory.
- Zero third-party runtime crates; no_std parser/model/cadence core; alloc and std configurations.
- Boundary/permission/lifecycle/concurrency fixtures, live smoke tests, 10,000-process fixture and standalone resource accounting.
- Fifty unit/integration tests, three documentation tests and an independent resource-test binary at the network-buffer snapshot. Portable core has fifteen tests plus documentation tests.
- Sixty-second isolated release-mode run: 23,885,007 refreshes, zero hot Rust allocations and no retained allocation/file-descriptor growth on the tested path.
- sysinfo 0.40.3 pinned independent benchmark workspace; paired scopes, cold/hot profiles, separate latency/allocation binaries, raw samples, skips, environment hashes and independent statistical validation.
- Full CI matrix passed at `aaa7d48608e1b3a2a2c1f22010dfd4174b0c03c1`: Linux x86-64/AArch64, Windows/macOS portable core, Debian glibc/Alpine musl, native ABI, MSRV 1.95, bare-metal no_std, alloc/wasm, packaging, benchmark build and resource soak. Subsequent code changes must pass again before publication.
- Tested none/patch/minor/major version management, synchronized manifest and both lockfiles, changelog generation, clean-source packaging and checksum-gated publication/tagging.
- `CARGO_REGISTY_TOKEN` and `CARGO_REGISTRY_TOKEN` GitHub Secrets are both accepted, mapped to Cargo's official environment variable. Tokens never enter tracked files.

## Acceptance still requires an external success record

Publication is not established merely by a workflow existing. The release job must pass the complete validation matrix, publish the crate, confirm its registry archive checksum and create the matching GitHub tag/Release. Consult the actual Release run and registry rather than interpreting this implementation checklist as a publication receipt.

## Explicit limits / not complete

- Native Windows/macOS, FreeBSD, Android and iOS collectors are not implemented. Portable-core builds are not native collector support.
- GPU telemetry is driver-attribute dependent; no universal NVIDIA/Intel/AMD accounting or vendor-control API is claimed.
- Several latency/P99 goals are not met. At 501 visible processes, the matched median refresh was 1.129 ms. See BENCHMARKS.md for both favorable and unfavorable results.
- Live 1,000/5,000/10,000-process benchmarks were safety-budget skips in the local host. The 10,000-process fixture is correctness testing only.
- No matched disk-I/O performance claim is possible on the local host without a common accessible block-device scope.
- Longer all-module leak/permission/hotplug soaks and dedicated-machine tail-latency validation remain necessary before high-stakes deployment.
- Replacement of a device/interface entirely between observations with indistinguishable identity/counters cannot always be detected; this limitation is documented.

## Next optimization sequence

Profile and bound per-PID descriptor caching; optimize complete network metadata without caching stale fields; measure CPU kernel read cost separately; repeat the scale/P99 matrix on a dedicated host; then implement platform-native Windows and macOS backends without pretending Linux data models fit all OS semantics.
