//! Caller-driven scheduling. No background thread, sleep, or hidden refresh.

/// A minimum interval between successful samples, in caller-defined monotonic nanoseconds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cadence {
    interval_ns: u64,
    last_success: Option<u64>,
}
impl Cadence {
    pub const fn new(interval_ns: u64) -> Self {
        Self { interval_ns, last_success: None }
    }
    pub const fn interval_ns(&self) -> u64 {
        self.interval_ns
    }
    pub fn set_interval_ns(&mut self, interval_ns: u64) {
        self.interval_ns = interval_ns;
    }
    pub fn is_due(&self, now_ns: u64) -> bool {
        match self.last_success.and_then(|old| now_ns.checked_sub(old)) {
            Some(elapsed) => elapsed >= self.interval_ns,
            None => true,
        }
    }
    /// Call only after collection succeeds. A failed read must remain retryable.
    pub fn mark_success(&mut self, now_ns: u64) {
        self.last_success = Some(now_ns);
    }
    pub fn reset(&mut self) {
        self.last_success = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_intervals_without_changing_sample_time() {
        let mut schedule = Cadence::new(100);
        assert!(schedule.is_due(10));
        schedule.mark_success(10);
        assert!(!schedule.is_due(109));
        assert!(schedule.is_due(110));
        schedule.set_interval_ns(200);
        assert!(!schedule.is_due(110));
        assert!(schedule.is_due(210));
        assert!(schedule.is_due(0));
        schedule.reset();
        assert!(schedule.is_due(10));
    }
}
