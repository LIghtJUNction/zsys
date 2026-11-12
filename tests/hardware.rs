#![cfg(all(feature = "std", target_os = "linux"))]
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use zsys::linux::hardware::{Attribute, Gpus, HardwareInfo, Temperatures};
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
    fn write(&self, path: &str, value: &str) {
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
fn sensors_support_negative_values_and_explicit_errors() {
    let f = Fixture::new();
    f.write("class/hwmon/hwmon0/name", "fixture\n");
    f.write("class/hwmon/hwmon0/temp1_input", "-12500\n");
    f.write("class/hwmon/hwmon0/temp1_label", "Package\n");
    f.write("class/hwmon/hwmon0/temp1_crit", "100000\n");
    let mut c = Temperatures::with_sys_root(&f.0);
    c.discover().unwrap();
    assert!(c.iter().next().unwrap().reading.is_none());
    assert_eq!(c.refresh(), (1, 0));
    let reading = c.iter().next().unwrap().reading.unwrap();
    assert_eq!(reading.millidegrees_celsius, Attribute::Value(-12500));
    assert_eq!(reading.critical_millidegrees_celsius, Attribute::Value(100000));
    f.write("class/hwmon/hwmon0/temp1_input", "not-a-number");
    assert_eq!(c.refresh(), (0, 1));
    assert_eq!(
        c.iter().next().unwrap().reading.unwrap().millidegrees_celsius,
        Attribute::Failed(std::io::ErrorKind::InvalidData)
    );
    fs::remove_file(f.0.join("class/hwmon/hwmon0/temp1_input")).unwrap();
    assert_eq!(c.refresh(), (0, 1));
    c.discover().unwrap();
    assert_eq!(c.iter().count(), 0);
}
#[test]
fn gpu_capabilities_are_optional_and_do_not_include_connector_nodes() {
    let f = Fixture::new();
    f.write("class/drm/card0/device/vendor", "0x1002\n");
    f.write("class/drm/card0/device/device", "0xabcd\n");
    f.write("class/drm/card0/device/gpu_busy_percent", "0\n");
    f.write("class/drm/card0/device/mem_info_vram_total", "8589934592\n");
    f.write("class/drm/card0/device/mem_info_vram_used", "1024\n");
    fs::create_dir_all(f.0.join("class/drm/card0-DP-1")).unwrap();
    let mut g = Gpus::with_sys_root(&f.0);
    g.discover().unwrap();
    g.refresh();
    assert_eq!(g.iter().count(), 1);
    let device = g.iter().next().unwrap();
    assert_eq!(device.vendor_id, Attribute::Value(0x1002));
    assert_eq!(device.device_id, Attribute::Value(0xabcd));
    assert_eq!(device.reading.unwrap().utilization_percent, Attribute::Value(0));
    assert_eq!(device.reading.unwrap().vram_total_bytes, Attribute::Value(8589934592));
    f.write("class/drm/card0/device/gpu_busy_percent", "101");
    fs::remove_file(f.0.join("class/drm/card0/device/mem_info_vram_used")).unwrap();
    g.refresh();
    let value = g.iter().next().unwrap().reading.unwrap();
    assert_eq!(value.utilization_percent, Attribute::Failed(std::io::ErrorKind::InvalidData));
    assert_eq!(value.vram_used_bytes, Attribute::Unavailable);
}
#[test]
fn dmi_inventory_does_not_require_or_read_serial_numbers() {
    let f = Fixture::new();
    f.write("class/dmi/id/sys_vendor", "Fixture\n");
    f.write("class/dmi/id/product_name", "Test machine\n");
    let info = HardwareInfo::with_sys_root(&f.0);
    assert_eq!(info.system_vendor, Attribute::Value(b"Fixture".to_vec()));
    assert_eq!(info.bios_version, Attribute::Unavailable);
}
