//! Independent block I/O, mount discovery and filesystem-capacity collectors.
use super::LinuxError;
use super::io::CachedFile;
use crate::Sample;
use crate::disk::{
    Capacity, DiskCounters, DiskDelta, parse_diskstats_line, parse_mountinfo_line, unescape_mount_field,
};
use crate::parse::ParseError;
use std::collections::HashMap;
use std::ffi::{CStr, CString, OsStr};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct DiskDevice {
    pub major: u32,
    pub minor: u32,
    pub name: Vec<u8>,
    pub counters: DiskCounters,
    pub delta: Option<DiskDelta>,
    pub interval_ns: Option<u64>,
    pub sample: Sample,
    seen: u64,
}
#[derive(Clone, Copy)]
struct StagedDisk {
    major: u32,
    minor: u32,
    name_start: usize,
    name_len: usize,
    counters: DiskCounters,
}
pub struct DiskIoCollector {
    file: CachedFile,
    table: HashMap<(u32, u32), DiskDevice>,
    staged: Vec<StagedDisk>,
    started: Instant,
    sample: Sample,
}
impl Default for DiskIoCollector {
    fn default() -> Self {
        Self::new()
    }
}
impl DiskIoCollector {
    pub fn new() -> Self {
        Self::with_proc_root("/proc")
    }
    pub fn with_proc_root(root: impl AsRef<Path>) -> Self {
        Self {
            file: CachedFile::new(root.as_ref().join("diskstats")),
            table: HashMap::new(),
            staged: Vec::new(),
            started: Instant::now(),
            sample: Sample::default(),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &DiskDevice> {
        self.table.values()
    }
    pub fn get(&self, major: u32, minor: u32) -> Option<&DiskDevice> {
        self.table.get(&(major, minor))
    }
    pub fn sample(&self) -> Sample {
        self.sample
    }
    /// Includes partitions and whole devices. Do not sum them indiscriminately: that double-counts I/O.
    pub fn refresh(&mut self) -> Result<(), LinuxError> {
        let bytes = self.file.read()?;
        self.staged.clear();
        for line in bytes.split(|b| *b == b'\n').filter(|s| !s.trim_ascii().is_empty()) {
            let raw = parse_diskstats_line(line)?;
            self.staged.push(StagedDisk {
                major: raw.major,
                minor: raw.minor,
                name_start: raw.name.as_ptr() as usize - bytes.as_ptr() as usize,
                name_len: raw.name.len(),
                counters: raw.counters,
            });
        }
        self.staged.sort_unstable_by_key(|d| (d.major, d.minor));
        if self.staged.windows(2).any(|d| (d[0].major, d[0].minor) == (d[1].major, d[1].minor)) {
            return Err(ParseError::InvalidRecord.into());
        }
        let now = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let sequence = self.sample.sequence.wrapping_add(1);
        for raw in &self.staged {
            let name = &bytes[raw.name_start..raw.name_start + raw.name_len];
            let sample = Sample { timestamp_ns: now, sequence };
            if let Some(old) = self.table.get_mut(&(raw.major, raw.minor)) {
                old.delta = if old.name == name { raw.counters.delta_since(old.counters) } else { None };
                old.interval_ns = now.checked_sub(old.sample.timestamp_ns).filter(|n| *n != 0);
                old.name.clear();
                old.name.extend_from_slice(name);
                old.counters = raw.counters;
                old.sample = sample;
                old.seen = sequence;
            } else {
                self.table.insert(
                    (raw.major, raw.minor),
                    DiskDevice {
                        major: raw.major,
                        minor: raw.minor,
                        name: name.to_vec(),
                        counters: raw.counters,
                        delta: None,
                        interval_ns: None,
                        sample,
                        seen: sequence,
                    },
                );
            }
        }
        self.table.retain(|_, disk| disk.seen == sequence);
        self.sample = Sample { timestamp_ns: now, sequence };
        Ok(())
    }
}

/// One mounted path. Cached path bytes avoid allocation in repeated capacity queries.
#[derive(Debug)]
pub struct Filesystem {
    path: CString,
    capacity: Option<Capacity>,
    last_error: Option<io::ErrorKind>,
    started: Instant,
    sample: Sample,
}
impl Filesystem {
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = CString::new(path.as_ref().as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"))?;
        Ok(Self { path, capacity: None, last_error: None, started: Instant::now(), sample: Sample::default() })
    }
    pub fn path(&self) -> &Path {
        Path::new(OsStr::from_bytes(self.path.to_bytes()))
    }
    pub fn capacity(&self) -> Option<Capacity> {
        self.capacity
    }
    pub fn last_error(&self) -> Option<io::ErrorKind> {
        self.last_error
    }
    pub fn sample(&self) -> Sample {
        self.sample
    }
    /// Errors retain the previous capacity and timestamp; inspect `last_error` before using stale values.
    pub fn refresh(&mut self) -> io::Result<Capacity> {
        match native_capacity(&self.path) {
            Ok(value) => {
                self.capacity = Some(value);
                self.last_error = None;
                self.sample.sequence = self.sample.sequence.wrapping_add(1);
                self.sample.timestamp_ns = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
                Ok(value)
            }
            Err(e) => {
                self.last_error = Some(e.kind());
                Err(e)
            }
        }
    }
}

#[derive(Debug)]
pub struct Mount {
    pub id: u64,
    pub parent_id: u64,
    pub major: u32,
    pub minor: u32,
    pub root: Vec<u8>,
    pub filesystem_type: Vec<u8>,
    pub source: Vec<u8>,
    pub options: Vec<u8>,
    pub filesystem: Filesystem,
}
/// Discovery includes virtual filesystems and bind mounts. Capacity queries are separately opt-in.
pub struct Mounts {
    file: CachedFile,
    current: Vec<Mount>,
    started: Instant,
    sample: Sample,
}
impl Default for Mounts {
    fn default() -> Self {
        Self::new()
    }
}
impl Mounts {
    pub fn new() -> Self {
        Self::with_proc_root("/proc")
    }
    pub fn with_proc_root(root: impl AsRef<Path>) -> Self {
        Self {
            file: CachedFile::new(root.as_ref().join("self/mountinfo")),
            current: Vec::new(),
            started: Instant::now(),
            sample: Sample::default(),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &Mount> {
        self.current.iter()
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Mount> {
        self.current.iter_mut()
    }
    pub fn sample(&self) -> Sample {
        self.sample
    }
    pub fn refresh(&mut self) -> Result<(), LinuxError> {
        let bytes = self.file.read()?;
        let mut next = Vec::with_capacity(self.current.len());
        for line in bytes.split(|b| *b == b'\n').filter(|s| !s.trim_ascii().is_empty()) {
            let raw = parse_mountinfo_line(line)?;
            let path = decode(raw.mount_point)?;
            let filesystem = Filesystem::new(Path::new(OsStr::from_bytes(&path)))?;
            next.push(Mount {
                id: raw.id,
                parent_id: raw.parent_id,
                major: raw.major,
                minor: raw.minor,
                root: decode(raw.root)?,
                filesystem_type: decode(raw.filesystem)?,
                source: decode(raw.source)?,
                options: raw.options.to_vec(),
                filesystem,
            });
        }
        next.sort_unstable_by_key(|mount| mount.id);
        if next.windows(2).any(|m| m[0].id == m[1].id) {
            return Err(ParseError::InvalidRecord.into());
        }
        // Preserve successful capacity samples only for unchanged mount identity and path.
        for mount in &mut next {
            if let Ok(index) = self.current.binary_search_by_key(&mount.id, |m| m.id) {
                let old = &mut self.current[index];
                if old.major == mount.major
                    && old.minor == mount.minor
                    && old.filesystem.path == mount.filesystem.path
                    && old.root == mount.root
                    && old.filesystem_type == mount.filesystem_type
                    && old.source == mount.source
                {
                    std::mem::swap(&mut old.filesystem, &mut mount.filesystem);
                }
            }
        }
        self.current = next;
        self.sample.sequence = self.sample.sequence.wrapping_add(1);
        self.sample.timestamp_ns = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        Ok(())
    }
    /// Returns (successful, failed). Remote mounts may block: callers choose which mounts to sample.
    pub fn refresh_capacities(&mut self) -> (usize, usize) {
        let mut successful = 0;
        for mount in &mut self.current {
            successful += usize::from(mount.filesystem.refresh().is_ok());
        }
        (successful, self.current.len() - successful)
    }
}
fn decode(input: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut out = vec![0; input.len()];
    let len = unescape_mount_field(input, &mut out)?;
    out.truncate(len);
    Ok(out)
}

#[cfg(target_pointer_width = "64")]
#[repr(C)]
#[derive(Default)]
struct Statvfs {
    block_size: u64,
    fragment_size: u64,
    blocks: u64,
    free_blocks: u64,
    available_blocks: u64,
    files: u64,
    free_files: u64,
    available_files: u64,
    filesystem_id: u64,
    flags: u64,
    name_max: u64,
    reserved: [u32; 6],
}
#[cfg(target_pointer_width = "64")]
fn native_capacity(path: &CStr) -> io::Result<Capacity> {
    use std::ffi::{c_char, c_int};
    unsafe extern "C" {
        fn statvfs(path: *const c_char, result: *mut Statvfs) -> c_int;
    }
    let mut data = Statvfs::default();
    // SAFETY: Linux LP64 glibc/musl ABI is checked in CI against system headers.
    // `path` is NUL-terminated, and data is writable with statvfs's size and alignment.
    if unsafe { statvfs(path.as_ptr(), &mut data) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let bytes =
        |blocks: u64| blocks.checked_mul(data.fragment_size).ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData));
    Ok(Capacity {
        total_bytes: bytes(data.blocks)?,
        free_bytes: bytes(data.free_blocks)?,
        available_bytes: bytes(data.available_blocks)?,
        total_inodes: data.files,
        free_inodes: data.free_files,
        available_inodes: data.available_files,
        filesystem_id: data.filesystem_id,
        read_only: data.flags & 1 != 0,
    })
}
#[cfg(not(target_pointer_width = "64"))]
fn native_capacity(_path: &CStr) -> io::Result<Capacity> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "filesystem capacity currently requires Linux LP64"))
}

#[cfg(all(test, target_pointer_width = "64"))]
mod tests {
    use super::*;
    #[test]
    fn linux_lp64_statvfs_layout() {
        assert_eq!(std::mem::size_of::<Statvfs>(), 112);
        assert_eq!(std::mem::align_of::<Statvfs>(), 8);
        assert_eq!(std::mem::offset_of!(Statvfs, available_blocks), 32);
        assert_eq!(std::mem::offset_of!(Statvfs, flags), 72);
        assert_eq!(std::mem::offset_of!(Statvfs, reserved), 88);
    }
}
