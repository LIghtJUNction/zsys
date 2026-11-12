//! Physical memory and swap counters.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Memory {
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub free_bytes: u64,
    pub buffers_bytes: u64,
    pub cached_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_free_bytes: u64,
}

impl Memory {
    #[inline]
    pub const fn used_bytes(self) -> u64 {
        self.total_bytes.saturating_sub(self.available_bytes)
    }
    #[inline]
    pub const fn swap_used_bytes(self) -> u64 {
        self.swap_total_bytes.saturating_sub(self.swap_free_bytes)
    }
}
