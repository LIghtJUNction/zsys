//! Allocation-free network counter model and Linux record parsing.
use crate::parse::{ParseError, fields, number};

/// The sixteen cumulative `/proc/net/dev` counters, in kernel order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NetworkCounters {
    pub received_bytes: u64,
    pub received_packets: u64,
    pub received_errors: u64,
    pub received_drops: u64,
    pub received_fifo: u64,
    pub received_frame: u64,
    pub received_compressed: u64,
    pub multicast: u64,
    pub transmitted_bytes: u64,
    pub transmitted_packets: u64,
    pub transmitted_errors: u64,
    pub transmitted_drops: u64,
    pub transmitted_fifo: u64,
    pub collisions: u64,
    pub transmitted_carrier: u64,
    pub transmitted_compressed: u64,
}
impl NetworkCounters {
    fn values(self) -> [u64; 16] {
        [self.received_bytes, self.received_packets, self.received_errors, self.received_drops,
         self.received_fifo, self.received_frame, self.received_compressed, self.multicast,
         self.transmitted_bytes, self.transmitted_packets, self.transmitted_errors, self.transmitted_drops,
         self.transmitted_fifo, self.collisions, self.transmitted_carrier, self.transmitted_compressed]
    }
    fn from_values(v: [u64; 16]) -> Self {
        Self {
            received_bytes: v[0], received_packets: v[1], received_errors: v[2], received_drops: v[3],
            received_fifo: v[4], received_frame: v[5], received_compressed: v[6], multicast: v[7],
            transmitted_bytes: v[8], transmitted_packets: v[9], transmitted_errors: v[10], transmitted_drops: v[11],
            transmitted_fifo: v[12], collisions: v[13], transmitted_carrier: v[14], transmitted_compressed: v[15],
        }
    }
    /// `None` indicates counter reset or wrap. Interface identity must be checked by the caller.
    pub fn delta_since(self, old: Self) -> Option<Self> {
        let mut delta = [0; 16];
        for (out, (now, before)) in delta.iter_mut().zip(self.values().into_iter().zip(old.values())) {
            *out = now.checked_sub(before)?;
        }
        Some(Self::from_values(delta))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawNetwork<'a> { pub name: &'a [u8], pub counters: NetworkCounters }

pub fn parse_net_dev_line(line: &[u8]) -> Result<RawNetwork<'_>, ParseError> {
    let colon = line.iter().rposition(|b| *b == b':').ok_or(ParseError::InvalidRecord)?;
    let name = line[..colon].trim_ascii();
    if name.is_empty() { return Err(ParseError::InvalidRecord); }
    let mut it = fields(&line[colon + 1..]);
    let mut values = [0; 16];
    for value in &mut values { *value = number(it.next().ok_or(ParseError::MissingField)?)?; }
    Ok(RawNetwork { name, counters: NetworkCounters::from_values(values) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fields_and_rollbacks() {
        let raw = parse_net_dev_line(b"  eth0: 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16").unwrap();
        assert_eq!(raw.name, b"eth0");
        assert_eq!(raw.counters.transmitted_bytes, 9);
        assert_eq!(raw.counters.transmitted_packets, 10);
        assert_eq!(raw.counters.transmitted_compressed, 16);
        assert_eq!(raw.counters.delta_since(raw.counters), Some(NetworkCounters::default()));
        assert!(NetworkCounters::default().delta_since(raw.counters).is_none());
        assert!(parse_net_dev_line(b"eth0: 1 2 3").is_err());
        assert!(parse_net_dev_line(b": 1 2 3").is_err());
    }
}
