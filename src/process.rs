//! Process identity and cumulative counters.

/// Start time is included to distinguish a reused PID from its previous process.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ProcessId {
    pub pid: u32,
    pub start_time_ticks: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessCounters {
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub virtual_bytes: u64,
    pub resident_pages: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessState {
    Running, Sleeping, DiskSleep, Zombie, Stopped, Tracing, Dead, Idle, Unknown,
}
