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
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
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
    reset_pending: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceAddress {
    pub address: IpAddr,
    pub netmask: Option<IpAddr>,
    pub scope_id: u32,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationalState {
    Unknown,
    NotPresent,
    Down,
    LowerLayerDown,
    Testing,
    Dormant,
    Up,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceDetails {
    pub name: Vec<u8>,
    pub index: u32,
    pub flags: u32,
    pub addresses: Vec<InterfaceAddress>,
    /// Link-layer address from AF_PACKET; not limited to six-byte Ethernet addresses.
    pub hardware_address: Vec<u8>,
    pub mtu: Option<u64>,
    /// Negotiated link speed; unavailable or negative sysfs readings remain None.
    pub link_speed_bps: Option<u64>,
    pub operational_state: Option<OperationalState>,
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

#[derive(Clone, Copy)]
struct StagedNetwork {
    start: usize,
    len: usize,
    counters: NetworkCounters,
}
pub struct Networks {
    file: CachedFile,
    sys_root: PathBuf,
    staged: Vec<StagedNetwork>,
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
        Self::with_roots(root, "/sys")
    }
    pub fn with_roots(proc_root: impl AsRef<Path>, sys_root: impl AsRef<Path>) -> Self {
        Self {
            file: CachedFile::new(proc_root.as_ref().join("net/dev")),
            sys_root: sys_root.as_ref().to_owned(),
            staged: Vec::new(),
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
        self.staged.clear();
        for line in bytes.split(|b| *b == b'\n').skip(2).filter(|s| !s.trim_ascii().is_empty()) {
            let raw = parse_net_dev_line(line)?;
            self.staged.push(StagedNetwork {
                start: raw.name.as_ptr() as usize - bytes.as_ptr() as usize,
                len: raw.name.len(),
                counters: raw.counters,
            });
        }
        let name = |entry: StagedNetwork| &bytes[entry.start..entry.start + entry.len];
        self.staged.sort_unstable_by(|a, b| name(*a).cmp(name(*b)));
        if self.staged.windows(2).any(|pair| name(pair[0]) == name(pair[1])) {
            return Err(ParseError::InvalidRecord.into());
        }
        let now = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let sequence = self.sample.sequence.wrapping_add(1);
        for raw in &self.staged {
            let name = name(*raw);
            let sample = Sample { timestamp_ns: now, sequence };
            if let Some(old) = self.table.get_mut(name) {
                old.delta = (!old.reset_pending).then(|| raw.counters.delta_since(old.counters)).flatten();
                old.interval_ns = old.delta.and_then(|_| now.checked_sub(old.sample.timestamp_ns).filter(|n| *n != 0));
                old.reset_pending = false;
                old.counters = raw.counters;
                old.sample = sample;
                old.seen = sequence;
            } else {
                self.table.insert(
                    name.to_vec(),
                    Network {
                        name: name.to_vec(),
                        counters: raw.counters,
                        delta: None,
                        interval_ns: None,
                        sample,
                        details: None,
                        seen: sequence,
                        reset_pending: false,
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
        let mut details = interface_details(now, &self.sys_root)?;
        for network in self.table.values_mut() {
            let value = details.iter().position(|d| d.name == network.name).map(|index| details.swap_remove(index));
            apply_details(network, value);
        }
        Ok(())
    }
    pub fn refresh(&mut self) -> Result<(), LinuxError> {
        self.refresh_counters()?;
        self.refresh_details()?;
        Ok(())
    }
}

fn apply_details(network: &mut Network, value: Option<InterfaceDetails>) {
    if let Some(ref new) = value {
        if network.details.as_ref().is_some_and(|old| old.index != new.index) {
            network.reset_pending = true;
            network.delta = None;
            network.interval_ns = None;
        }
    } else if network.details.is_some() {
        network.reset_pending = true;
        network.delta = None;
        network.interval_ns = None;
    }
    network.details = value;
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
#[repr(C)]
#[derive(Clone, Copy)]
struct LinkAddress {
    family: u16,
    protocol: u16,
    index: c_int,
    hardware_type: u16,
    packet_type: u8,
    length: u8,
    address: [u8; 8],
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
fn interface_details(sampled_at_ns: u64, sys_root: &Path) -> io::Result<Vec<InterfaceDetails>> {
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
                hardware_address: Vec::new(),
                mtu: None,
                link_speed_bps: None,
                operational_state: None,
                sampled_at_ns,
            });
            result.len() - 1
        });
        let record = &mut result[position];
        record.flags |= entry.flags;
        // SAFETY: non-null getifaddrs addresses start with sa_family; AF_PACKET
        // supplies sockaddr_ll. Copy only its fixed-size address storage.
        if !entry.address.is_null() && unsafe { entry.address.cast::<u16>().read_unaligned() } == 17 {
            let link = unsafe { entry.address.cast::<LinkAddress>().read_unaligned() };
            if usize::from(link.length) <= link.address.len() {
                record.hardware_address.clear();
                record.hardware_address.extend_from_slice(&link.address[..usize::from(link.length)]);
            }
        }
        if let Some((ip, scope_id)) = unsafe { decode_address(entry.address) } {
            let netmask = unsafe { decode_address(entry.netmask) }.map(|(ip, _)| ip);
            let address = InterfaceAddress { address: ip, netmask, scope_id };
            if !record.addresses.contains(&address) {
                record.addresses.push(address);
            }
        }
    }
    for interface in &mut result {
        read_link_properties(interface, sys_root);
    }
    Ok(result)
}
fn read_link_properties(interface: &mut InterfaceDetails, sys_root: &Path) {
    let path = sys_root.join("class/net").join(std::ffi::OsStr::from_bytes(&interface.name));
    let number =
        |name| std::fs::read(path.join(name)).ok().and_then(|bytes| crate::parse::number(bytes.trim_ascii()).ok());
    interface.mtu = number("mtu");
    interface.link_speed_bps = number("speed").filter(|value| *value != 0).and_then(|mbps| mbps.checked_mul(1_000_000));
    interface.operational_state = std::fs::read(path.join("operstate")).ok().and_then(|value| {
        Some(match value.trim_ascii() {
            b"unknown" => OperationalState::Unknown,
            b"notpresent" => OperationalState::NotPresent,
            b"down" => OperationalState::Down,
            b"lowerlayerdown" => OperationalState::LowerLayerDown,
            b"testing" => OperationalState::Testing,
            b"dormant" => OperationalState::Dormant,
            b"up" => OperationalState::Up,
            _ => return None,
        })
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn native_socket_layout_matches_the_checked_c_contract() {
        assert_eq!(std::mem::size_of::<IfAddrs>(), 56);
        assert_eq!(std::mem::offset_of!(IfAddrs, flags), 16);
        assert_eq!(std::mem::offset_of!(IfAddrs, address), 24);
        assert_eq!(std::mem::size_of::<Address4>(), 16);
        assert_eq!(std::mem::size_of::<Address6>(), 28);
        assert_eq!(std::mem::offset_of!(Address6, scope), 24);
        assert_eq!(std::mem::size_of::<LinkAddress>(), 20);
        assert_eq!(std::mem::offset_of!(LinkAddress, address), 12);
    }
    fn details(index: u32) -> InterfaceDetails {
        InterfaceDetails {
            name: b"test0".to_vec(),
            index,
            flags: 1,
            addresses: Vec::new(),
            hardware_address: Vec::new(),
            mtu: None,
            link_speed_bps: None,
            operational_state: None,
            sampled_at_ns: 0,
        }
    }
    #[test]
    fn interface_replacement_invalidates_the_next_counter_baseline() {
        let mut network = Network {
            name: b"test0".to_vec(),
            counters: NetworkCounters::default(),
            delta: Some(NetworkCounters::default()),
            interval_ns: Some(100),
            sample: Sample::default(),
            details: Some(details(1)),
            seen: 0,
            reset_pending: false,
        };
        apply_details(&mut network, Some(details(2)));
        assert!(network.reset_pending);
        assert!(network.delta.is_none());
        assert!(network.interval_ns.is_none());
        apply_details(&mut network, Some(details(2)));
        assert!(network.reset_pending, "metadata refresh cannot revalidate old counters");
    }
}
