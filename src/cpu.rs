//! Cumulative CPU counters, in kernel clock ticks. Guest time is already in user/nice.

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
    fn fields(self) -> [u64; 8] {
        [self.user, self.nice, self.system, self.idle, self.iowait, self.irq, self.softirq, self.steal]
    }

    /// Saturating total for display. Delta calculation uses wider arithmetic.
    pub fn total(self) -> u64 {
        self.fields().into_iter().fold(0, u64::saturating_add)
    }

    /// Busy percentage between two samples of the same CPU.
    ///
    /// Returns `None` for a counter rollback (including decreasing iowait),
    /// CPU replacement, or zero elapsed ticks. Do not invent a zero baseline.
    pub fn busy_percent_since(self, previous: Self) -> Option<f32> {
        let mut total = 0u128;
        let mut idle = 0u128;
        for (index, (now, old)) in self.fields().into_iter().zip(previous.fields()).enumerate() {
            let delta = u128::from(now.checked_sub(old)?);
            total += delta;
            if index == 3 || index == 4 {
                idle += delta;
            }
        }
        (total != 0).then(|| ((total - idle) as f64 * 100.0 / total as f64) as f32)
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
        let new = CpuTimes { user: 99, idle: 100, ..Default::default() };
        assert_eq!(new.busy_percent_since(old), None);
    }

    #[test]
    fn totals_do_not_overflow() {
        let now = CpuTimes { user: u64::MAX, idle: u64::MAX, ..Default::default() };
        assert_eq!(now.total(), u64::MAX);
        assert_eq!(now.busy_percent_since(CpuTimes::default()), Some(50.0));
        assert_eq!(now.busy_percent_since(now), None);
    }
}
