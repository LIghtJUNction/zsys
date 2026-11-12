//! Incremental PID table. Enumeration and details are independent operations.
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::Instant;

use super::LinuxError;
use super::io::{Buffer, Directory, ScanMode, clock_parameters, open_at, pid_path};
use crate::parse::{fields, number, process_stat};
use crate::process::{ProcessCounters, ProcessId, ProcessState};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Availability {
    Fresh,
    Inaccessible,
    Vanished,
    Invalid,
}

#[derive(Clone, Debug)]
pub struct Process {
    pub id: ProcessId,
    pub parent_pid: u32,
    /// Kernel `comm` bytes, not necessarily UTF-8 or the full executable name.
    pub name: Vec<u8>,
    pub state: ProcessState,
    pub threads: u32,
    pub counters: ProcessCounters,
    /// Resident bytes from `/proc/PID/statm`, matching the kernel's resident-set interface.
    /// Linux still documents this as approximate; `counters.resident_pages` preserves `stat`.
    pub memory_bytes: u64,
    pub cpu_percent: Option<f32>,
    pub sampled_at_ns: u64,
    pub availability: Availability,
    seen: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessChanges {
    pub added: usize,
    /// Successful samples, including newly added and reused PIDs.
    pub updated: usize,
    pub removed: usize,
    pub reused: usize,
    pub failed: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessFailure {
    pub pid: u32,
    pub reason: Availability,
}

// Kept separate from the clonable public snapshot: cloning a Process never
// extends the lifetime of a kernel descriptor.
struct Entry {
    process: Process,
    directory: Option<File>,
}

pub struct Processes {
    directory: Directory,
    buffer: Buffer,
    pids: Vec<u32>,
    table: HashMap<u32, Entry>,
    directory_cache_limit: usize,
    cached_directories: usize,
    failures: Vec<ProcessFailure>,
    mode: ScanMode,
    generation: u32,
    started: Instant,
    ticks_per_second: u64,
    page_size: u64,
}

impl Processes {
    pub fn new() -> Result<Self, LinuxError> {
        Self::with_proc_root("/proc")
    }
    pub fn with_proc_root(root: impl AsRef<Path>) -> Result<Self, LinuxError> {
        let (ticks, pages) = clock_parameters()?;
        Self::with_parameters(root, ticks, pages)
    }
    /// Explicit units make fixtures deterministic; no machine-specific values are hardcoded in data parsing.
    pub fn with_parameters(root: impl AsRef<Path>, ticks_per_second: u64, page_size: u64) -> Result<Self, LinuxError> {
        if ticks_per_second == 0 || page_size == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "clock/page units must be nonzero").into());
        }
        Ok(Self {
            directory: Directory::new(root.as_ref())?,
            buffer: Buffer::new(4096, 1024 * 1024),
            pids: Vec::new(),
            table: HashMap::new(),
            directory_cache_limit: 0,
            cached_directories: 0,
            failures: Vec::new(),
            mode: if cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
                ScanMode::Native
            } else {
                ScanMode::Std
            },
            generation: 0,
            started: Instant::now(),
            ticks_per_second,
            page_size,
        })
    }
    pub fn set_scan_mode(&mut self, mode: ScanMode) {
        self.mode = mode;
    }
    pub fn iter(&self) -> impl Iterator<Item = &Process> {
        self.table.values().map(|entry| &entry.process)
    }
    pub fn get(&self, pid: u32) -> Option<&Process> {
        self.table.get(&pid).map(|entry| &entry.process)
    }
    pub fn len(&self) -> usize {
        self.table.len()
    }
    pub fn is_empty(&self) -> bool {
        self.table.is_empty()
    }
    pub fn failures(&self) -> &[ProcessFailure] {
        &self.failures
    }
    pub fn ticks_per_second(&self) -> u64 {
        self.ticks_per_second
    }
    pub fn page_size(&self) -> u64 {
        self.page_size
    }

    /// Opts into reusing at most `limit` PID directory descriptors. Disabled by default.
    ///
    /// Each sample still opens and reads fresh `statm` and `stat` files, so their
    /// access checks are not bypassed. Only directory lookup/open/close is saved.
    /// The first successfully sampled PIDs fill the budget; there is no LRU churn.
    /// Slots are released on failed samples, observed exit, reconfiguration, or drop.
    /// Reconfiguration closes all cached descriptors, without discarding snapshots.
    ///
    /// Use a stable, genuine procfs mount: Linux pins an open PID directory to its
    /// original process even after PID reuse. Synthetic roots that rename live PID
    /// directories should leave caching disabled. Choose a budget below the caller's
    /// descriptor limit; this collector never changes that process-wide limit.
    pub fn set_directory_cache_limit(&mut self, limit: usize) {
        for entry in self.table.values_mut() {
            entry.directory = None;
        }
        self.cached_directories = 0;
        self.directory_cache_limit = limit;
    }

    pub fn directory_cache_limit(&self) -> usize {
        self.directory_cache_limit
    }

    /// Number of currently retained PID directory descriptors (excluding the proc root).
    pub fn cached_directory_count(&self) -> usize {
        self.cached_directories
    }

    /// Lists numeric PIDs without reading their records or changing the process table.
    /// The returned list is sorted, unique, and borrowed from reusable storage.
    /// A PID can exit immediately after enumeration; use `refresh_pid` to read it.
    pub fn enumerate_pids(&mut self) -> Result<&[u32], LinuxError> {
        self.directory.pids(&mut self.pids, self.mode)?;
        Ok(&self.pids)
    }

    /// Enumerates first: a failed directory scan never deletes the existing process table.
    /// Unreadable but still enumerated PIDs retain explicitly stale records, not fake zeros.
    pub fn refresh(&mut self) -> Result<ProcessChanges, LinuxError> {
        self.directory.pids(&mut self.pids, self.mode)?;
        self.generation = self.generation.wrapping_add(1);
        self.failures.clear();
        let mut changes = ProcessChanges::default();
        for index in 0..self.pids.len() {
            let pid = self.pids[index];
            match self.update(pid) {
                Ok((added, reused)) => {
                    changes.added += usize::from(added);
                    changes.reused += usize::from(reused);
                    changes.updated += 1;
                }
                Err(error) => {
                    let reason = classify(&error);
                    if let Some(previous) = self.table.get_mut(&pid) {
                        previous.process.seen = self.generation;
                        previous.process.availability = reason;
                        previous.process.cpu_percent = None;
                    }
                    self.failures.push(ProcessFailure { pid, reason });
                    changes.failed += 1;
                }
            }
        }
        let old = self.table.len();
        self.table.retain(|_, entry| {
            let keep = entry.process.seen == self.generation;
            if !keep && entry.directory.is_some() {
                self.cached_directories -= 1;
            }
            keep
        });
        changes.removed = old - self.table.len();
        Ok(changes)
    }

    /// Updates only this PID. Other PIDs are untouched, even if they have exited.
    pub fn refresh_pid(&mut self, pid: u32) -> Result<&Process, LinuxError> {
        if let Err(error) = self.update(pid) {
            if classify(&error) == Availability::Vanished {
                self.table.remove(&pid);
            } else if let Some(previous) = self.table.get_mut(&pid) {
                previous.process.availability = classify(&error);
                previous.process.cpu_percent = None;
            }
            return Err(error);
        }
        Ok(&self.table.get(&pid).expect("successful update inserts the record").process)
    }

    fn update(&mut self, pid: u32) -> Result<(bool, bool), LinuxError> {
        let mut path = [0; 64];
        // A procfs directory descriptor remains tied to this process after PID reuse.
        // Both reads must use it so a dying PID cannot combine two processes' data.
        let cached = if self.directory_cache_limit == 0 {
            None
        } else {
            self.table.get_mut(&pid).and_then(|entry| entry.directory.take())
        };
        let was_cached = cached.is_some();
        self.cached_directories -= usize::from(was_cached);
        let mut directory = match cached {
            Some(directory) => directory,
            None => open_at(&self.directory.file, pid_path(pid, b"", &mut path))?,
        };
        let memory_bytes = {
            let file = match open_at(&directory, c"statm") {
                // A dead cached process can no longer resolve children. Reopen the
                // PID once, then sample BOTH files through the new pinned directory.
                Err(error)
                    if was_cached && (error.kind() == io::ErrorKind::NotFound || error.raw_os_error() == Some(3)) =>
                {
                    drop(directory);
                    directory = open_at(&self.directory.file, pid_path(pid, b"", &mut path))?;
                    open_at(&directory, c"statm")?
                }
                result => result?,
            };
            let mut values = fields(self.buffer.read(&file)?);
            let _virtual_pages = number(values.next().ok_or(crate::parse::ParseError::MissingField)?)?;
            number(values.next().ok_or(crate::parse::ParseError::MissingField)?)?
                .checked_mul(self.page_size)
                .ok_or(crate::parse::ParseError::InvalidNumber)?
        };
        let file = open_at(&directory, c"stat")?;
        let raw = process_stat(self.buffer.read(&file)?)?;
        if raw.id.pid != pid {
            return Err(crate::parse::ParseError::InvalidRecord.into());
        }
        let now = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let mut added = false;
        let entry = self.table.entry(pid).or_insert_with(|| {
            added = true;
            Entry {
                directory: None,
                process: Process {
                    id: raw.id,
                    parent_pid: raw.parent_pid,
                    name: Vec::with_capacity(raw.name.len()),
                    state: raw.state,
                    threads: raw.threads,
                    counters: raw.counters,
                    memory_bytes,
                    cpu_percent: None,
                    sampled_at_ns: now,
                    availability: Availability::Fresh,
                    seen: self.generation,
                },
            }
        });
        if self.cached_directories < self.directory_cache_limit {
            entry.directory = Some(directory);
            self.cached_directories += 1;
        }
        let entry = &mut entry.process;
        let reused = !added && entry.id != raw.id;
        entry.cpu_percent = if added || reused || entry.availability != Availability::Fresh {
            None
        } else {
            now.checked_sub(entry.sampled_at_ns)
                .and_then(|elapsed| raw.counters.cpu_percent_since(entry.counters, elapsed, self.ticks_per_second))
        };
        entry.id = raw.id;
        entry.parent_pid = raw.parent_pid;
        entry.name.clear();
        entry.name.extend_from_slice(raw.name);
        entry.state = raw.state;
        entry.threads = raw.threads;
        entry.counters = raw.counters;
        entry.memory_bytes = memory_bytes;
        entry.sampled_at_ns = now;
        entry.availability = Availability::Fresh;
        entry.seen = self.generation;
        Ok((added, reused))
    }

    /// Reads metadata through one opened PID directory, then checks the same identity again.
    /// This avoids attaching a new process's details to a reused PID.
    pub fn details(&mut self, pid: u32, options: DetailOptions) -> Result<ProcessDetails, LinuxError> {
        let mut path = [0; 64];
        let directory = open_at(&self.directory.file, pid_path(pid, b"", &mut path))?;
        let stat = open_at(&directory, c"stat")?;
        let identity = process_stat(self.buffer.read(&stat)?)?.id;
        if identity.pid != pid {
            return Err(crate::parse::ParseError::InvalidRecord.into());
        }
        let mut result = ProcessDetails {
            id: identity,
            executable: Detail::NotRequested,
            command_line: Detail::NotRequested,
            user_ids: Detail::NotRequested,
            group_ids: Detail::NotRequested,
        };
        if options.executable {
            result.executable = detail(read_executable(&directory));
        }
        if options.command_line {
            result.command_line = detail((|| {
                let file = open_at(&directory, c"cmdline")?;
                let mut bytes = Buffer::new(4096, 16 * 1024 * 1024);
                Ok(bytes.read(&file)?.to_vec())
            })());
        }
        if options.user {
            match open_at(&directory, c"status").and_then(|file| {
                let bytes = self.buffer.read(&file)?;
                parse_ids(bytes).map_err(|_| io::ErrorKind::InvalidData.into())
            }) {
                Ok((uids, gids)) => {
                    result.user_ids = Detail::Value(uids);
                    result.group_ids = Detail::Value(gids);
                }
                Err(e) => {
                    result.user_ids = Detail::Unavailable(e.kind());
                    result.group_ids = Detail::Unavailable(e.kind());
                }
            }
        }
        if process_stat(self.buffer.read(&stat)?)?.id != identity {
            return Err(io::Error::new(io::ErrorKind::NotFound, "process identity changed during metadata read").into());
        }
        Ok(result)
    }
}

