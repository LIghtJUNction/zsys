#![cfg(all(feature = "std", target_os = "linux"))]
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use zsys::linux::{Hardware, HardwareValue, TemperatureSource};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "zsys-hardware-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, value: &[u8]) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn temperature_units_hotplug_faults_and_missing_values() {
    let f = Fixture::new();
    f.write("class/hwmon/hwmon0/name", b"fixture_chip\n");
    f.write("class/hwmon/hwmon0/temp1_input", b"42500\n");
    f.write("class/hwmon/hwmon0/temp1_label", b"board\n");
    f.write("class/hwmon/hwmon0/temp1_crit", b"100000\n");
    f.write("class/hwmon/hwmon0/temp1_fault", b"0\n");
    f.write("class/thermal/thermal_zone0/type", b"fixture_thermal\n");
    f.write("class/thermal/thermal_zone0/temp", b"-1250\n");
    let mut hardware = Hardware::with_sys_root(&f.0);
    hardware.refresh_inventory().unwrap();
    assert_eq!(hardware.temperatures().len(), 2);
    assert!(hardware.temperatures().iter().all(|s| s.raw_input == HardwareValue::NotSampled));
    hardware.refresh_temperatures();
    let hwmon = hardware.temperatures().iter().find(|s| s.source == TemperatureSource::Hwmon).unwrap();
    assert_eq!(hwmon.raw_input, HardwareValue::Value(42500));
    assert_eq!(hwmon.raw_critical, HardwareValue::Value(100000));
    assert_eq!(hwmon.fault, HardwareValue::Value(false));
    assert_eq!(hwmon.celsius(), None); // Driver-defined voltage channels are never silently converted.
    let thermal = hardware.temperatures().iter().find(|s| s.source == TemperatureSource::ThermalZone).unwrap();
    assert_eq!(thermal.celsius(), Some(-1.25));
    assert_eq!(thermal.sample.sequence, 1);
    f.write("class/hwmon/hwmon0/temp1_input", b"malformed\n");
    f.write("class/hwmon/hwmon0/temp1_fault", b"1\n");
    fs::remove_file(f.0.join("class/thermal/thermal_zone0/temp")).unwrap();
    hardware.refresh_temperatures();
    let hwmon = hardware.temperatures().iter().find(|s| s.source == TemperatureSource::Hwmon).unwrap();
    assert_eq!(hwmon.raw_input, HardwareValue::Unavailable(ErrorKind::InvalidData));
    assert_eq!(hwmon.fault, HardwareValue::Value(true));
    let thermal = hardware.temperatures().iter().find(|s| s.source == TemperatureSource::ThermalZone).unwrap();
    assert_eq!(thermal.raw_input, HardwareValue::Unavailable(ErrorKind::NotFound));
    fs::remove_dir_all(f.0.join("class/hwmon/hwmon0")).unwrap();
    hardware.refresh_inventory().unwrap();
    assert_eq!(hardware.temperatures().len(), 1);
}
#[test]
fn gpu_inventory_and_optional_telemetry_are_explicit() {
    let f = Fixture::new();
    f.write("class/drm/card0/device/vendor", b"0x1002\n");
    f.write("class/drm/card0/device/device", b"0x744c\n");
    f.write("class/drm/card0/device/product_name", b"fixture GPU\n");
    f.write("class/drm/card0/device/mem_info_vram_total", b"8589934592\n");
    f.write("class/drm/card0/device/mem_info_vram_used", b"1048576\n");
    f.write("class/drm/card0/device/gpu_busy_percent", b"35\n");
    fs::create_dir_all(f.0.join("class/drm/card0-HDMI-A-1")).unwrap();
    fs::create_dir_all(f.0.join("class/drm/renderD128")).unwrap();
    fs::create_dir_all(f.0.join("class/drm/card1/device")).unwrap();
    symlink("../../../../bus/pci/drivers/amdgpu", f.0.join("class/drm/card0/device/driver")).unwrap();
    let mut hardware = Hardware::with_sys_root(&f.0);
    hardware.refresh_inventory().unwrap();
    assert_eq!(hardware.gpus().len(), 2);
    assert_eq!(hardware.gpus()[0].vendor_id, HardwareValue::Value(0x1002));
    assert_eq!(hardware.gpus()[0].driver.value().unwrap(), b"amdgpu");
    hardware.refresh_gpus();
    assert_eq!(hardware.gpus()[0].vram_total_bytes, HardwareValue::Value(8_589_934_592));
    assert_eq!(hardware.gpus()[0].vram_used_bytes, HardwareValue::Value(1_048_576));
    assert_eq!(hardware.gpus()[0].busy_percent, HardwareValue::Value(35));
    assert_eq!(hardware.gpus()[1].busy_percent, HardwareValue::Unavailable(ErrorKind::NotFound));
    f.write("class/drm/card0/device/gpu_busy_percent", b"101\n");
    hardware.refresh_gpus();
    assert_eq!(hardware.gpus()[0].busy_percent, HardwareValue::Unavailable(ErrorKind::InvalidData));
}
#[test]
fn hardware_description_and_empty_inventory_are_valid() {
    let f = Fixture::new();
    f.write("class/dmi/id/sys_vendor", b"Fixture vendor\n");
    f.write("class/dmi/id/product_name", b"Fixture product\n");
    let mut hardware = Hardware::with_sys_root(&f.0);
    hardware.refresh_inventory().unwrap();
    assert!(hardware.temperatures().is_empty());
    assert!(hardware.gpus().is_empty());
    let info = hardware.refresh_info();
    assert_eq!(info.system_vendor.value().unwrap(), b"Fixture vendor");
    assert_eq!(info.product_name.value().unwrap(), b"Fixture product");
    assert_eq!(info.bios_version, HardwareValue::Unavailable(ErrorKind::NotFound));
    assert_eq!(info.sample.sequence, 1);
}
#[test]
fn inventory_failure_preserves_successful_snapshot() {
    let f = Fixture::new();
    f.write("class/thermal/thermal_zone0/type", b"fixture\n");
    f.write("class/thermal/thermal_zone0/temp", b"40000\n");
    let mut hardware = Hardware::with_sys_root(&f.0);
    hardware.refresh_inventory().unwrap();
    let sample = hardware.inventory_sample();
    f.write("class/hwmon/hwmon0", b"not a directory");
    assert!(hardware.refresh_inventory().is_err());
    assert_eq!(hardware.inventory_sample(), sample);
    assert_eq!(hardware.temperatures().len(), 1);
}
#[test]
fn live_hardware_handles_headless_containers() {
    let mut hardware = Hardware::new();
    hardware.refresh_inventory().unwrap();
    hardware.refresh_info();
    hardware.refresh_temperatures();
    hardware.refresh_gpus();
    assert_eq!(hardware.inventory_sample().sequence, 1);
}
