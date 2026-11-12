#![cfg_attr(not(feature = "std"), no_std)]
#![doc = include_str!("../README.md")]

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
