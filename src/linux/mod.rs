//! Linux procfs collector.
mod parse;
mod reader;
pub use parse::{parse_cpu_stat, parse_meminfo, parse_process_stat, ParseError};
pub use reader::{LinuxCollector, LinuxError, ProcessRecord};
