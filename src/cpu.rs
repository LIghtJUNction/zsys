//! CPU counter model and delta calculations.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CpuTimes {
    pub user: u64,
    pub nice: u64,
    pub system: u64,
    pub idle: u64,
    pub iowait: u64,
    pub irq: u64,
    pub softirq: u64,
    pub steal: u64,
}

impl CpuTimes {
    #[inline]
    pub const fn total(self) -> u64 {
        self.user + self.nice + self.system + self.idle
            + self.iowait + self.irq + self.softirq + self.steal
    }

    /// Calculates busy percentage from two cumulative kernel samples.
    #[inline]
    pub fn busy_percent_since(self, previous: Self) -> Option<f32> {
        let total = self.total().checked_sub(previous.total())?;
        if total == 0 { return None; }
        let idle_now = self.idle.saturating_add(self.iowait);
        let idle_prev = previous.idle.saturating_add(previous.iowait);
        let idle = idle_now.checked_sub(idle_prev)?;
        Some((total.saturating_sub(idle) as f64 * 100.0 / total as f64) as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn computes_busy_delta() {
        let old = CpuTimes { user: 100, idle: 900, ..Default::default() };
        let new = CpuTimes { user: 150, idle: 950, ..Default::default() };
        assert_eq!(new.busy_percent_since(old), Some(50.0));
    }
    #[test]
    fn detects_counter_rollback() {
        let old = CpuTimes { user: 100, ..Default::default() };
        let new = CpuTimes { user: 10, ..Default::default() };
        assert_eq!(new.busy_percent_since(old), None);
    }
}
