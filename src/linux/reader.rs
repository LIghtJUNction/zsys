//! Linux collector reusing a single read buffer.

use std::{fs::{self, File}, io::{self, Read}, path::{Path, PathBuf}};
use crate::{cpu::CpuTimes, memory::Memory, process::{ProcessCounters, ProcessId, ProcessState}};
use super::parse::{parse_cpu_stat, parse_meminfo, parse_process_stat, ParseError};

#[derive(Debug)]
pub enum LinuxError { Io(io::Error), Parse(ParseError) }
impl From<io::Error> for LinuxError { fn from(e: io::Error) -> Self { Self::Io(e) } }
impl From<ParseError> for LinuxError { fn from(e: ParseError) -> Self { Self::Parse(e) } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessRecord {
    pub id: ProcessId,
    pub parent_pid: u32,
    pub state: ProcessState,
    pub counters: ProcessCounters,
}

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

    /// Enumerates readable numeric PID directories and reads each process stat record.
    /// Processes that exit or become inaccessible during the scan are skipped.
    pub fn processes(&mut self) -> Result<Vec<ProcessRecord>, LinuxError> {
        let entries = fs::read_dir(&self.proc_root)?;
        let mut records = Vec::new();
        for entry in entries {
            let entry = match entry { Ok(v) => v, Err(_) => continue };
            let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
            self.buffer.clear();
            let path = entry.path().join("stat");
            let mut file = match File::open(path) { Ok(v) => v, Err(_) => continue };
            if file.read_to_end(&mut self.buffer).is_err() { continue; }
            if let Ok((parsed_pid, state, counters, parent_pid, start_time_ticks)) = parse_process_stat(&self.buffer) {
                if parsed_pid == pid {
                    records.push(ProcessRecord {
                        id: ProcessId { pid, start_time_ticks },
                        parent_pid,
                        state,
                        counters,
                    });
                }
            }
        }
        Ok(records)
    }
}
