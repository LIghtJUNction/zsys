//! Linux I/O primitives. All owned descriptors are closed by `File`.
use std::ffi::{CStr, c_char, c_int, c_long};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

unsafe extern "C" {
    fn openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    fn sysconf(name: c_int) -> c_long;
    #[cfg(all(target_pointer_width = "64", any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn syscall(number: c_long, ...) -> c_long;
}

/// Reuses initialized storage. Reads until EOF, including after short reads.
pub(crate) struct Buffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Buffer {
    pub(crate) fn new(initial: usize, limit: usize) -> Self {
        Self { bytes: vec![0; initial.min(limit + 1)], limit }
    }
    pub(crate) fn read<'a>(&'a mut self, file: &File) -> io::Result<&'a [u8]> {
        let mut used = 0;
        loop {
            if used == self.bytes.len() {
                self.bytes.resize((used * 2).max(1).min(self.limit + 1), 0);
            }
            let n = match file.read_at(&mut self.bytes[used..], used as u64) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                result => result?,
            };
            if n == 0 {
                return Ok(&self.bytes[..used]);
            }
            used += n;
            if used > self.limit {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "procfs record exceeds read limit"));
            }
        }
    }
}

pub(crate) struct CachedFile {
    path: PathBuf,
    file: Option<File>,
    buffer: Buffer,
}
impl CachedFile {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, file: None, buffer: Buffer::new(8192, 16 * 1024 * 1024) }
    }
    pub(crate) fn read(&mut self) -> io::Result<&[u8]> {
        if self.file.is_none() {
            self.file = Some(File::open(&self.path)?);
        }
        self.buffer.read(self.file.as_ref().expect("opened above"))
    }
}

pub(crate) fn open_at(directory: &File, name: &CStr) -> io::Result<File> {
    loop {
        // SAFETY: name is NUL-terminated; no mode argument is needed without O_CREAT.
        let fd = unsafe { openat(directory.as_raw_fd(), name.as_ptr(), 0x80000) }; // O_RDONLY | O_CLOEXEC
        if fd >= 0 {
            // SAFETY: openat returned a new owned descriptor, transferred exactly once.
            return Ok(unsafe { File::from_raw_fd(fd) });
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

pub(crate) fn clock_parameters() -> io::Result<(u64, u64)> {
    // Linux glibc/musl _SC_CLK_TCK and _SC_PAGESIZE. No libc Rust dependency.
    let ticks = unsafe { sysconf(2) };
    let pages = unsafe { sysconf(30) };
    if ticks <= 0 || pages <= 0 {
        return Err(io::Error::new(io::ErrorKind::Unsupported, "invalid clock/page configuration"));
    }
    Ok((ticks as u64, pages as u64))
}

/// Native enumeration is available on Linux x86_64 and aarch64. Std is a benchmarkable fallback.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScanMode {
    #[default]
    Native,
    Std,
}

pub(crate) struct Directory {
    pub(crate) file: File,
    path: PathBuf,
    entries: Vec<u8>,
}
impl Directory {
    pub(crate) fn new(path: &Path) -> io::Result<Self> {
        Ok(Self { file: File::open(path)?, path: path.to_owned(), entries: vec![0; 32 * 1024] })
    }
    pub(crate) fn pids(&mut self, out: &mut Vec<u32>, mode: ScanMode) -> io::Result<()> {
        out.clear();
        if mode == ScanMode::Std {
            for entry in std::fs::read_dir(&self.path)? {
                let entry = entry?;
                if let Some(pid) = numeric_pid(entry.file_name().as_bytes()) {
                    out.push(pid);
                }
            }
        } else {
            self.native_pids(out)?;
        }
        out.sort_unstable();
        out.dedup();
        Ok(())
    }
    #[cfg(all(target_pointer_width = "64", any(target_arch = "x86_64", target_arch = "aarch64")))]
    fn native_pids(&mut self, out: &mut Vec<u32>) -> io::Result<()> {
        use std::io::{Seek, SeekFrom};
        self.file.seek(SeekFrom::Start(0))?;
        #[cfg(target_arch = "x86_64")]
        const GETDENTS64: c_long = 217;
        #[cfg(target_arch = "aarch64")]
        const GETDENTS64: c_long = 61;
        loop {
            // SAFETY: live directory fd and writable initialized buffer of the stated length.
            let n =
                unsafe { syscall(GETDENTS64, self.file.as_raw_fd(), self.entries.as_mut_ptr(), self.entries.len()) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e);
            }
            if n == 0 {
                return Ok(());
            }
            parse_dirents(&self.entries[..n as usize], out)?;
        }
    }
    #[cfg(not(all(target_pointer_width = "64", any(target_arch = "x86_64", target_arch = "aarch64"))))]
    fn native_pids(&mut self, _out: &mut Vec<u32>) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "select ScanMode::Std on this architecture"))
    }
}

fn numeric_pid(name: &[u8]) -> Option<u32> {
    if name.is_empty() {
        return None;
    }
    let mut pid = 0u32;
    for &b in name {
        if !b.is_ascii_digit() {
            return None;
        }
        pid = pid.checked_mul(10)?.checked_add(u32::from(b - b'0'))?;
    }
    (pid != 0).then_some(pid)
}

fn parse_dirents(mut bytes: &[u8], out: &mut Vec<u32>) -> io::Result<()> {
    while !bytes.is_empty() {
        if bytes.len() < 20 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let len = usize::from(u16::from_ne_bytes([bytes[16], bytes[17]]));
        if len < 20 || len > bytes.len() {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let name = &bytes[19..len];
        let end = name.iter().position(|b| *b == 0).ok_or(io::ErrorKind::InvalidData)?;
        if let Some(pid) = numeric_pid(&name[..end]) {
            out.push(pid);
        }
        bytes = &bytes[len..];
    }
    Ok(())
}

/// PID path in stack storage, including its trailing NUL.
pub(crate) fn pid_path<'a>(pid: u32, suffix: &[u8], storage: &'a mut [u8; 64]) -> &'a CStr {
    let mut digits = [0u8; 10];
    let mut pos = digits.len();
    let mut value = pid;
    loop {
        pos -= 1;
        digits[pos] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    let count = digits.len() - pos;
    storage[..count].copy_from_slice(&digits[pos..]);
    storage[count..count + suffix.len()].copy_from_slice(suffix);
    storage[count + suffix.len()] = 0;
    CStr::from_bytes_with_nul(&storage[..count + suffix.len() + 1]).expect("numeric PID and static suffix")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pid_paths_are_bounded() {
        let mut buffer = [0; 64];
        assert_eq!(pid_path(u32::MAX, b"/stat", &mut buffer).to_bytes(), b"4294967295/stat");
    }
    #[test]
    fn corrupt_directory_entries_are_rejected() {
        assert!(parse_dirents(&[0; 19], &mut Vec::new()).is_err());
        assert!(parse_dirents(&[0; 24], &mut Vec::new()).is_err());
        let mut bytes = [0u8; 24];
        bytes[16..18].copy_from_slice(&24u16.to_ne_bytes());
        bytes[19..22].copy_from_slice(b"123");
        let mut out = Vec::new();
        parse_dirents(&bytes, &mut out).unwrap();
        assert_eq!(out, [123]);
    }
}