fn classify(error: &LinuxError) -> Availability {
    match error {
        // Open procfs descriptors for exited processes commonly report ESRCH.
        LinuxError::Io(e) if e.kind() == io::ErrorKind::NotFound || e.raw_os_error() == Some(3) => {
            Availability::Vanished
        }
        LinuxError::Io(e) if e.kind() == io::ErrorKind::PermissionDenied => Availability::Inaccessible,
        _ => Availability::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_generation_wrap_preserves_live_and_removes_absent_entries() {
        for limit in [0, 1] {
            let mut processes = Processes::new().unwrap();
            processes.set_directory_cache_limit(limit);
            processes.generation = u32::MAX;
            let pid = std::process::id();
            processes.refresh_pid(pid).unwrap();
            let phantom = processes.get(pid).unwrap().clone();
            // Deliberately absent PID, inserted without a directory to exercise retain.
            processes.table.insert(u32::MAX, Entry { process: phantom, directory: None });
            let changes = processes.refresh().unwrap();
            assert_eq!(processes.generation, 0);
            assert!(processes.get(pid).is_some());
            assert!(processes.get(u32::MAX).is_none());
            assert!(changes.removed >= 1);
            assert!(processes.cached_directory_count() <= limit);
        }
    }

    #[test]
    fn classifies_exited_and_inaccessible_processes() {
        assert_eq!(classify(&io::Error::from_raw_os_error(3).into()), Availability::Vanished);
        assert_eq!(classify(&io::Error::from_raw_os_error(2).into()), Availability::Vanished);
        assert_eq!(classify(&io::Error::from_raw_os_error(13).into()), Availability::Inaccessible);
        assert_eq!(classify(&crate::parse::ParseError::InvalidRecord.into()), Availability::Invalid);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DetailOptions {
    pub executable: bool,
    pub command_line: bool,
    pub user: bool,
}
impl DetailOptions {
    pub const fn all() -> Self {
        Self { executable: true, command_line: true, user: true }
    }
}
#[derive(Clone, Debug)]
pub enum Detail<T> {
    NotRequested,
    Unavailable(io::ErrorKind),
    Value(T),
}
fn detail<T>(value: io::Result<T>) -> Detail<T> {
    match value {
        Ok(v) => Detail::Value(v),
        Err(e) => Detail::Unavailable(e.kind()),
    }
}
#[derive(Clone, Debug)]
pub struct ProcessDetails {
    pub id: ProcessId,
    pub executable: Detail<PathBuf>,
    /// Raw NUL-separated arguments. Empty arguments and non-UTF-8 bytes are preserved.
    pub command_line: Detail<Vec<u8>>,
    /// Real, effective, saved-set and filesystem user/group IDs.
    pub user_ids: Detail<[u32; 4]>,
    pub group_ids: Detail<[u32; 4]>,
}
fn parse_ids(input: &[u8]) -> Result<([u32; 4], [u32; 4]), crate::parse::ParseError> {
    let mut uid = None;
    let mut gid = None;
    for line in input.split(|b| *b == b'\n') {
        let mut values = fields(line);
        let slot = match values.next() {
            Some(b"Uid:") => &mut uid,
            Some(b"Gid:") => &mut gid,
            _ => continue,
        };
        let mut ids = [0; 4];
        for id in &mut ids {
            *id = u32::try_from(number(values.next().ok_or(crate::parse::ParseError::MissingField)?)?)
                .map_err(|_| crate::parse::ParseError::InvalidNumber)?;
        }
        *slot = Some(ids);
    }
    Ok((uid.ok_or(crate::parse::ParseError::MissingField)?, gid.ok_or(crate::parse::ParseError::MissingField)?))
}
fn read_executable(directory: &File) -> io::Result<PathBuf> {
    use std::ffi::{OsString, c_char, c_int};
    use std::os::unix::ffi::OsStringExt;
    unsafe extern "C" {
        fn readlinkat(fd: c_int, path: *const c_char, buffer: *mut c_char, size: usize) -> isize;
    }
    let mut buffer = vec![0; 256];
    loop {
        // SAFETY: valid descriptor/name and writable initialized storage. readlinkat does not NUL-terminate.
        let len =
            unsafe { readlinkat(directory.as_raw_fd(), c"exe".as_ptr(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if len < 0 {
            return Err(io::Error::last_os_error());
        }
        if (len as usize) < buffer.len() {
            buffer.truncate(len as usize);
            return Ok(PathBuf::from(OsString::from_vec(buffer)));
        }
        if buffer.len() >= 1024 * 1024 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        buffer.resize(buffer.len() * 2, 0);
    }
}
