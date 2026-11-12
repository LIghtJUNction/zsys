//! Small compatibility collector. Stateful process users should use `Processes` directly.
use super::io::CachedFile;
use super::parse::{ParseError, parse_cpu_stat, parse_meminfo};
use crate::{
    cpu::CpuTimes,
    memory::Memory,
    process::{ProcessCounters, ProcessId, ProcessState},
};
use std::path::{Path, PathBuf};
use std::{fmt, io};

#[derive(Debug)]
pub enum LinuxError {
    Io(io::Error),
    Parse(ParseError),
}
impl From<io::Error> for LinuxError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<ParseError> for LinuxError {
    fn from(e: ParseError) -> Self {
        Self::Parse(e)
    }
}
impl fmt::Display for LinuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => e.fmt(f),
            Self::Parse(e) => e.fmt(f),
        }
    }
}
impl std::error::Error for LinuxError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessRecord {
    pub id: ProcessId,
    pub parent_pid: u32,
    pub state: ProcessState,
    pub counters: ProcessCounters,
}
pub struct LinuxCollector {
    proc_root: PathBuf,
    cpu: CachedFile,
    memory: CachedFile,
    processes: Option<super::Processes>,
}
impl Default for LinuxCollector {
    fn default() -> Self {
        Self::new()
    }
}
impl LinuxCollector {
    pub fn new() -> Self {
        Self::with_proc_root("/proc")
    }
    pub fn with_proc_root(path: impl AsRef<Path>) -> Self {
        Self {
            proc_root: path.as_ref().to_owned(),
            cpu: CachedFile::new(path.as_ref().join("stat")),
            memory: CachedFile::new(path.as_ref().join("meminfo")),
            processes: None,
        }
    }
    pub fn cpu_times(&mut self) -> Result<CpuTimes, LinuxError> {
        Ok(parse_cpu_stat(self.cpu.read()?)?)
    }
    pub fn memory(&mut self) -> Result<Memory, LinuxError> {
        Ok(parse_meminfo(self.memory.read()?)?)
    }
    /// Compatibility API allocates an owned result. `Processes::refresh` retains its table instead.
    pub fn processes(&mut self) -> Result<Vec<ProcessRecord>, LinuxError> {
        if self.processes.is_none() {
            self.processes = Some(super::Processes::with_proc_root(&self.proc_root)?);
        }
        let processes = self.processes.as_mut().expect("initialized above");
        processes.refresh()?;
        Ok(processes
            .iter()
            .filter(|p| p.availability == super::Availability::Fresh)
            .map(|p| ProcessRecord { id: p.id, parent_pid: p.parent_pid, state: p.state, counters: p.counters })
            .collect())
    }
}
