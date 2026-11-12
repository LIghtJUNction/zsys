//! Block-device I/O and filesystem-capacity models. No allocation or OS calls.
use crate::parse::{ParseError, fields, number, split_at_byte};

/// Linux diskstats sectors always denote 512 bytes, not the device's physical sector size.
pub const DISKSTATS_SECTOR_BYTES: u64 = 512;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiscardCounters {
    pub completed: u64,
    pub merged: u64,
    pub sectors: u64,
    pub time_ms: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlushCounters {
    pub completed: u64,
    pub time_ms: u64,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiskCounters {
    pub reads: u64,
    pub reads_merged: u64,
    pub sectors_read: u64,
    pub read_time_ms: u64,
    pub writes: u64,
    pub writes_merged: u64,
    pub sectors_written: u64,
    pub write_time_ms: u64,
    /// Gauge, not a cumulative counter. Never subtract it when calculating I/O rates.
    pub in_flight: u64,
    pub io_time_ms: u64,
    pub weighted_io_time_ms: u64,
    pub discards: Option<DiscardCounters>,
    pub flushes: Option<FlushCounters>,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DiskDelta {
    pub reads: u64,
    pub writes: u64,
    pub read_bytes: u64,
    pub written_bytes: u64,
    pub read_time_ms: u64,
    pub write_time_ms: u64,
    pub io_time_ms: u64,
    pub weighted_io_time_ms: u64,
}
impl DiskCounters {
    pub fn read_bytes(self) -> Option<u64> {
        self.sectors_read.checked_mul(DISKSTATS_SECTOR_BYTES)
    }
    pub fn written_bytes(self) -> Option<u64> {
        self.sectors_written.checked_mul(DISKSTATS_SECTOR_BYTES)
    }
    /// A reset or arithmetic overflow invalidates the delta rather than becoming a large rate.
    pub fn delta_since(self, old: Self) -> Option<DiskDelta> {
        Some(DiskDelta {
            reads: self.reads.checked_sub(old.reads)?,
            writes: self.writes.checked_sub(old.writes)?,
            read_bytes: self.sectors_read.checked_sub(old.sectors_read)?.checked_mul(DISKSTATS_SECTOR_BYTES)?,
            written_bytes: self
                .sectors_written
                .checked_sub(old.sectors_written)?
                .checked_mul(DISKSTATS_SECTOR_BYTES)?,
            read_time_ms: self.read_time_ms.checked_sub(old.read_time_ms)?,
            write_time_ms: self.write_time_ms.checked_sub(old.write_time_ms)?,
            io_time_ms: self.io_time_ms.checked_sub(old.io_time_ms)?,
            weighted_io_time_ms: self.weighted_io_time_ms.checked_sub(old.weighted_io_time_ms)?,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Capacity {
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// Space available to unprivileged users, distinct from all free blocks.
    pub available_bytes: u64,
    pub total_inodes: u64,
    pub free_inodes: u64,
    pub available_inodes: u64,
    pub filesystem_id: u64,
    pub read_only: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawDisk<'a> {
    pub major: u32,
    pub minor: u32,
    pub name: &'a [u8],
    pub counters: DiskCounters,
}
pub fn parse_diskstats_line(line: &[u8]) -> Result<RawDisk<'_>, ParseError> {
    let mut it = fields(line);
    let mut integer = || -> Result<u32, ParseError> {
        u32::try_from(number(it.next().ok_or(ParseError::MissingField)?)?).map_err(|_| ParseError::InvalidNumber)
    };
    let major = integer()?;
    let minor = integer()?;
    let name = it.next().ok_or(ParseError::MissingField)?;
    let mut values = [0; 17];
    for value in &mut values[..11] {
        *value = number(it.next().ok_or(ParseError::MissingField)?)?;
    }
    let discards = if let Some(first) = it.next() {
        values[11] = number(first)?;
        for value in &mut values[12..15] {
            *value = number(it.next().ok_or(ParseError::MissingField)?)?;
        }
        Some(DiscardCounters { completed: values[11], merged: values[12], sectors: values[13], time_ms: values[14] })
    } else {
        None
    };
    let flushes = if let Some(first) = it.next() {
        Some(FlushCounters { completed: number(first)?, time_ms: number(it.next().ok_or(ParseError::MissingField)?)? })
    } else {
        None
    };
    Ok(RawDisk {
        major,
        minor,
        name,
        counters: DiskCounters {
            reads: values[0],
            reads_merged: values[1],
            sectors_read: values[2],
            read_time_ms: values[3],
            writes: values[4],
            writes_merged: values[5],
            sectors_written: values[6],
            write_time_ms: values[7],
            in_flight: values[8],
            io_time_ms: values[9],
            weighted_io_time_ms: values[10],
            discards,
            flushes,
        },
    })
}

/// Fields are still procfs-escaped; unescape only when constructing an owned OS path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawMount<'a> {
    pub id: u64,
    pub parent_id: u64,
    pub major: u32,
    pub minor: u32,
    pub root: &'a [u8],
    pub mount_point: &'a [u8],
    pub options: &'a [u8],
    pub filesystem: &'a [u8],
    pub source: &'a [u8],
    pub super_options: &'a [u8],
}
pub fn parse_mountinfo_line(line: &[u8]) -> Result<RawMount<'_>, ParseError> {
    let mut it = fields(line);
    let id = number(it.next().ok_or(ParseError::MissingField)?)?;
    let parent_id = number(it.next().ok_or(ParseError::MissingField)?)?;
    let device = it.next().ok_or(ParseError::MissingField)?;
    let (major, minor) = split_at_byte(device, b':').ok_or(ParseError::InvalidRecord)?;
    let major = u32::try_from(number(major)?).map_err(|_| ParseError::InvalidNumber)?;
    let minor = u32::try_from(number(minor)?).map_err(|_| ParseError::InvalidNumber)?;
    let root = it.next().ok_or(ParseError::MissingField)?;
    let mount_point = it.next().ok_or(ParseError::MissingField)?;
    let options = it.next().ok_or(ParseError::MissingField)?;
    if !it.any(|field| field == b"-") {
        return Err(ParseError::MissingField);
    }
    let filesystem = it.next().ok_or(ParseError::MissingField)?;
    let source = it.next().ok_or(ParseError::MissingField)?;
    let super_options = it.next().ok_or(ParseError::MissingField)?;
    Ok(RawMount { id, parent_id, major, minor, root, mount_point, options, filesystem, source, super_options })
}

/// Decode the four kernel mount-field escapes into caller-owned storage.
/// Returns the number of written bytes; rejects malformed escapes and insufficient output space.
pub fn unescape_mount_field(input: &[u8], output: &mut [u8]) -> Result<usize, ParseError> {
    let mut i = 0;
    let mut used = 0;
    while i < input.len() {
        let byte = if input[i] == b'\\' {
            let escape = input.get(i + 1..i + 4).ok_or(ParseError::InvalidRecord)?;
            let value = match escape {
                b"040" => b' ',
                b"011" => b'\t',
                b"012" => b'\n',
                b"134" => b'\\',
                _ => return Err(ParseError::InvalidRecord),
            };
            i += 4;
            value
        } else {
            let value = input[i];
            i += 1;
            value
        };
        *output.get_mut(used).ok_or(ParseError::MissingField)? = byte;
        used += 1;
    }
    Ok(used)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_optional_io_fields_and_keeps_inflight_as_gauge() {
        let raw = parse_diskstats_line(b"8 0 sda 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17").unwrap();
        assert_eq!(raw.counters.read_bytes(), Some(1536));
        assert_eq!(raw.counters.written_bytes(), Some(3584));
        assert_eq!(raw.counters.discards.unwrap().sectors, 14);
        assert_eq!(raw.counters.flushes.unwrap().time_ms, 17);
        let new = DiskCounters { in_flight: 0, ..raw.counters };
        assert_eq!(new.delta_since(raw.counters), Some(DiskDelta::default()));
        assert!(DiskCounters::default().delta_since(raw.counters).is_none());
        assert!(parse_diskstats_line(b"8 0 sda 1 2 3").is_err());
        assert!(parse_diskstats_line(b"8 0 sda 1 2 3 4 5 6 7 8 9 10 11 12").is_err());
    }
    #[test]
    fn parses_optional_mount_fields_and_escaped_paths() {
        let raw = parse_mountinfo_line(b"42 1 8:2 / /space\\040name rw shared:1 - ext4 /dev/sda2 rw").unwrap();
        assert_eq!((raw.id, raw.major, raw.minor), (42, 8, 2));
        assert_eq!(raw.filesystem, b"ext4");
        let mut out = [0; 64];
        let used = unescape_mount_field(raw.mount_point, &mut out).unwrap();
        assert_eq!(&out[..used], b"/space name");
        assert!(unescape_mount_field(b"bad\\000", &mut out).is_err());
        assert!(unescape_mount_field(b"long", &mut [0; 1]).is_err());
        assert!(parse_mountinfo_line(b"1 2 8:0 / / rw").is_err());
    }
}
