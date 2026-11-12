#![cfg_attr(not(feature = "std"), no_std)]
//! Low-overhead system information collection.
//!
//! Counter models and parsers work without `std` or an allocator.
//! Linux collection is enabled by `std`; other targets currently expose models only.
//!
//! ```
//! use zsys::cpu::CpuTimes;
//! let before = CpuTimes { user: 100, idle: 900, ..CpuTimes::default() };
//! let after = CpuTimes { user: 150, idle: 950, ..CpuTimes::default() };
//! assert_eq!(after.busy_percent_since(before), Some(50.0));
//! ```
//!
//! ```no_run
//! # #[cfg(all(feature = "std", target_os = "linux"))]
//! # fn main() -> Result<(), zsys::linux::LinuxError> {
//! let mut system = zsys::System::new();
//! system.refresh_memory()?;
//! system.refresh_processes()?;
//! for process in system.processes() {
//!     println!("{}: {} bytes", process.id.pid, process.memory_bytes);
//! }
//! # Ok(())
//! # }
//! # #[cfg(not(all(feature = "std", target_os = "linux")))]
//! # fn main() {}
//! ```

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod cpu;
pub mod disk;
pub mod memory;
pub mod network;
#[path = "linux/parse.rs"]
pub mod parse;
pub mod process;
pub mod sampling;
pub mod snapshot;

#[cfg(all(feature = "std", target_os = "linux"))]
pub mod linux;

#[cfg(all(feature = "std", target_os = "linux"))]
pub use linux::System;

pub use snapshot::{Sample, SystemSnapshot};
