//! Traffic counters are separate from address/flag discovery; both are explicitly refreshed.
use super::LinuxError;
use super::io::CachedFile;
use crate::{
    Sample,
    network::{NetworkCounters, parse_net_dev_line},
    parse::ParseError,
};
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Network {
    pub name: Vec<u8>,
    pub counters: NetworkCounters,
    pub delta: Option<NetworkCounters>,
    pub interval_ns: Option<u64>,
    pub sample: Sample,
    /// Details have their own update time and are never silently treated as fresh counters.
    pub details: Option<InterfaceDetails>,
    seen: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceAddress {
    pub address: IpAddr,
    pub netmask: Option<IpAddr>,
    pub scope_id: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceDetails {
    pub name: Vec<u8>,
    pub index: u32,
    pub flags: u32,
    pub addresses: Vec<InterfaceAddress>,
    pub sampled_at_ns: u64,
}
impl InterfaceDetails {
    pub fn is_up(&self) -> bool {
        self.flags & 1 != 0
    }
    pub fn is_running(&self) -> bool {
        self.flags & 0x40 != 0
    }
    pub fn is_loopback(&self) -> bool {
        self.flags & 8 != 0
    }
}

pub struct Networks {
    file: CachedFile,
    table: HashMap<Vec<u8>, Network>,
    started: Instant,
    sample: Sample,
}
impl Default for Networks {
    fn default() -> Self {
        Self::new()
    }
}
impl Networks {
    pub fn new() -> Self {
        Self::with_proc_root("/proc")
    }
    pub fn with_proc_root(root: impl AsRef<Path>) -> Self {
        Self {
            file: CachedFile::new(root.as_ref().join("net/dev")),
            table: HashMap::new(),
            started: Instant::now(),
            sample: Sample::default(),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &Network> {
        self.table.values()
    }
    pub fn get(&self, name: &[u8]) -> Option<&Network> {
        self.table.get(name)
    }
    pub fn sample(&self) -> Sample {
        self.sample
    }
    /// Every call reads live counters. Steady-state storage is reused.
    /// Deltas are invalidated on rollback and when an explicit details refresh detects a changed ifindex.
    /// Name reuse entirely between details refreshes cannot be distinguished from continuous traffic.
    pub fn refresh_counters(&mut self) -> Result<(), LinuxError> {
        let bytes = self.file.read()?;
        let mut headers = bytes.split(|b| *b == b'\n');
        if !headers.next().is_some_and(|s| s.starts_with(b"Inter-"))
            || !headers.next().is_some_and(|s| s.windows(4).any(|w| w == b"face"))
        {
            return Err(ParseError::InvalidRecord.into());
        }
        for line in bytes.split(|b| *b == b'\n').skip(2).filter(|s| !s.trim_ascii().is_empty()) {
            parse_net_dev_line(line)?;
        }
        let now = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let sequence = self.sample.sequence.wrapping_add(1);
        for line in bytes.split(|b| *b == b'\n').skip(2).filter(|s| !s.trim_ascii().is_empty()) {
            let raw = parse_net_dev_line(line)?;
            let sample = Sample { timestamp_ns: now, sequence };
            if let Some(old) = self.table.get_mut(raw.name) {
                old.delta = raw.counters.delta_since(old.counters);
                old.interval_ns = now.checked_sub(old.sample.timestamp_ns).filter(|n| *n != 0);
                old.counters = raw.counters;
                old.sample = sample;
                old.seen = sequence;
            } else {
                self.table.insert(
                    raw.name.to_vec(),
                    Network {
                        name: raw.name.to_vec(),
                        counters: raw.counters,
                        delta: None,
                        interval_ns: None,
                        sample,
                        details: None,
                        seen: sequence,
                    },
                );
            }
        }
        self.table.retain(|_, network| network.seen == sequence);
        self.sample = Sample { timestamp_ns: now, sequence };
        Ok(())
    }
    /// Uses the current network namespace, independently of a custom procfs fixture root.
    pub fn refresh_details(&mut self) -> io::Result<()> {
        let now = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let details = interface_details(now)?;
        for network in self.table.values_mut() {
            if let Some(value) = details.iter().find(|d| d.name == network.name) {
                if network.details.as_ref().is_none_or(|old| old.index != value.index) {
                    network.delta = None;
                    network.interval_ns = None;
                }
                network.details = Some(value.clone());
            } else {
                network.details = None;
            }
        }
        Ok(())
    }
    pub fn refresh(&mut self) -> Result<(), LinuxError> {
        self.refresh_counters()?;
        self.refresh_details()?;
        Ok(())
    }
}

#[repr(C)]
struct IfAddrs {
    next: *mut IfAddrs,
    name: *mut c_char,
    flags: c_uint,
    address: *mut c_void,
    netmask: *mut c_void,
    broadcast: *mut c_void,
    data: *mut c_void,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Address4 {
    family: u16,
    port: u16,
    address: [u8; 4],
    zero: [u8; 8],
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Address6 {
    family: u16,
    port: u16,
    flow: u32,
    address: [u8; 16],
    scope: u32,
}
unsafe extern "C" {
    fn getifaddrs(result: *mut *mut IfAddrs) -> c_int;
    fn freeifaddrs(list: *mut IfAddrs);
    fn if_nametoindex(name: *const c_char) -> c_uint;
}
struct AddressList(*mut IfAddrs);
impl Drop for AddressList {
    fn drop(&mut self) {
        // SAFETY: this is the original list returned by getifaddrs; it is freed exactly once.
        unsafe { freeifaddrs(self.0) };
    }
}
fn interface_details(sampled_at_ns: u64) -> io::Result<Vec<InterfaceDetails>> {
    let mut head = std::ptr::null_mut();
    // SAFETY: head points to writable pointer storage.
    if unsafe { getifaddrs(&mut head) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let list = AddressList(head);
    let mut cursor = list.0;
    let mut result: Vec<InterfaceDetails> = Vec::new();
    while !cursor.is_null() {
        // SAFETY: every node and its strings/addresses remain valid while list is owned.
        let entry = unsafe { &*cursor };
        cursor = entry.next;
        if entry.name.is_null() {
            continue;
        }
        let name = unsafe { CStr::from_ptr(entry.name) }.to_bytes();
        let position = result.iter().position(|d| d.name == name).unwrap_or_else(|| {
            result.push(InterfaceDetails {
                name: name.to_vec(),
                index: unsafe { if_nametoindex(entry.name) },
                flags: entry.flags,
                addresses: Vec::new(),
                sampled_at_ns,
            });
            result.len() - 1
        });
        let record = &mut result[position];
        record.flags |= entry.flags;
        if let Some((ip, scope_id)) = unsafe { decode_address(entry.address) } {
            let netmask = unsafe { decode_address(entry.netmask) }.map(|(ip, _)| ip);
            let address = InterfaceAddress { address: ip, netmask, scope_id };
            if !record.addresses.contains(&address) {
                record.addresses.push(address);
            }
        }
    }
    Ok(result)
}
/// The pointer must be null or an OS-owned sockaddr matching its family.
unsafe fn decode_address(pointer: *const c_void) -> Option<(IpAddr, u32)> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: every sockaddr starts with the Linux u16 sa_family field.
    match unsafe { pointer.cast::<u16>().read_unaligned() } {
        2 => {
            let a = unsafe { pointer.cast::<Address4>().read_unaligned() };
            Some((IpAddr::V4(Ipv4Addr::from(a.address)), 0))
        }
        10 => {
            let a = unsafe { pointer.cast::<Address6>().read_unaligned() };
            Some((IpAddr::V6(Ipv6Addr::from(a.address)), a.scope))
        }
        _ => None,
    }
}
