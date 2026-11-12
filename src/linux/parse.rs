//! Allocation-free parsers for selected Linux procfs records.

use crate::{cpu::CpuTimes, memory::Memory, process::{ProcessCounters, ProcessState}};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseError { MissingField, InvalidNumber, InvalidRecord }

fn number(bytes: &[u8]) -> Result<u64, ParseError> {
    if bytes.is_empty() { return Err(ParseError::InvalidNumber); }
    let mut value = 0u64;
    for &b in bytes {
        if !b.is_ascii_digit() { return Err(ParseError::InvalidNumber); }
        value = value.checked_mul(10).and_then(|v| v.checked_add((b - b'0') as u64))
            .ok_or(ParseError::InvalidNumber)?;
    }
    Ok(value)
}
fn fields(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(|b| b.is_ascii_whitespace()).filter(|s| !s.is_empty())
}

pub fn parse_cpu_stat(input: &[u8]) -> Result<CpuTimes, ParseError> {
    let line = input.split(|b| *b == b'\n').next().ok_or(ParseError::InvalidRecord)?;
    let mut it = fields(line);
    if it.next() != Some(&b"cpu"[..]) { return Err(ParseError::InvalidRecord); }
    let mut next = || it.next().map(number).unwrap_or(Ok(0));
    Ok(CpuTimes { user: next()?, nice: next()?, system: next()?, idle: next()?,
        iowait: next()?, irq: next()?, softirq: next()?, steal: next()? })
}

pub fn parse_meminfo(input: &[u8]) -> Result<Memory, ParseError> {
    let mut out = Memory::default();
    for line in input.split(|b| *b == b'\n') {
        let Some((key, rest)) = line.split_once(|b| *b == b':') else { continue };
        let value = fields(rest).next().map(number).transpose()?.unwrap_or(0).saturating_mul(1024);
        match key {
            b"MemTotal" => out.total_bytes = value,
            b"MemAvailable" => out.available_bytes = value,
            b"MemFree" => out.free_bytes = value,
            b"Buffers" => out.buffers_bytes = value,
            b"Cached" => out.cached_bytes = value,
            b"SwapTotal" => out.swap_total_bytes = value,
            b"SwapFree" => out.swap_free_bytes = value,
            _ => {}
        }
    }
    if out.total_bytes == 0 { return Err(ParseError::MissingField); }
    Ok(out)
}

pub fn parse_process_stat(input: &[u8]) -> Result<(u32, ProcessState, ProcessCounters, u32, u64), ParseError> {
    let open = input.iter().position(|b| *b == b'(').ok_or(ParseError::InvalidRecord)?;
    let close = input.iter().rposition(|b| *b == b')').ok_or(ParseError::InvalidRecord)?;
    if close <= open { return Err(ParseError::InvalidRecord); }
    let pid = number(&input[..open.saturating_sub(1)])? as u32;
    let state = match input.get(close + 2).copied().ok_or(ParseError::MissingField)? {
        b'R' => ProcessState::Running, b'S' => ProcessState::Sleeping,
        b'D' => ProcessState::DiskSleep, b'Z' => ProcessState::Zombie,
        b'T' => ProcessState::Stopped, b't' => ProcessState::Tracing,
        b'X' | b'x' => ProcessState::Dead, _ => ProcessState::Unknown,
    };
    let mut it = fields(input.get(close + 2..).ok_or(ParseError::MissingField)?);
    it.next().ok_or(ParseError::MissingField)?;
    let ppid = number(it.next().ok_or(ParseError::MissingField)?)? as u32;
    for _ in 0..9 { it.next().ok_or(ParseError::MissingField)?; }
    let utime = number(it.next().ok_or(ParseError::MissingField)?)?;
    let stime = number(it.next().ok_or(ParseError::MissingField)?)?;
    for _ in 0..6 { it.next().ok_or(ParseError::MissingField)?; }
    let start = number(it.next().ok_or(ParseError::MissingField)?)?;
    let vsize = number(it.next().ok_or(ParseError::MissingField)?)?;
    let rss = number(it.next().ok_or(ParseError::MissingField)?)?;
    Ok((pid, state, ProcessCounters { user_ticks: utime, system_ticks: stime,
        virtual_bytes: vsize, resident_pages: rss }, ppid, start))
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
    fn parses_cpu_stat() {
        let c = parse_cpu_stat(b"cpu 10 2 3 40 5 6 7 8 9 10\ncpu0 1 2 3 4").unwrap();
        assert_eq!(c.user, 10);
        assert_eq!(c.steal, 8);
    }
}
