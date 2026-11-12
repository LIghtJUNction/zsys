//! Allocation-free procfs parsers. These also compile on non-Linux, no_std targets.

use crate::{
    cpu::CpuTimes,
    memory::Memory,
    process::{ProcessCounters, ProcessId, ProcessState},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError {
    MissingField,
    InvalidNumber,
    InvalidRecord,
}

impl core::fmt::Display for ParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid procfs data: {self:?}")
    }
}

impl core::error::Error for ParseError {}

pub(crate) fn number(bytes: &[u8]) -> Result<u64, ParseError> {
    if bytes.is_empty() {
        return Err(ParseError::InvalidNumber);
    }
    let mut value = 0u64;
    // Any 19 decimal digits fit in u64. Keep the checked path for longer
    // inputs, including valid values with arbitrarily many leading zeroes.
    if bytes.len() <= 19 {
        for &byte in bytes {
            let digit = byte.wrapping_sub(b'0');
            if digit > 9 {
                return Err(ParseError::InvalidNumber);
            }
            value = value * 10 + u64::from(digit);
        }
        return Ok(value);
    }
    for &b in bytes {
        if !b.is_ascii_digit() {
            return Err(ParseError::InvalidNumber);
        }
        value =
            value.checked_mul(10).and_then(|v| v.checked_add(u64::from(b - b'0'))).ok_or(ParseError::InvalidNumber)?;
    }
    Ok(value)
}

pub(crate) fn fields(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(u8::is_ascii_whitespace).filter(|s| !s.is_empty())
}

pub(crate) fn split_at_byte(bytes: &[u8], delimiter: u8) -> Option<(&[u8], &[u8])> {
    let index = bytes.iter().position(|b| *b == delimiter)?;
    Some((&bytes[..index], &bytes[index + 1..]))
}

fn next_number<'a>(it: &mut impl Iterator<Item = &'a [u8]>) -> Result<u64, ParseError> {
    number(it.next().ok_or(ParseError::MissingField)?)
}

pub fn parse_cpu_stat(input: &[u8]) -> Result<CpuTimes, ParseError> {
    let line = input.split(|b| *b == b'\n').next().ok_or(ParseError::InvalidRecord)?;
    let mut it = fields(line);
    if it.next() != Some(&b"cpu"[..]) {
        return Err(ParseError::InvalidRecord);
    }
    cpu_fields(it)
}

/// Parses counters after a `cpu` or `cpuN` label; guest fields are not counted twice.
pub(crate) fn cpu_fields<'a>(mut it: impl Iterator<Item = &'a [u8]>) -> Result<CpuTimes, ParseError> {
    let user = next_number(&mut it)?;
    let nice = next_number(&mut it)?;
    let system = next_number(&mut it)?;
    let idle = next_number(&mut it)?;
    let mut optional = || it.next().map(number).unwrap_or(Ok(0));
    Ok(CpuTimes {
        user,
        nice,
        system,
        idle,
        iowait: optional()?,
        irq: optional()?,
        softirq: optional()?,
        steal: optional()?,
    })
}

pub fn parse_meminfo(input: &[u8]) -> Result<Memory, ParseError> {
    meminfo(input).map(|(memory, _)| memory)
}

pub(crate) fn meminfo(input: &[u8]) -> Result<(Memory, bool), ParseError> {
    let mut out = Memory::default();
    let mut has_available = false;
    let mut has_free = false;
    let mut reclaimable = 0u64;
    let mut shared = 0u64;
    for line in input.split(|b| *b == b'\n') {
        let Some((key, rest)) = split_at_byte(line, b':') else {
            continue;
        };
        let slot = match key {
            b"MemTotal" => &mut out.total_bytes,
            b"MemAvailable" => {
                has_available = true;
                &mut out.available_bytes
            }
            b"MemFree" => {
                has_free = true;
                &mut out.free_bytes
            }
            b"Buffers" => &mut out.buffers_bytes,
            b"Cached" => &mut out.cached_bytes,
            b"SReclaimable" => &mut reclaimable,
            b"Shmem" => &mut shared,
            b"SwapTotal" => &mut out.swap_total_bytes,
            b"SwapFree" => &mut out.swap_free_bytes,
            _ => continue,
        };
        let mut values = fields(rest);
        let value = next_number(&mut values)?;
        if values.next() != Some(&b"kB"[..]) {
            return Err(ParseError::InvalidRecord);
        }
        *slot = value.checked_mul(1024).ok_or(ParseError::InvalidNumber)?;
    }
    if out.total_bytes == 0 || (!has_available && !has_free) {
        return Err(ParseError::MissingField);
    }
    if !has_available {
        out.available_bytes = out
            .free_bytes
            .saturating_add(out.buffers_bytes)
            .saturating_add(out.cached_bytes)
            .saturating_add(reclaimable)
            .saturating_sub(shared)
            .min(out.total_bytes);
    }
    Ok((out, !has_available))
}

/// Borrowed process record. `name` is raw bytes: procfs does not guarantee UTF-8.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawProcess<'a> {
    pub id: ProcessId,
    pub name: &'a [u8],
    pub parent_pid: u32,
    pub state: ProcessState,
    pub threads: u32,
    pub counters: ProcessCounters,
}

