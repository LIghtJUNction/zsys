//! Lazy composition of the independently usable collectors.
use super::{
    CpuCollector, DiskIoCollector, LinuxError, MemoryCollector, MemorySample, Mounts, Networks, Process,
    ProcessChanges, Processes,
};

/// A convenient owner for collectors. Construction performs no filesystem I/O.
///
/// Each module is initialized on its first explicit refresh. Samples retain their
/// own timestamps; refreshing one module never silently refreshes another.
#[derive(Default)]
pub struct System {
    cpu: Option<CpuCollector>,
    memory: Option<MemoryCollector>,
    processes: Option<Processes>,
    networks: Option<Networks>,
    disk_io: Option<DiskIoCollector>,
    mounts: Option<Mounts>,
}

impl System {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn refresh_cpu(&mut self) -> Result<&CpuCollector, LinuxError> {
        let collector = self.cpu.get_or_insert_with(CpuCollector::new);
        collector.refresh()?;
        Ok(collector)
    }

    pub fn refresh_memory(&mut self) -> Result<&MemorySample, LinuxError> {
        self.memory.get_or_insert_with(MemoryCollector::new).refresh()
    }

    pub fn refresh_processes(&mut self) -> Result<ProcessChanges, LinuxError> {
        self.process_collector()?.refresh()
    }

    pub fn refresh_process(&mut self, pid: u32) -> Result<&Process, LinuxError> {
        self.process_collector()?.refresh_pid(pid)
    }

    /// Includes interface addresses and flags. Use `refresh_network_counters`
    /// for the independent, high-frequency traffic sampling path.
    pub fn refresh_networks(&mut self) -> Result<&Networks, LinuxError> {
        let collector = self.networks.get_or_insert_with(Networks::new);
        collector.refresh()?;
        Ok(collector)
    }

    pub fn refresh_network_counters(&mut self) -> Result<&Networks, LinuxError> {
        let collector = self.networks.get_or_insert_with(Networks::new);
        collector.refresh_counters()?;
        Ok(collector)
    }

    /// Samples block-device and partition counters, not mounted filesystem capacity.
    pub fn refresh_disk_io(&mut self) -> Result<&DiskIoCollector, LinuxError> {
        let collector = self.disk_io.get_or_insert_with(DiskIoCollector::new);
        collector.refresh()?;
        Ok(collector)
    }

    /// Discovers mounted paths. Capacity sampling is explicit because remote
    /// filesystems can block: call `Mounts::iter_mut` to select the paths.
    pub fn refresh_mounts(&mut self) -> Result<&mut Mounts, LinuxError> {
        let collector = self.mounts.get_or_insert_with(Mounts::new);
        collector.refresh()?;
        Ok(collector)
    }

    /// Refreshes CPU, memory, processes, network counters and disk I/O in order.
    ///
    /// This is not an atomic kernel snapshot. On an error, preceding modules
    /// retain their new samples and later modules retain their earlier samples.
    pub fn refresh_basic(&mut self) -> Result<(), LinuxError> {
        self.refresh_cpu()?;
        self.refresh_memory()?;
        self.refresh_processes()?;
        self.refresh_network_counters()?;
        self.refresh_disk_io()?;
        Ok(())
    }

    pub fn cpu(&self) -> Option<&CpuCollector> {
        self.cpu.as_ref().filter(|cpu| cpu.sample().sequence != 0)
    }

    pub fn memory(&self) -> Option<&MemorySample> {
        self.memory.as_ref().map(MemoryCollector::snapshot).filter(|m| m.sample.sequence != 0)
    }

    pub fn processes(&self) -> impl Iterator<Item = &Process> {
        self.processes.iter().flat_map(Processes::iter)
    }

    pub fn process(&self, pid: u32) -> Option<&Process> {
        self.processes.as_ref().and_then(|p| p.get(pid))
    }

    /// Accesses the retained table, including explicit failure/staleness information.
    pub fn process_table(&self) -> Option<&Processes> {
        self.processes.as_ref()
    }

    /// Initializes only the process collector, for optional metadata queries.
    pub fn process_collector(&mut self) -> Result<&mut Processes, LinuxError> {
        if self.processes.is_none() {
            self.processes = Some(Processes::new()?);
        }
        Ok(self.processes.as_mut().expect("initialized above"))
    }

    pub fn networks(&self) -> Option<&Networks> {
        self.networks.as_ref().filter(|networks| networks.sample().sequence != 0)
    }

    pub fn disk_io(&self) -> Option<&DiskIoCollector> {
        self.disk_io.as_ref().filter(|disks| disks.sample().sequence != 0)
    }

    pub fn mounts(&self) -> Option<&Mounts> {
        self.mounts.as_ref().filter(|mounts| mounts.sample().sequence != 0)
    }
}
