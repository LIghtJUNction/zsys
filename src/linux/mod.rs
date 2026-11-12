//! Linux procfs collector.
mod reader;
pub use crate::parse;
pub use parse::{ParseError, parse_cpu_stat, parse_meminfo, parse_process_stat};
pub use reader::{LinuxCollector, LinuxError, ProcessRecord};
