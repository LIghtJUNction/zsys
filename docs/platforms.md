# Platform support

The portable core and native collection backends have different support boundaries. A successful Windows or macOS core build does not provide a native system-information collector on that platform.

## Implemented capability

| Target | Portable models/parsers | Native collection | Boundaries |
|---|---|---|---|
| Linux x86_64, glibc | Yes | CPU, memory, processes, disk/mounts, network, optional sysfs hardware | Requires accessible kernel files and permitted native calls |
| Linux aarch64, glibc | Yes | Same Linux backend | Native `getdents64` number and C ABI have dedicated CI checks |
| Linux x86_64, musl | Yes | Same Linux backend | Alpine CI checks the native headers and runs Linux tests |
| Other Linux architectures | Yes | Common procfs paths are conditionally available | Process construction defaults to `ScanMode::Std` outside x86_64/aarch64; these architectures do not have the same native test coverage |
| Linux 32-bit | Yes | Partial platform paths only | Filesystem capacity returns `Unsupported`; no complete backend acceptance claim |
| Windows and macOS | Yes | **Not implemented** | CI builds/tests the core and verifies Linux code stays gated |
| Android, iOS, FreeBSD | Portable core is designed to remain OS-independent | **Not implemented** | No native collection or runtime validation is claimed |
| `thumbv7em-none-eabi` | Core compilation | None | `no_std`, including the optional `alloc` configuration, is a compile gate |
| `wasm32-unknown-unknown` | Core compilation | None | `no_std` core is a compile gate |

These rows describe source capabilities and the configured validation scope. Inspect the [CI runs](https://github.com/LIghtJUNction/zsys/actions/workflows/ci.yml) and [acceptance record](progress.md) for execution results on the commit you use.

## Feature and compiler requirements

The MSRV is Rust **1.95**, edition **2024**. With default features disabled, the crate is `no_std` and its counter types and parsers do not require a global allocator. The `alloc` feature enables that capability without enabling OS access; it currently adds no separate allocation-backed public collection API. Default `std` enables Linux collection only when `target_os = "linux"`.

```sh
cargo test --locked --no-default-features
cargo check --locked --no-default-features --features alloc
cargo check --locked --no-default-features --target thumbv7em-none-eabi
cargo check --locked --no-default-features --features alloc --target thumbv7em-none-eabi
cargo check --locked --no-default-features --target wasm32-unknown-unknown
```

Install the listed Rust targets before running cross-target commands. Compilation on bare metal or WASM validates the portable core; these commands do not read the host's system information from those targets.

## Linux native boundary

The runtime dependency list is empty. Linux FFI calls the platform C ABI and kernel interfaces directly; the Rust `libc` crate is not required. The application still uses Rust `std` and the operating system's normal C runtime/linker environment. `getdents64` is used on x86_64/aarch64; `ScanMode::Std` allows a directly comparable standard-library enumeration path.

Filesystem capacity uses the Linux LP64 `statvfs` layout. Native headers are checked by [scripts/check-abi.c](../scripts/check-abi.c), including socket/address layouts and syscall/ioctl constants; corresponding Rust layout assertions run in unit tests. Current glibc and musl declarations for `statvfs` preserve the offsets used here, including their trailing reserved storage. See [statvfs(3)](https://man7.org/linux/man-pages/man3/statvfs.3.html) and the [musl header](https://git.musl-libc.org/cgit/musl/tree/include/sys/statvfs.h).

A mounted `/proc` and `/sys` can expose a container-specific, restricted, or host-wide view. zsys reports the view it reads: it does not grant permissions, join other namespaces, normalize CPU by container quota, or replace RAM totals with cgroup limits. `with_proc_root` and `with_sys_root` support fixtures and alternate mounted views. Cached descriptors are associated with the view used when opened; create new collectors after changing the process's namespace or mounts.

Network details use `getifaddrs`, with a read-only ioctl/procfs fallback when the native address path encounters specified permission/protocol restrictions. Metadata queries still can fail; counter collection remains independently callable. Six-octet MAC addresses are optional, and link speed depends on driver sysfs support. Details use the current network namespace even when the counter collector reads a custom proc root.

Hardware inventory depends on exported kernel attributes. A headless container can legitimately have no sensors or DRM cards. Thermal-class temperatures, raw hwmon channels, DMI descriptions, PCI identification, and optional AMDGPU metrics are the current scope. There is no universal GPU telemetry backend, and no hardware-control operation.

## Validation matrix

[ci.yml](../.github/workflows/ci.yml) defines Ubuntu 22.04/24.04, Windows/macOS core jobs, a native Ubuntu aarch64 job, Debian/glibc and Alpine/musl containers, MSRV, bare-metal/WASM checks, formatting, Clippy, rustdoc and benchmark compilation. Fixtures cover parsing, stale values, lifecycle and identity changes without requiring a particular physical disk, GPU, or sensor.

[soak.yml](../.github/workflows/soak.yml) runs an extended process-resource test, with a manually selectable duration and a scheduled run. [tests/process_resources.rs](../tests/process_resources.rs) checks allocations and descriptor growth for a continuously reused collector. This test's scope is the process collector; hardware-driver behavior and every filesystem implementation remain dependent on the deployment environment.

## Next native backends

Windows, macOS, FreeBSD, Android and iOS require their own native interfaces and semantics. They will be added behind platform-specific modules rather than treating Linux procfs fields as an implementation for other kernels. Their implementation and native testing are pending.
