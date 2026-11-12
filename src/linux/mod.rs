//! Linux procfs collector.
mod parse;
mod reader;
pub use reader::{LinuxCollector, LinuxError};
