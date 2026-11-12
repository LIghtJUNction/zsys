//! Process identity and cumulative counters.

/// Start time distinguishes a reused PID from the previous process in the same boot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ProcessId {
    pub pid: u32,
    pub start_time_ticks: u64,
}

impl ProcessId {
    /// Kernel boot-relative start time, with subsecond precision from `_SC_CLK_TCK`.
    /// This is not a wall-clock timestamp. Zero clock frequency is invalid.
    pub fn start_time_since_boot(self, ticks_per_second: u64) -> Option<core::time::Duration> {
        if ticks_per_second == 0 {
            return None;
        }
        let seconds = self.start_time_ticks / ticks_per_second;
        let nanos = u128::from(self.start_time_ticks % ticks_per_second) * 1_000_000_000 / u128::from(ticks_per_second);
        Some(core::time::Duration::new(seconds, nanos as u32))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessCounters {
    pub user_ticks: u64,
    pub system_ticks: u64,
    pub virtual_bytes: u64,
    pub resident_pages: u64,
}
impl ProcessCounters {
    /// Percentage of one logical CPU; multi-threaded processes may exceed 100%.
    /// The caller must verify identity and supply time BETWEEN successful samples.
    pub fn cpu_percent_since(self, old: Self, elapsed_ns: u64, ticks_per_second: u64) -> Option<f32> {
        if elapsed_ns == 0 || ticks_per_second == 0 {
            return None;
        }
        let user = self.user_ticks.checked_sub(old.user_ticks)?;
        let system = self.system_ticks.checked_sub(old.system_ticks)?;
        let ticks = u128::from(user) + u128::from(system);
        Some((ticks as f64 * 100.0 * 1e9 / (elapsed_ns as f64 * ticks_per_second as f64)) as f32)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessState {
    Running,
    Sleeping,
    DiskSleep,
    Zombie,
    Stopped,
    Tracing,
    Dead,
    Idle,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_uses_sample_interval_and_allows_multiple_cores() {
        let old = ProcessCounters::default();
        let new = ProcessCounters { user_ticks: 200, ..old };
        assert_eq!(new.cpu_percent_since(old, 1_000_000_000, 100), Some(200.0));
        assert_eq!(new.cpu_percent_since(old, 2_000_000_000, 100), Some(100.0));
        assert_eq!(new.cpu_percent_since(old, 0, 100), None);
        assert_eq!(old.cpu_percent_since(new, 1, 100), None);
    }

    #[test]
    fn start_time_keeps_subsecond_precision() {
        let id = ProcessId { pid: 1, start_time_ticks: 151 };
        assert_eq!(id.start_time_since_boot(100), Some(core::time::Duration::from_millis(1510)));
        assert_eq!(id.start_time_since_boot(0), None);
        let id = ProcessId { pid: 1, start_time_ticks: u64::MAX };
        assert_eq!(id.start_time_since_boot(u64::MAX), Some(core::time::Duration::from_secs(1)));
    }
}
