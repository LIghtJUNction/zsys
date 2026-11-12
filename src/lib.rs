#![cfg_attr(not(feature = "std"), no_std)]
//! Low-overhead system information collection.

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod cpu;
pub mod memory;
pub mod process;
pub mod snapshot;

#[cfg(all(feature = "std", target_os = "linux"))]
pub mod linux;

pub use snapshot::{Sample, SystemSnapshot};
