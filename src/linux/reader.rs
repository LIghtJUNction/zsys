//! Linux collector reusing a single read buffer.

use std::{fs::File, io::{self, Read}, path::{Path, PathBuf}};
use crate::{cpu::CpuTimes, memory::Memory};
use super::parse::{parse_cpu_stat, parse_meminfo, ParseError};

#[derive(Debug)]
pub enum LinuxError { Io(io::Error), Parse(ParseError) }
impl From<io::Error> for LinuxError { fn from(e: io::Error) -> Self { Self::Io(e) } }
impl From<ParseError> for LinuxError { fn from(e: ParseError) -> Self { Self::Parse(e) } }

pub struct LinuxCollector { proc_root: PathBuf, buffer: Vec<u8> }
impl Default for LinuxCollector { fn default() -> Self { Self::new() } }
impl LinuxCollector {
    pub fn new() -> Self {
        Self { proc_root: PathBuf::from("/proc"), buffer: Vec::with_capacity(16 * 1024) }
    }
    pub fn with_proc_root(path: impl AsRef<Path>) -> Self {
        Self { proc_root: path.as_ref().to_path_buf(), buffer: Vec::with_capacity(16 * 1024) }
    }
    fn read(&mut self, name: &str) -> Result<&[u8], LinuxError> {
        self.buffer.clear();
        let mut file = File::open(self.proc_root.join(name))?;
        file.read_to_end(&mut self.buffer)?;
        Ok(&self.buffer)
    }
    pub fn cpu_times(&mut self) -> Result<CpuTimes, LinuxError> {
        let bytes = self.read("stat")?;
        Ok(parse_cpu_stat(bytes)?)
    }
    pub fn memory(&mut self) -> Result<Memory, LinuxError> {
        let bytes = self.read("meminfo")?;
        Ok(parse_meminfo(bytes)?)
    }
}