pub fn process_stat(input: &[u8]) -> Result<RawProcess<'_>, ParseError> {
    let open = input.iter().position(|b| *b == b'(').ok_or(ParseError::InvalidRecord)?;
    let close = input.iter().rposition(|b| *b == b')').ok_or(ParseError::InvalidRecord)?;
    if open == 0 || close <= open || input.get(close + 1) != Some(&b' ') {
        return Err(ParseError::InvalidRecord);
    }
    let pid = u32::try_from(number(input[..open].trim_ascii())?).map_err(|_| ParseError::InvalidNumber)?;
    let mut it = fields(&input[close + 2..]);
    let state = match it.next().ok_or(ParseError::MissingField)? {
        b"R" => ProcessState::Running,
        b"S" => ProcessState::Sleeping,
        b"D" => ProcessState::DiskSleep,
        b"Z" => ProcessState::Zombie,
        b"T" => ProcessState::Stopped,
        b"t" => ProcessState::Tracing,
        b"X" | b"x" => ProcessState::Dead,
        b"I" => ProcessState::Idle,
        other if other.len() == 1 => ProcessState::Unknown,
        _ => return Err(ParseError::InvalidRecord),
    };
    let parent_pid = u32::try_from(next_number(&mut it)?).map_err(|_| ParseError::InvalidNumber)?;
    let user_ticks = number(it.nth(9).ok_or(ParseError::MissingField)?)?;
    let system_ticks = next_number(&mut it)?;
    let threads =
        u32::try_from(number(it.nth(4).ok_or(ParseError::MissingField)?)?).map_err(|_| ParseError::InvalidNumber)?;
    let start_time_ticks = number(it.nth(1).ok_or(ParseError::MissingField)?)?;
    let virtual_bytes = next_number(&mut it)?;
    let resident_pages = next_number(&mut it)?;
    Ok(RawProcess {
        id: ProcessId { pid, start_time_ticks },
        name: &input[open + 1..close],
        parent_pid,
        state,
        threads,
        counters: ProcessCounters { user_ticks, system_ticks, virtual_bytes, resident_pages },
    })
}

/// Compatibility wrapper around the richer borrowed parser.
pub fn parse_process_stat(input: &[u8]) -> Result<(u32, ProcessState, ProcessCounters, u32, u64), ParseError> {
    let p = process_stat(input)?;
    Ok((p.id.pid, p.state, p.counters, p.parent_pid, p.id.start_time_ticks))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_meminfo() {
        let m = parse_meminfo(b"MemTotal: 1000 kB\nMemAvailable: 400 kB\nSwapTotal: 20 kB\nSwapFree: 5 kB\n").unwrap();
        assert_eq!(m.total_bytes, 1_024_000);
        assert_eq!(m.used_bytes(), 614_400);
        assert_eq!(m.swap_used_bytes(), 15_360);
    }

    #[test]
    fn derives_available_memory_when_kernel_omits_it() {
        let m = parse_meminfo(
            b"MemTotal: 1000 kB\nMemFree: 100 kB\nBuffers: 20 kB\nCached: 200 kB\nSReclaimable: 30 kB\nShmem: 10 kB\n",
        )
        .unwrap();
        assert_eq!(m.available_bytes, 340 * 1024);
    }

    #[test]
    fn parses_cpu_stat() {
        let c = parse_cpu_stat(b"cpu 10 2 3 40 5 6 7 8 9 10\ncpu0 1 2 3 4").unwrap();
        assert_eq!(c.user, 10);
        assert_eq!(c.steal, 8);
        assert_eq!(c.total(), 81);
    }

    #[test]
    fn parses_process_stat_with_parentheses_in_comm() {
        let line = b"42 (worker (io)) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 2000 5";
        let (pid, state, counters, ppid, start) = parse_process_stat(line).unwrap();
        assert_eq!(pid, 42);
        assert_eq!(state, ProcessState::Sleeping);
        assert_eq!(ppid, 1);
        assert_eq!(counters.user_ticks, 11);
        assert_eq!(counters.system_ticks, 12);
        assert_eq!(start, 19);
        assert_eq!(counters.virtual_bytes, 2000);
        assert_eq!(counters.resident_pages, 5);
        assert_eq!(process_stat(line).unwrap().threads, 17);
        assert_eq!(process_stat(line).unwrap().name, b"worker (io)");
    }

    #[test]
    fn rejects_truncated_overflow_and_wrong_units() {
        assert!(parse_cpu_stat(b"cpu 1 2").is_err());
        assert!(parse_meminfo(b"MemTotal: 1000 kB\n").is_err());
        assert!(parse_meminfo(b"MemTotal: 1 GB\nMemFree: 1 kB").is_err());
        assert!(number(b"18446744073709551616").is_err());
        assert_eq!(number(b"9999999999999999999"), Ok(9_999_999_999_999_999_999));
        assert_eq!(number(b"18446744073709551615"), Ok(u64::MAX));
        assert_eq!(number(b"000000000000000000000000001"), Ok(1));
        assert!(number(b"999999999999999999x").is_err());
        assert!(process_stat(b"4294967296 (x) R 1").is_err());
        assert!(process_stat(b"1 (x) R 1").is_err());
    }

    #[test]
    fn arbitrary_input_never_panics() {
        let mut input = [0u8; 128];
        for seed in 0..128u8 {
            for (i, byte) in input.iter_mut().enumerate() {
                *byte = seed.wrapping_mul(31).wrapping_add(i as u8);
            }
            for len in 0..input.len() {
                let _ = process_stat(&input[..len]);
                let _ = parse_meminfo(&input[..len]);
                let _ = parse_cpu_stat(&input[..len]);
            }
        }
    }
}
