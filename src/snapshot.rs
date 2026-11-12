//! Shared snapshot types and sampling metadata.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Sample {
    /// Monotonic timestamp in nanoseconds.
    pub timestamp_ns: u64,
    pub sequence: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SystemSnapshot {
    pub sample: Sample,
    pub cpu: crate::cpu::CpuTimes,
    pub memory: crate::memory::Memory,
}
