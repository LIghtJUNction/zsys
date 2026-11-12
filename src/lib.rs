#![cfg_attr(not(feature = "std"), no_std)]
//! Low-overhead system information collection.
//!
//! Counter models and parsers work without `std` or an allocator.
//! Linux collection is enabled by `std`; other targets currently expose models only.

#[cfg(feature = "alloc")]
extern crate alloc;

pub mod cpu;
pub mod memory;
pub mod network;
#[path = "linux/parse.rs"]
pub mod parse;
pub mod process;
pub mod sampling;
pub mod snapshot;

#[cfg(all(feature = "std", target_os = "linux"))]
pub mod linux;

pub use snapshot::{Sample, SystemSnapshot};
