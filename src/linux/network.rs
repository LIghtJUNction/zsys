//! Traffic counters are separate from address/flag discovery; both are explicitly refreshed.
use super::LinuxError;
use super::io::CachedFile;
use crate::{
    Sample,
    network::{NetworkCounters, parse_net_dev_line},
    parse::ParseError,
};
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_uint, c_ulong, c_void};
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
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
/// Native source used to enumerate addresses. Both include IPv4 and IPv6.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressSource {
    Getifaddrs,
    /// IPv4 SIOCGIFCONF plus /proc/net/if_inet6; flag bits are limited to SIOCGIFFLAGS.
    IoctlProcfs,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InterfaceDetails {
    pub name: Vec<u8>,
    pub index: u32,
    pub flags: u32,
    pub addresses: Vec<InterfaceAddress>,
    /// Ethernet-style address, or None for an unsupported hardware type/query failure.
    pub mac_address: Option<[u8; 6]>,
    pub mtu: Option<u64>,
    /// Negotiated link speed in bits/second, when exported by the driver via sysfs.
    pub link_speed_bits_per_second: Option<u64>,
    pub address_source: AddressSource,
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
    name_start: usize,
    name_len: usize,
    counters: NetworkCounters,
}
pub struct Networks {
    file: CachedFile,
    table: HashMap<Vec<u8>, Network>,
    staged: Vec<StagedNetwork>,
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
            staged: Vec::new(),
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
                name_start: raw.name.as_ptr() as usize - bytes.as_ptr() as usize,
                name_len: raw.name.len(),
                counters: raw.counters,
            });
        }
        let name = |raw: &StagedNetwork| &bytes[raw.name_start..raw.name_start + raw.name_len];
        self.staged.sort_unstable_by(|a, b| name(a).cmp(name(b)));
        if self.staged.windows(2).any(|pair| name(&pair[0]) == name(&pair[1])) {
            return Err(ParseError::InvalidRecord.into());
        }
        let now = self.started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        let sequence = self.sample.sequence.wrapping_add(1);
        for raw in &self.staged {
            let name = name(raw);
            let sample = Sample { timestamp_ns: now, sequence };
            if let Some(old) = self.table.get_mut(name) {
                old.delta = raw.counters.delta_since(old.counters);
                old.interval_ns = now.checked_sub(old.sample.timestamp_ns).filter(|n| *n != 0);
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
    let mut result = match getifaddrs_details(sampled_at_ns) {
        Ok(details) => details,
        Err(error) if matches!(error.raw_os_error(), Some(1 | 13 | 38 | 93 | 97)) => {
            // Read-only ioctl queries remain available in containers that restrict netlink.
            ioctl_details(sampled_at_ns)?
        }
        Err(error) => return Err(error),
    };
    let socket = datagram_socket()?;
    for record in &mut result {
        record.mtu = query(&socket, &record.name, 0x8921).ok().and_then(|r| {
            let mtu = i32::from_ne_bytes(unsafe { r.data.bytes[..4].try_into().unwrap() });
            u64::try_from(mtu).ok()
        });
        record.mac_address = query(&socket, &record.name, 0x8927).ok().and_then(|r| {
            let bytes = unsafe { r.data.bytes };
            // ARPHRD_ETHER or ARPHRD_LOOPBACK. Other formats do not necessarily have six octets.
            matches!(u16::from_ne_bytes([bytes[0], bytes[1]]), 1 | 772).then(|| bytes[2..8].try_into().unwrap())
        });
        let path = Path::new("/sys/class/net").join(std::ffi::OsStr::from_bytes(&record.name)).join("speed");
        record.link_speed_bits_per_second = std::fs::read(path)
            .ok()
            .and_then(|v| crate::parse::number(v.trim_ascii()).ok())
            .filter(|v| *v != 0 && *v != u64::from(u32::MAX))
            .and_then(|mbps| mbps.checked_mul(1_000_000));
    }
    Ok(result)
}
fn getifaddrs_details(sampled_at_ns: u64) -> io::Result<Vec<InterfaceDetails>> {
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
                mac_address: None,
                mtu: None,
                link_speed_bits_per_second: None,
                address_source: AddressSource::Getifaddrs,
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

// Linux ifreq's union contains sockaddr (16 bytes) and ifmap (3 machine words).
// Union storage preserves both the LP64 and ILP32 size/alignment; CI checks C headers.
const REQUEST_DATA_BYTES: usize = if cfg!(target_pointer_width = "64") { 24 } else { 16 };
#[repr(C)]
#[derive(Clone, Copy)]
union RequestData {
    bytes: [u8; REQUEST_DATA_BYTES],
    alignment: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct IfRequest {
    name: [u8; 16],
    data: RequestData,
}
impl IfRequest {
    fn new(name: &[u8]) -> io::Result<Self> {
        if name.len() >= 16 || name.contains(&0) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let mut result = Self { name: [0; 16], data: RequestData { bytes: [0; REQUEST_DATA_BYTES] } };
        result.name[..name.len()].copy_from_slice(name);
        Ok(result)
    }
}
#[repr(C)]
struct IfConfig {
    length: c_int,
    requests: *mut IfRequest,
}
unsafe extern "C" {
    fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int;
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}
fn datagram_socket() -> io::Result<OwnedFd> {
    // SAFETY: AF_INET, SOCK_DGRAM | SOCK_CLOEXEC; no packets are sent and no address is bound.
    let descriptor = unsafe { socket(2, 2 | 0x80000, 0) };
    if descriptor < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the new owned descriptor is transferred once and closed by OwnedFd.
    Ok(unsafe { OwnedFd::from_raw_fd(descriptor) })
}
fn query(socket: &OwnedFd, name: &[u8], operation: c_ulong) -> io::Result<IfRequest> {
    let mut request = IfRequest::new(name)?;
    // SAFETY: the caller supplies a fixed read-only ifreq operation; request has the native ABI.
    if unsafe { ioctl(socket.as_raw_fd(), operation, &mut request) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(request)
}
fn ioctl_details(sampled_at_ns: u64) -> io::Result<Vec<InterfaceDetails>> {
    let socket = datagram_socket()?;
    let bytes = std::fs::read("/proc/net/dev")?;
    let mut result = Vec::new();
    for line in bytes.split(|b| *b == b'\n').skip(2).filter(|s| !s.trim_ascii().is_empty()) {
        let raw = parse_net_dev_line(line).map_err(|_| io::ErrorKind::InvalidData)?;
        let index = query(&socket, raw.name, 0x8933)?;
        let index = u32::from_ne_bytes(unsafe { index.data.bytes[..4].try_into().unwrap() });
        let flags = query(&socket, raw.name, 0x8913)?;
        let flags = u16::from_ne_bytes(unsafe { flags.data.bytes[..2].try_into().unwrap() });
        result.push(InterfaceDetails {
            name: raw.name.to_vec(),
            index,
            flags: u32::from(flags),
            addresses: Vec::new(),
            mac_address: None,
            mtu: None,
            link_speed_bits_per_second: None,
            address_source: AddressSource::IoctlProcfs,
            sampled_at_ns,
        });
    }
    let mut requests = vec![IfRequest::new(b"")?; 32];
    let entry_size = std::mem::size_of::<IfRequest>();
    loop {
        let buffer_length = requests.len() * entry_size;
        let mut config = IfConfig {
            length: c_int::try_from(buffer_length).map_err(|_| io::ErrorKind::InvalidData)?,
            requests: requests.as_mut_ptr(),
        };
        // SAFETY: config points to the initialized, correctly aligned array of the stated byte length.
        if unsafe { ioctl(socket.as_raw_fd(), 0x8912 as c_ulong, &mut config) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let returned = usize::try_from(config.length).map_err(|_| io::ErrorKind::InvalidData)?;
        if returned > buffer_length || returned % entry_size != 0 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        // SIOCGIFCONF silently truncates; at least one unused record establishes a complete read.
        if buffer_length - returned < entry_size {
            if requests.len() >= 65_536 {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "interface list exceeds limit"));
            }
            requests.resize(requests.len() * 2, IfRequest::new(b"")?);
            continue;
        }
        for request in &requests[..returned / entry_size] {
            let name_len = request.name.iter().position(|b| *b == 0).ok_or(io::ErrorKind::InvalidData)?;
            let name = &request.name[..name_len];
            let index = query(&socket, name, 0x8933)?;
            let index = u32::from_ne_bytes(unsafe { index.data.bytes[..4].try_into().unwrap() });
            let address = unsafe { decode_address(request.data.bytes.as_ptr().cast()) };
            if let (Some(record), Some((address, scope_id))) = (result.iter_mut().find(|r| r.index == index), address) {
                let netmask = query(&socket, name, 0x891b)
                    .ok()
                    .and_then(|r| unsafe { decode_address(r.data.bytes.as_ptr().cast()) }.map(|(ip, _)| ip));
                let value = InterfaceAddress { address, netmask, scope_id };
                if !record.addresses.contains(&value) {
                    record.addresses.push(value);
                }
            }
        }
        break;
    }
    match std::fs::read("/proc/net/if_inet6") {
        Ok(bytes) => append_ipv6(&bytes, &mut result)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {} // IPv6 disabled in this kernel/namespace.
        Err(error) => return Err(error),
    }
    Ok(result)
}
fn append_ipv6(bytes: &[u8], result: &mut [InterfaceDetails]) -> io::Result<()> {
    let hex = |value: &[u8]| -> io::Result<u32> {
        u32::from_str_radix(std::str::from_utf8(value).map_err(|_| io::ErrorKind::InvalidData)?, 16)
            .map_err(|_| io::ErrorKind::InvalidData.into())
    };
    for line in bytes.split(|b| *b == b'\n').filter(|s| !s.trim_ascii().is_empty()) {
        let fields: Vec<_> = crate::parse::fields(line).collect();
        if fields.len() != 6 || fields[0].len() != 32 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut octets = [0u8; 16];
        for (octet, pair) in octets.iter_mut().zip(fields[0].as_chunks::<2>().0) {
            *octet = hex(pair)? as u8;
        }
        let index = hex(fields[1])?;
        let prefix = hex(fields[2])?;
        if prefix > 128 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        hex(fields[3])?;
        hex(fields[4])?;
        let ip = Ipv6Addr::from(octets);
        let scope_id = if ip.is_unicast_link_local() || (octets[0] == 0xff && octets[1] & 15 <= 2) { index } else { 0 };
        let mask = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix) };
        let address =
            InterfaceAddress { address: IpAddr::V6(ip), netmask: Some(IpAddr::V6(Ipv6Addr::from(mask))), scope_id };
        if let Some(record) = result.iter_mut().find(|r| r.index == index && r.name == fields[5])
            && !record.addresses.contains(&address)
        {
            record.addresses.push(address);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_network_abi_layouts() {
        assert_eq!(std::mem::size_of::<Address4>(), 16);
        assert_eq!(std::mem::size_of::<Address6>(), 28);
        assert_eq!(std::mem::offset_of!(Address6, scope), 24);
        assert_eq!(std::mem::size_of::<IfRequest>(), if cfg!(target_pointer_width = "64") { 40 } else { 32 });
        assert_eq!(std::mem::align_of::<IfRequest>(), std::mem::align_of::<usize>());
    }
    #[test]
    fn ipv6_prefix_and_scope_are_preserved() {
        let mut entries = vec![InterfaceDetails {
            name: b"eth0".to_vec(),
            index: 2,
            flags: 1,
            addresses: Vec::new(),
            mac_address: None,
            mtu: None,
            link_speed_bits_per_second: None,
            address_source: AddressSource::IoctlProcfs,
            sampled_at_ns: 0,
        }];
        append_ipv6(b"fe800000000000000000000000000001 02 40 20 80 eth0\n", &mut entries).unwrap();
        assert_eq!(entries[0].addresses[0].scope_id, 2);
        assert_eq!(entries[0].addresses[0].netmask, Some("ffff:ffff:ffff:ffff::".parse().unwrap()));
        assert!(append_ipv6(b"::1 02 40 20 80 eth0\n", &mut entries).is_err());
        assert!(append_ipv6(b"fe800000000000000000000000000001 02 81 20 80 eth0\n", &mut entries).is_err());
    }
}
