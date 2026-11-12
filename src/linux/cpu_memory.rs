//! Independently usable CPU and memory collectors with reusable storage.
use super::LinuxError;
use super::io::CachedFile;
use crate::parse::{ParseError, cpu_fields, fields, number, parse_cpu_stat, parse_meminfo};
use crate::{Sample, cpu::CpuTimes, memory::Memory};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CpuSample {
    pub id: u32,
    pub times: CpuTimes,
    pub busy_percent: Option<f32>,
    /// Last explicitly refreshed scaling frequency. Not necessarily instantaneous hardware frequency.
    pub scaling_frequency_hz: Option<u64>,
}

pub struct CpuCollector {
    file: CachedFile,
    sys_root: PathBuf,
    current: Vec<CpuSample>,
    scratch: Vec<CpuSample>,
    aggregate: Option<CpuTimes>,
    aggregate_busy: Option<f32>,
    started: Instant,
    sample: Sample,
}
impl Default for CpuCollector {
    fn default() -> Self {
        Self::new()
    }
}
impl CpuCollector {
    pub fn new() -> Self {
        Self::with_roots("/proc", "/sys")
    }
    pub fn with_roots(proc_root: impl AsRef<Path>, sys_root: impl AsRef<Path>) -> Self {
        Self {
            file: CachedFile::new(proc_root.as_ref().join("stat")),
            sys_root: sys_root.as_ref().to_owned(),
            current: Vec::new(),
            scratch: Vec::new(),
            aggregate: None,
            aggregate_busy: None,
            started: Instant::now(),
            sample: Sample::default(),
        }
    }
    /// Samples the aggregate and every currently online logical CPU. No hidden throttling.
    /// A malformed read leaves the last successful public snapshot intact.
    pub fn refresh(&mut self) -> Result<(), LinuxError> {
        let bytes = self.file.read()?;
        let aggregate = parse_cpu_stat(bytes)?;
        self.scratch.clear();
        for line in bytes.split(|b| *b == b'\n').skip(1) {
            let mut values = fields(line);
            let Some(label) = values.next() else { continue };
            let Some(suffix) = label.strip_prefix(b"cpu") else { break };
            let id = u32::try_from(number(suffix)?).map_err(|_| ParseError::InvalidNumber)?;
            if self.scratch.last().is_some_and(|previous| previous.id >= id) {
                return Err(ParseError::InvalidRecord.into());
            }
            let times = cpu_fields(values)?;
            let previous = self.current.binary_search_by_key(&id, |cpu| cpu.id).ok().map(|i| self.current[i]);
            self.scratch.push(CpuSample {
                id,
                times,
                busy_percent: previous.and_then(|old| times.busy_percent_since(old.times)),
                scaling_frequency_hz: previous.and_then(|old| old.scaling_frequency_hz),
            });
        }
        if self.scratch.is_empty() {
            return Err(ParseError::MissingField.into());
        }
        // A changed online CPU set invalidates the aggregate baseline, not unaffected per-core baselines.
        let same_topology = self.current.iter().map(|c| c.id).eq(self.scratch.iter().map(|c| c.id));
        self.aggregate_busy =
            self.aggregate.filter(|_| same_topology).and_then(|old| aggregate.busy_percent_since(old));
        self.aggregate = Some(aggregate);
        std::mem::swap(&mut self.current, &mut self.scratch);
        self.sample.sequence = self.sample.sequence.wrapping_add(1);
        self.sample.timestamp_ns = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        Ok(())
    }
    pub fn cores(&self) -> &[CpuSample] {
        &self.current
    }
    pub fn aggregate_times(&self) -> Option<CpuTimes> {
        self.aggregate
    }
    pub fn aggregate_busy_percent(&self) -> Option<f32> {
        self.aggregate_busy
    }
    pub fn sample(&self) -> Sample {
        self.sample
    }
    /// Separate, potentially slower sysfs sampling; unsupported/denied values become `None`.
    /// Returns the number of readable frequencies. This never changes utilization sample times.
    pub fn refresh_frequencies(&mut self) -> usize {
        let mut count = 0;
        for cpu in &mut self.current {
            let path = self.sys_root.join(format!("devices/system/cpu/cpu{}/cpufreq/scaling_cur_freq", cpu.id));
            cpu.scaling_frequency_hz = std::fs::read(path)
                .ok()
                .and_then(|bytes| number(bytes.trim_ascii()).ok())
                .and_then(|khz| khz.checked_mul(1000));
            count += usize::from(cpu.scaling_frequency_hz.is_some());
        }
        count
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MemorySample {
    pub counters: Memory,
    pub sample: Sample,
    /// True only for the documented legacy approximation when MemAvailable is absent.
    pub available_is_estimate: bool,
}
pub struct MemoryCollector {
    file: CachedFile,
    started: Instant,
    current: MemorySample,
}
impl Default for MemoryCollector {
    fn default() -> Self {
        Self::new()
    }
}
impl MemoryCollector {
    pub fn new() -> Self {
        Self::with_proc_root("/proc")
    }
    pub fn with_proc_root(root: impl AsRef<Path>) -> Self {
        Self {
            file: CachedFile::new(root.as_ref().join("meminfo")),
            started: Instant::now(),
            current: MemorySample::default(),
        }
    }
    pub fn refresh(&mut self) -> Result<&MemorySample, LinuxError> {
        let bytes = self.file.read()?;
        let counters = parse_meminfo(bytes)?;
        let available_is_estimate = !bytes.split(|b| *b == b'\n').any(|line| line.starts_with(b"MemAvailable:"));
        self.current = MemorySample {
            counters,
            available_is_estimate,
            sample: Sample {
                timestamp_ns: self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
                sequence: self.current.sample.sequence.wrapping_add(1),
            },
        };
        Ok(&self.current)
    }
    /// Sequence zero means no successful refresh yet, not a real zero-valued observation.
    pub fn snapshot(&self) -> &MemorySample {
        &self.current
    }
}
