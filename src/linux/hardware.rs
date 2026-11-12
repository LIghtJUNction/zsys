//! Optional read-only sysfs capabilities. No vendor library, calibration guesses or control writes.
use super::io::Buffer;
use crate::Sample;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Missing attributes are unsupported, not zero-valued observations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Attribute<T> {
    Value(T),
    Unavailable,
    Failed(io::ErrorKind),
}
impl<T> Attribute<T> {
    fn from_result(result: io::Result<T>) -> Self {
        match result {
            Ok(value) => Self::Value(value),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::Unavailable,
            Err(error) => Self::Failed(error.kind()),
        }
    }
}
fn read<'a>(path: &Path, buffer: &'a mut Buffer) -> io::Result<&'a [u8]> {
    buffer.read(&File::open(path)?)
}
fn signed(path: &Path, buffer: &mut Buffer) -> Attribute<i64> {
    Attribute::from_result(read(path, buffer).and_then(|bytes| {
        std::str::from_utf8(bytes.trim_ascii())
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| io::ErrorKind::InvalidData.into())
    }))
}
fn unsigned(path: &Path, buffer: &mut Buffer) -> Attribute<u64> {
    Attribute::from_result(
        read(path, buffer)
            .and_then(|bytes| crate::parse::number(bytes.trim_ascii()).map_err(|_| io::ErrorKind::InvalidData.into())),
    )
}
fn bytes(path: &Path, buffer: &mut Buffer) -> Attribute<Vec<u8>> {
    Attribute::from_result(read(path, buffer).map(|b| b.trim_ascii().to_vec()))
}
fn next_sample(previous: Sample, started: Instant) -> Sample {
    Sample {
        sequence: previous.sequence.wrapping_add(1),
        timestamp_ns: started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemperatureReading {
    pub millidegrees_celsius: Attribute<i64>,
    pub critical_millidegrees_celsius: Attribute<i64>,
    pub sample: Sample,
}
#[derive(Debug)]
pub struct TemperatureSensor {
    pub device: PathBuf,
    pub index: u32,
    pub chip_name: Attribute<Vec<u8>>,
    pub label: Attribute<Vec<u8>>,
    /// None before first refresh; failed/new readings do not masquerade as stale successes.
    pub reading: Option<TemperatureReading>,
    input: PathBuf,
    critical: PathBuf,
}
/// Explicit discovery and independent reading refresh. Device labels are kernel labels, not calibration data.
pub struct Temperatures {
    root: PathBuf,
    sensors: Vec<TemperatureSensor>,
    buffer: Buffer,
    started: Instant,
    sample: Sample,
}
impl Default for Temperatures {
    fn default() -> Self {
        Self::new()
    }
}
impl Temperatures {
    pub fn new() -> Self {
        Self::with_sys_root("/sys")
    }
    pub fn with_sys_root(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_owned(),
            sensors: Vec::new(),
            buffer: Buffer::new(256, 4096),
            started: Instant::now(),
            sample: Sample::default(),
        }
    }
    /// Rebuilds topology; a failed directory traversal leaves the old list intact.
    pub fn discover(&mut self) -> io::Result<()> {
        let mut next = Vec::new();
        for chip in fs::read_dir(self.root.join("class/hwmon"))? {
            let device = chip?.path();
            for entry in fs::read_dir(&device)? {
                let entry = entry?;
                let name = entry.file_name();
                let Some(index) = name
                    .to_str()
                    .and_then(|s| s.strip_prefix("temp"))
                    .and_then(|s| s.strip_suffix("_input"))
                    .and_then(|s| s.parse::<u32>().ok())
                else {
                    continue;
                };
                if index == 0 {
                    continue;
                }
                next.push(TemperatureSensor {
                    device: device.clone(),
                    index,
                    chip_name: bytes(&device.join("name"), &mut self.buffer),
                    label: bytes(&device.join(format!("temp{index}_label")), &mut self.buffer),
                    input: entry.path(),
                    critical: device.join(format!("temp{index}_crit")),
                    reading: None,
                });
            }
        }
        next.sort_unstable_by(|a, b| (&a.device, a.index).cmp(&(&b.device, b.index)));
        self.sensors = next;
        Ok(())
    }
    pub fn iter(&self) -> impl Iterator<Item = &TemperatureSensor> {
        self.sensors.iter()
    }
    /// Returns successful input readings and failed/unavailable inputs. Thresholds remain independently optional.
    pub fn refresh(&mut self) -> (usize, usize) {
        self.sample = next_sample(self.sample, self.started);
        let mut successful = 0;
        for sensor in &mut self.sensors {
            let input = signed(&sensor.input, &mut self.buffer);
            successful += usize::from(matches!(input, Attribute::Value(_)));
            sensor.reading = Some(TemperatureReading {
                millidegrees_celsius: input,
                critical_millidegrees_celsius: signed(&sensor.critical, &mut self.buffer),
                sample: self.sample,
            });
        }
        (successful, self.sensors.len() - successful)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GpuReading {
    /// amdgpu-style sysfs aggregate utilization when the driver exports it; not a universal GPU API.
    pub utilization_percent: Attribute<u64>,
    pub vram_total_bytes: Attribute<u64>,
    pub vram_used_bytes: Attribute<u64>,
    pub sample: Sample,
}
#[derive(Debug)]
pub struct GpuDevice {
    pub card: PathBuf,
    pub vendor_id: Attribute<u16>,
    pub device_id: Attribute<u16>,
    pub reading: Option<GpuReading>,
    busy: PathBuf,
    total: PathBuf,
    used: PathBuf,
}
pub struct Gpus {
    root: PathBuf,
    devices: Vec<GpuDevice>,
    buffer: Buffer,
    started: Instant,
    sample: Sample,
}
impl Default for Gpus {
    fn default() -> Self {
        Self::new()
    }
}
impl Gpus {
    pub fn new() -> Self {
        Self::with_sys_root("/sys")
    }
    pub fn with_sys_root(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_owned(),
            devices: Vec::new(),
            buffer: Buffer::new(256, 4096),
            started: Instant::now(),
            sample: Sample::default(),
        }
    }
    pub fn discover(&mut self) -> io::Result<()> {
        let mut next = Vec::new();
        for entry in fs::read_dir(self.root.join("class/drm"))? {
            let entry = entry?;
            let name = entry.file_name();
            if name
                .to_str()
                .and_then(|s| s.strip_prefix("card"))
                .filter(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
                .is_none()
            {
                continue;
            }
            let card = entry.path();
            let device = card.join("device");
            let mut id = |file: &str| {
                Attribute::from_result(read(&device.join(file), &mut self.buffer).and_then(|b| {
                    let value = std::str::from_utf8(b.trim_ascii()).map_err(|_| io::ErrorKind::InvalidData)?;
                    u16::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16)
                        .map_err(|_| io::ErrorKind::InvalidData.into())
                }))
            };
            next.push(GpuDevice {
                card,
                vendor_id: id("vendor"),
                device_id: id("device"),
                reading: None,
                busy: device.join("gpu_busy_percent"),
                total: device.join("mem_info_vram_total"),
                used: device.join("mem_info_vram_used"),
            });
        }
        next.sort_unstable_by(|a, b| a.card.cmp(&b.card));
        self.devices = next;
        Ok(())
    }
    pub fn iter(&self) -> impl Iterator<Item = &GpuDevice> {
        self.devices.iter()
    }
    pub fn refresh(&mut self) {
        self.sample = next_sample(self.sample, self.started);
        for gpu in &mut self.devices {
            let utilization = match unsigned(&gpu.busy, &mut self.buffer) {
                Attribute::Value(n) if n > 100 => Attribute::Failed(io::ErrorKind::InvalidData),
                value => value,
            };
            gpu.reading = Some(GpuReading {
                utilization_percent: utilization,
                vram_total_bytes: unsigned(&gpu.total, &mut self.buffer),
                vram_used_bytes: unsigned(&gpu.used, &mut self.buffer),
                sample: self.sample,
            });
        }
    }
}

/// Non-identifying DMI inventory. Serial numbers and machine UUIDs are deliberately not collected.
#[derive(Debug)]
pub struct HardwareInfo {
    pub system_vendor: Attribute<Vec<u8>>,
    pub product_name: Attribute<Vec<u8>>,
    pub bios_version: Attribute<Vec<u8>>,
}
impl HardwareInfo {
    pub fn read() -> Self {
        Self::with_sys_root("/sys")
    }
    pub fn with_sys_root(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().join("class/dmi/id");
        let mut buffer = Buffer::new(256, 4096);
        Self {
            system_vendor: bytes(&root.join("sys_vendor"), &mut buffer),
            product_name: bytes(&root.join("product_name"), &mut buffer),
            bios_version: bytes(&root.join("bios_version"), &mut buffer),
        }
    }
}
