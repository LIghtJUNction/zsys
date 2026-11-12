//! Optional sysfs hardware inventory. No device-control files are written.
//!
//! Drivers expose different capabilities. Missing attributes remain `Unavailable`,
//! and hardware refreshes are independent of CPU/memory/process sampling.
use crate::Sample;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// A requested field keeps its actual read error; absent support never becomes zero.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HardwareValue<T> {
    NotSampled,
    Unavailable(io::ErrorKind),
    Value(T),
}
impl<T> HardwareValue<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::Value(value) => Some(value),
            _ => None,
        }
    }
}
impl<T> From<io::Result<T>> for HardwareValue<T> {
    fn from(result: io::Result<T>) -> Self {
        match result {
            Ok(value) => Self::Value(value),
            Err(error) => Self::Unavailable(error.kind()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemperatureSource {
    /// Linux thermal class values are millidegrees Celsius.
    ThermalZone,
    /// Raw hwmon ABI. Usually millidegrees Celsius; some ADC drivers export millivolts.
    /// Interpret the value using the named driver's ABI before converting to Celsius.
    Hwmon,
}
#[derive(Clone, Debug)]
pub struct TemperatureSensor {
    pub path: PathBuf,
    pub source: TemperatureSource,
    pub chip: HardwareValue<Vec<u8>>,
    pub label: HardwareValue<Vec<u8>>,
    pub raw_input: HardwareValue<i64>,
    pub raw_critical: HardwareValue<i64>,
    pub fault: HardwareValue<bool>,
    pub sample: Sample,
    critical_path: Option<PathBuf>,
    fault_path: Option<PathBuf>,
}
impl TemperatureSensor {
    /// Thermal-class readings have a guaranteed unit. Hwmon conversion is driver-specific.
    pub fn celsius(&self) -> Option<f64> {
        if self.source != TemperatureSource::ThermalZone {
            return None;
        }
        self.raw_input.value().map(|v| *v as f64 / 1000.0)
    }
}

#[derive(Clone, Debug)]
pub struct Gpu {
    /// DRM primary card name, e.g. card0. Connector and render nodes are excluded.
    pub card: Vec<u8>,
    pub device_path: PathBuf,
    pub vendor_id: HardwareValue<u32>,
    pub device_id: HardwareValue<u32>,
    pub driver: HardwareValue<Vec<u8>>,
    pub product_name: HardwareValue<Vec<u8>>,
    /// Driver-exported byte counters; currently provided by AMDGPU sysfs when supported.
    pub vram_total_bytes: HardwareValue<u64>,
    pub vram_used_bytes: HardwareValue<u64>,
    pub busy_percent: HardwareValue<u32>,
    pub sample: Sample,
}
#[derive(Clone, Debug)]
pub struct HardwareInfo {
    pub system_vendor: HardwareValue<Vec<u8>>,
    pub product_name: HardwareValue<Vec<u8>>,
    pub product_version: HardwareValue<Vec<u8>>,
    pub board_vendor: HardwareValue<Vec<u8>>,
    pub board_name: HardwareValue<Vec<u8>>,
    pub bios_vendor: HardwareValue<Vec<u8>>,
    pub bios_version: HardwareValue<Vec<u8>>,
    pub sample: Sample,
}
impl Default for HardwareInfo {
    fn default() -> Self {
        Self {
            system_vendor: HardwareValue::NotSampled,
            product_name: HardwareValue::NotSampled,
            product_version: HardwareValue::NotSampled,
            board_vendor: HardwareValue::NotSampled,
            board_name: HardwareValue::NotSampled,
            bios_vendor: HardwareValue::NotSampled,
            bios_version: HardwareValue::NotSampled,
            sample: Sample::default(),
        }
    }
}

pub struct Hardware {
    root: PathBuf,
    temperatures: Vec<TemperatureSensor>,
    gpus: Vec<Gpu>,
    info: HardwareInfo,
    started: Instant,
    inventory_sample: Sample,
    read_buffer: [u8; 4097],
}
impl Default for Hardware {
    fn default() -> Self {
        Self::new()
    }
}
impl Hardware {
    pub fn new() -> Self {
        Self::with_sys_root("/sys")
    }
    pub fn with_sys_root(root: impl AsRef<Path>) -> Self {
        Self {
            root: root.as_ref().to_owned(),
            temperatures: Vec::new(),
            gpus: Vec::new(),
            info: HardwareInfo::default(),
            started: Instant::now(),
            inventory_sample: Sample::default(),
            read_buffer: [0; 4097],
        }
    }
    pub fn temperatures(&self) -> &[TemperatureSensor] {
        &self.temperatures
    }
    pub fn gpus(&self) -> &[Gpu] {
        &self.gpus
    }
    pub fn info(&self) -> &HardwareInfo {
        &self.info
    }
    pub fn inventory_sample(&self) -> Sample {
        self.inventory_sample
    }

    /// Rediscovers hotplugged devices. Missing class directories mean no exposed devices.
    /// Other directory errors preserve the previous inventory and return an error.
    /// Both hwmon and thermal class views are retained; they may describe the same sensor.
    pub fn refresh_inventory(&mut self) -> io::Result<()> {
        let mut temperatures = Vec::new();
        for directory in directories(&self.root.join("class/hwmon"), b"hwmon")? {
            let chip: HardwareValue<Vec<u8>> = read_text(&directory.join("name"), &mut self.read_buffer).into();
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                let name = entry.file_name();
                let Some(index) = name.as_bytes().strip_prefix(b"temp").and_then(|s| s.strip_suffix(b"_input")) else {
                    continue;
                };
                if index.is_empty() || !index.iter().all(u8::is_ascii_digit) {
                    continue;
                }
                let prefix = format!("temp{}", std::str::from_utf8(index).map_err(|_| io::ErrorKind::InvalidData)?);
                temperatures.push(TemperatureSensor {
                    path: entry.path(),
                    source: TemperatureSource::Hwmon,
                    chip: chip.clone(),
                    label: read_text(&directory.join(format!("{prefix}_label")), &mut self.read_buffer).into(),
                    raw_input: HardwareValue::NotSampled,
                    raw_critical: HardwareValue::NotSampled,
                    fault: HardwareValue::NotSampled,
                    sample: Sample::default(),
                    critical_path: Some(directory.join(format!("{prefix}_crit"))),
                    fault_path: Some(directory.join(format!("{prefix}_fault"))),
                });
            }
        }
        for directory in directories(&self.root.join("class/thermal"), b"thermal_zone")? {
            temperatures.push(TemperatureSensor {
                path: directory.join("temp"),
                source: TemperatureSource::ThermalZone,
                chip: read_text(&directory.join("type"), &mut self.read_buffer).into(),
                label: HardwareValue::Unavailable(io::ErrorKind::NotFound),
                raw_input: HardwareValue::NotSampled,
                raw_critical: HardwareValue::NotSampled,
                fault: HardwareValue::NotSampled,
                sample: Sample::default(),
                critical_path: None,
                fault_path: None,
            });
        }
        temperatures.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        let mut gpus = Vec::new();
        for directory in directories(&self.root.join("class/drm"), b"card")? {
            let device_path = directory.join("device");
            let driver = fs::read_link(device_path.join("driver"))
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.as_bytes().to_vec())
                        .ok_or_else(|| io::ErrorKind::InvalidData.into())
                })
                .into();
            gpus.push(Gpu {
                card: directory.file_name().ok_or(io::ErrorKind::InvalidData)?.as_bytes().to_vec(),
                vendor_id: read_hex(&device_path.join("vendor"), &mut self.read_buffer).into(),
                device_id: read_hex(&device_path.join("device"), &mut self.read_buffer).into(),
                driver,
                product_name: read_text(&device_path.join("product_name"), &mut self.read_buffer).into(),
                device_path,
                vram_total_bytes: HardwareValue::NotSampled,
                vram_used_bytes: HardwareValue::NotSampled,
                busy_percent: HardwareValue::NotSampled,
                sample: Sample::default(),
            });
        }
        self.temperatures = temperatures;
        self.gpus = gpus;
        advance(&mut self.inventory_sample, self.started);
        Ok(())
    }
    /// Reads DMI descriptions only. Serial numbers and machine UUIDs are intentionally not part of this API.
    pub fn refresh_info(&mut self) -> &HardwareInfo {
        let root = self.root.join("class/dmi/id");
        self.info.system_vendor = read_text(&root.join("sys_vendor"), &mut self.read_buffer).into();
        self.info.product_name = read_text(&root.join("product_name"), &mut self.read_buffer).into();
        self.info.product_version = read_text(&root.join("product_version"), &mut self.read_buffer).into();
        self.info.board_vendor = read_text(&root.join("board_vendor"), &mut self.read_buffer).into();
        self.info.board_name = read_text(&root.join("board_name"), &mut self.read_buffer).into();
        self.info.bios_vendor = read_text(&root.join("bios_vendor"), &mut self.read_buffer).into();
        self.info.bios_version = read_text(&root.join("bios_version"), &mut self.read_buffer).into();
        advance(&mut self.info.sample, self.started);
        &self.info
    }
    /// Refreshes known sensors. Individual read errors replace their fields with `Unavailable`.
    pub fn refresh_temperatures(&mut self) {
        for sensor in &mut self.temperatures {
            sensor.raw_input = read_number(&sensor.path, &mut self.read_buffer).into();
            sensor.raw_critical =
                sensor.critical_path.as_ref().map_or(HardwareValue::Unavailable(io::ErrorKind::Unsupported), |path| {
                    read_number(path, &mut self.read_buffer).into()
                });
            sensor.fault =
                sensor.fault_path.as_ref().map_or(HardwareValue::Unavailable(io::ErrorKind::Unsupported), |path| {
                    read_number::<u8>(path, &mut self.read_buffer)
                        .and_then(|value| match value {
                            0 => Ok(false),
                            1 => Ok(true),
                            _ => Err(io::ErrorKind::InvalidData.into()),
                        })
                        .into()
                });
            advance(&mut sensor.sample, self.started);
        }
    }
    /// Reads optional driver telemetry; unsupported vendors remain explicit missing values.
    pub fn refresh_gpus(&mut self) {
        for gpu in &mut self.gpus {
            gpu.vram_total_bytes =
                read_number(&gpu.device_path.join("mem_info_vram_total"), &mut self.read_buffer).into();
            gpu.vram_used_bytes =
                read_number(&gpu.device_path.join("mem_info_vram_used"), &mut self.read_buffer).into();
            gpu.busy_percent = read_number::<u32>(&gpu.device_path.join("gpu_busy_percent"), &mut self.read_buffer)
                .and_then(|value| if value <= 100 { Ok(value) } else { Err(io::ErrorKind::InvalidData.into()) })
                .into();
            advance(&mut gpu.sample, self.started);
        }
    }
}
fn advance(sample: &mut Sample, started: Instant) {
    sample.sequence = sample.sequence.wrapping_add(1);
    sample.timestamp_ns = started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
}
fn directories(root: &Path, prefix: &[u8]) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut result = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let Some(suffix) = name.as_bytes().strip_prefix(prefix) else { continue };
        if !suffix.is_empty() && suffix.iter().all(u8::is_ascii_digit) {
            result.push(entry.path());
        }
    }
    result.sort_unstable();
    Ok(result)
}
fn read_attribute<'a>(path: &Path, buffer: &'a mut [u8; 4097]) -> io::Result<&'a [u8]> {
    let mut file = File::open(path)?;
    let mut used = 0;
    loop {
        let len = match file.read(&mut buffer[used..]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if len == 0 {
            return Ok(buffer[..used].trim_ascii());
        }
        used += len;
        if used == buffer.len() {
            return Err(io::ErrorKind::InvalidData.into());
        }
    }
}
fn read_text(path: &Path, buffer: &mut [u8; 4097]) -> io::Result<Vec<u8>> {
    Ok(read_attribute(path, buffer)?.to_vec())
}
fn read_number<T: std::str::FromStr>(path: &Path, buffer: &mut [u8; 4097]) -> io::Result<T> {
    std::str::from_utf8(read_attribute(path, buffer)?)
        .map_err(|_| io::ErrorKind::InvalidData)?
        .parse()
        .map_err(|_| io::ErrorKind::InvalidData.into())
}
fn read_hex(path: &Path, buffer: &mut [u8; 4097]) -> io::Result<u32> {
    let raw = read_attribute(path, buffer)?;
    let raw = raw.strip_prefix(b"0x").unwrap_or(raw);
    u32::from_str_radix(std::str::from_utf8(raw).map_err(|_| io::ErrorKind::InvalidData)?, 16)
        .map_err(|_| io::ErrorKind::InvalidData.into())
}
