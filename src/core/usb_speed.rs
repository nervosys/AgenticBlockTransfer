// USB speed detection module — detect USB port speed and warn about suboptimal connections.
//
// Inspired by Rufus's device enumeration in dev.c which reads USB connection info
// via IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2 to detect whether a device
// is running at USB 1.0/1.1/2.0/3.0/3.1/3.2/4.0 speeds. Warns users when a
// USB 3.0+ capable device is plugged into a USB 2.0 port.
//
// On Linux, reads from sysfs: /sys/bus/usb/devices/*/speed
// On macOS, uses IOKit to query USB device properties.
// On Windows, uses SetupAPI/USB IOCTLs.

use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fmt;

/// USB speed classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum UsbSpeed {
    /// USB 1.0 Low Speed (1.5 Mbps).
    Usb1Low,
    /// USB 1.1 Full Speed (12 Mbps).
    Usb1Full,
    /// USB 2.0 High Speed (480 Mbps).
    Usb2High,
    /// USB 3.0 / 3.1 Gen 1 SuperSpeed (5 Gbps).
    Usb3Super,
    /// USB 3.1 Gen 2 / 3.2 Gen 2 SuperSpeed+ (10 Gbps).
    Usb3SuperPlus,
    /// USB 3.2 Gen 2x2 (20 Gbps).
    Usb32Gen2x2,
    /// USB4 (40 Gbps).
    Usb4,
    /// Unknown / not a USB device.
    Unknown,
}

impl UsbSpeed {
    /// Theoretical maximum throughput in megabytes/sec.
    pub fn max_throughput_mbps(&self) -> f64 {
        match self {
            UsbSpeed::Usb1Low => 0.1875,    // 1.5 Mbps / 8
            UsbSpeed::Usb1Full => 1.5,      // 12 Mbps / 8
            UsbSpeed::Usb2High => 60.0,     // 480 Mbps / 8
            UsbSpeed::Usb3Super => 625.0,   // 5 Gbps / 8
            UsbSpeed::Usb3SuperPlus => 1250.0, // 10 Gbps / 8
            UsbSpeed::Usb32Gen2x2 => 2500.0,  // 20 Gbps / 8
            UsbSpeed::Usb4 => 5000.0,       // 40 Gbps / 8
            UsbSpeed::Unknown => 0.0,
        }
    }

    /// Parse from a Linux sysfs speed value (in Mbps).
    pub fn from_linux_speed(speed_mbps: u32) -> Self {
        match speed_mbps {
            0..=2 => UsbSpeed::Usb1Low,
            3..=12 => UsbSpeed::Usb1Full,
            13..=480 => UsbSpeed::Usb2High,
            481..=5000 => UsbSpeed::Usb3Super,
            5001..=10000 => UsbSpeed::Usb3SuperPlus,
            10001..=20000 => UsbSpeed::Usb32Gen2x2,
            _ => UsbSpeed::Usb4,
        }
    }

    /// Whether this speed is considered "slow" for disk imaging.
    pub fn is_slow_for_imaging(&self) -> bool {
        matches!(self, UsbSpeed::Usb1Low | UsbSpeed::Usb1Full | UsbSpeed::Usb2High)
    }

    /// Short descriptive name.
    pub fn short_name(&self) -> &'static str {
        match self {
            UsbSpeed::Usb1Low => "USB 1.0",
            UsbSpeed::Usb1Full => "USB 1.1",
            UsbSpeed::Usb2High => "USB 2.0",
            UsbSpeed::Usb3Super => "USB 3.0",
            UsbSpeed::Usb3SuperPlus => "USB 3.1 Gen2",
            UsbSpeed::Usb32Gen2x2 => "USB 3.2 Gen2x2",
            UsbSpeed::Usb4 => "USB4",
            UsbSpeed::Unknown => "Unknown",
        }
    }
}

impl fmt::Display for UsbSpeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({:.0} MB/s max)",
            self.short_name(),
            self.max_throughput_mbps()
        )
    }
}

/// USB device information (VID, PID, speed, capability).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsbDeviceInfo {
    /// Device path (e.g., /dev/sdb, \\.\PhysicalDrive1).
    pub device_path: String,
    /// USB Vendor ID.
    pub vid: u16,
    /// USB Product ID.
    pub pid: u16,
    /// Current connection speed.
    pub current_speed: UsbSpeed,
    /// Maximum device capability (e.g., USB 3.0 device on USB 2.0 port).
    pub device_capability: UsbSpeed,
    /// Whether the device is running below its capability.
    pub speed_degraded: bool,
    /// USB serial number string.
    pub serial: Option<String>,
    /// USB hub port number.
    pub port: Option<u32>,
    /// USB bus number.
    pub bus: Option<u32>,
}

impl UsbDeviceInfo {
    /// Generate a warning message if speed is degraded.
    pub fn speed_warning(&self) -> Option<String> {
        if self.speed_degraded {
            Some(format!(
                "Device {} is a {} device but connected at {} speed. \
                 Consider plugging it into a {} port for faster writes.",
                self.device_path,
                self.device_capability.short_name(),
                self.current_speed.short_name(),
                self.device_capability.short_name()
            ))
        } else {
            None
        }
    }

    /// Estimated write time for a given image size (very rough, assuming 50% of theoretical max).
    pub fn estimated_write_secs(&self, image_size_bytes: u64) -> f64 {
        let throughput = self.current_speed.max_throughput_mbps() * 0.5; // 50% of theoretical
        if throughput <= 0.0 {
            return 0.0;
        }
        let size_mb = image_size_bytes as f64 / (1024.0 * 1024.0);
        size_mb / throughput
    }
}

impl fmt::Display for UsbDeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} [VID:{:04x} PID:{:04x}] {}",
            self.device_path, self.vid, self.pid, self.current_speed
        )?;
        if self.speed_degraded {
            write!(f, " (capable of {})", self.device_capability.short_name())?;
        }
        Ok(())
    }
}

/// Detect USB speed for a block device.
///
/// Platform-specific implementation:
/// - Linux: reads from sysfs
/// - macOS: uses IOKit (stub)
/// - Windows: uses SetupAPI (stub)
pub fn detect_usb_speed(device_path: &str) -> Result<UsbDeviceInfo> {
    #[cfg(target_os = "linux")]
    {
        detect_usb_speed_linux(device_path)
    }
    #[cfg(target_os = "windows")]
    {
        detect_usb_speed_windows(device_path)
    }
    #[cfg(target_os = "macos")]
    {
        detect_usb_speed_macos(device_path)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        Ok(UsbDeviceInfo {
            device_path: device_path.to_string(),
            vid: 0,
            pid: 0,
            current_speed: UsbSpeed::Unknown,
            device_capability: UsbSpeed::Unknown,
            speed_degraded: false,
            serial: None,
            port: None,
            bus: None,
        })
    }
}

/// Linux: detect USB speed from sysfs.
#[cfg(target_os = "linux")]
fn detect_usb_speed_linux(device_path: &str) -> Result<UsbDeviceInfo> {
    // Extract device name (e.g., "sdb" from "/dev/sdb")
    let dev_name = device_path.rsplit('/').next().unwrap_or(device_path);

    // Find the USB device in sysfs by following the block device symlink chain
    let sysfs_block = format!("/sys/block/{}", dev_name);
    let sysfs_device = if Path::new(&sysfs_block).exists() {
        // Follow the device symlink to find the USB ancestor
        let device_link = std::fs::read_link(format!("{}/device", sysfs_block))
            .unwrap_or_default();
        device_link.to_string_lossy().to_string()
    } else {
        String::new()
    };

    // Walk up the sysfs tree to find a USB device with speed info
    let mut vid = 0u16;
    let mut pid = 0u16;
    let mut speed = UsbSpeed::Unknown;
    let mut serial = None;
    let mut bus = None;
    let mut port = None;

    // Search common USB sysfs paths
    let usb_devices_dir = Path::new("/sys/bus/usb/devices");
    if usb_devices_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(usb_devices_dir) {
            for entry in entries.flatten() {
                let path = entry.path();

                // Check if this USB device is associated with our block device
                let usb_path_str = path.to_string_lossy().to_string();
                if !sysfs_device.is_empty() && !sysfs_device.contains(&usb_path_str) {
                    // Try checking if this USB device has our block device as a child
                    let glob_pattern = format!("{}/host*/target*/*/block/{}", path.display(), dev_name);
                    let has_block = std::fs::read_dir(&path)
                        .map(|_| Path::new(&glob_pattern).exists())
                        .unwrap_or(false);
                    if !has_block {
                        continue;
                    }
                }

                // Read speed
                if let Ok(speed_str) = std::fs::read_to_string(path.join("speed")) {
                    if let Ok(speed_val) = speed_str.trim().parse::<u32>() {
                        speed = UsbSpeed::from_linux_speed(speed_val);
                    }
                }

                // Read VID/PID
                if let Ok(vid_str) = std::fs::read_to_string(path.join("idVendor")) {
                    vid = u16::from_str_radix(vid_str.trim(), 16).unwrap_or(0);
                }
                if let Ok(pid_str) = std::fs::read_to_string(path.join("idProduct")) {
                    pid = u16::from_str_radix(pid_str.trim(), 16).unwrap_or(0);
                }

                // Read serial
                if let Ok(serial_str) = std::fs::read_to_string(path.join("serial")) {
                    let s = serial_str.trim().to_string();
                    if !s.is_empty() {
                        serial = Some(s);
                    }
                }

                // Read bus/port
                if let Ok(busnum) = std::fs::read_to_string(path.join("busnum")) {
                    bus = busnum.trim().parse().ok();
                }
                if let Ok(devpath) = std::fs::read_to_string(path.join("devpath")) {
                    port = devpath.trim().parse().ok();
                }

                if vid > 0 {
                    break;
                }
            }
        }
    }

    // For now, assume device_capability = current_speed (no way to detect
    // device's max capability from sysfs reliably without USB descriptor queries)
    let device_capability = speed;

    Ok(UsbDeviceInfo {
        device_path: device_path.to_string(),
        vid,
        pid,
        current_speed: speed,
        device_capability,
        speed_degraded: false,
        serial,
        port,
        bus,
    })
}

/// Windows: detect USB speed (stub — reads device info from registry).
#[cfg(target_os = "windows")]
fn detect_usb_speed_windows(device_path: &str) -> Result<UsbDeviceInfo> {
    // On Windows, full implementation would use SetupAPI and
    // IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX_V2.
    // For now, return a basic stub that reports Unknown speed.
    info!("USB speed detection on Windows: querying {}", device_path);

    Ok(UsbDeviceInfo {
        device_path: device_path.to_string(),
        vid: 0,
        pid: 0,
        current_speed: UsbSpeed::Unknown,
        device_capability: UsbSpeed::Unknown,
        speed_degraded: false,
        serial: None,
        port: None,
        bus: None,
    })
}

/// macOS: detect USB speed (stub).
#[cfg(target_os = "macos")]
fn detect_usb_speed_macos(device_path: &str) -> Result<UsbDeviceInfo> {
    info!("USB speed detection on macOS: querying {}", device_path);

    Ok(UsbDeviceInfo {
        device_path: device_path.to_string(),
        vid: 0,
        pid: 0,
        current_speed: UsbSpeed::Unknown,
        device_capability: UsbSpeed::Unknown,
        speed_degraded: false,
        serial: None,
        port: None,
        bus: None,
    })
}

/// Check if a device is running at a degraded USB speed and produce a warning.
pub fn check_and_warn_speed(device_path: &str) -> Option<String> {
    match detect_usb_speed(device_path) {
        Ok(info) => {
            if let Some(warning) = info.speed_warning() {
                warn!("{}", warning);
                Some(warning)
            } else if info.current_speed.is_slow_for_imaging() && info.current_speed != UsbSpeed::Unknown {
                let msg = format!(
                    "Device {} is connected at {} — writes may be slow for large images",
                    device_path,
                    info.current_speed
                );
                warn!("{}", msg);
                Some(msg)
            } else {
                info!("USB speed for {}: {}", device_path, info.current_speed);
                None
            }
        }
        Err(e) => {
            warn!("Could not detect USB speed for {}: {}", device_path, e);
            None
        }
    }
}

/// Format a duration in seconds as a human-readable time estimate.
pub fn format_eta(seconds: f64) -> String {
    if seconds <= 0.0 {
        return "unknown".to_string();
    }
    let secs = seconds as u64;
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usb_speed_ordering() {
        assert!(UsbSpeed::Usb1Low < UsbSpeed::Usb1Full);
        assert!(UsbSpeed::Usb1Full < UsbSpeed::Usb2High);
        assert!(UsbSpeed::Usb2High < UsbSpeed::Usb3Super);
        assert!(UsbSpeed::Usb3Super < UsbSpeed::Usb3SuperPlus);
        assert!(UsbSpeed::Usb3SuperPlus < UsbSpeed::Usb32Gen2x2);
        assert!(UsbSpeed::Usb32Gen2x2 < UsbSpeed::Usb4);
    }

    #[test]
    fn test_usb_speed_throughput() {
        assert!(UsbSpeed::Usb2High.max_throughput_mbps() > 0.0);
        assert!(UsbSpeed::Usb3Super.max_throughput_mbps() > UsbSpeed::Usb2High.max_throughput_mbps());
        assert_eq!(UsbSpeed::Unknown.max_throughput_mbps(), 0.0);
    }

    #[test]
    fn test_from_linux_speed() {
        assert_eq!(UsbSpeed::from_linux_speed(1), UsbSpeed::Usb1Low);
        assert_eq!(UsbSpeed::from_linux_speed(12), UsbSpeed::Usb1Full);
        assert_eq!(UsbSpeed::from_linux_speed(480), UsbSpeed::Usb2High);
        assert_eq!(UsbSpeed::from_linux_speed(5000), UsbSpeed::Usb3Super);
        assert_eq!(UsbSpeed::from_linux_speed(10000), UsbSpeed::Usb3SuperPlus);
        assert_eq!(UsbSpeed::from_linux_speed(20000), UsbSpeed::Usb32Gen2x2);
        assert_eq!(UsbSpeed::from_linux_speed(40000), UsbSpeed::Usb4);
    }

    #[test]
    fn test_is_slow_for_imaging() {
        assert!(UsbSpeed::Usb1Low.is_slow_for_imaging());
        assert!(UsbSpeed::Usb1Full.is_slow_for_imaging());
        assert!(UsbSpeed::Usb2High.is_slow_for_imaging());
        assert!(!UsbSpeed::Usb3Super.is_slow_for_imaging());
        assert!(!UsbSpeed::Usb4.is_slow_for_imaging());
        assert!(!UsbSpeed::Unknown.is_slow_for_imaging());
    }

    #[test]
    fn test_usb_speed_display() {
        let s = format!("{}", UsbSpeed::Usb3Super);
        assert!(s.contains("USB 3.0"));
        assert!(s.contains("625"));
    }

    #[test]
    fn test_usb_speed_short_name() {
        assert_eq!(UsbSpeed::Usb2High.short_name(), "USB 2.0");
        assert_eq!(UsbSpeed::Usb3Super.short_name(), "USB 3.0");
        assert_eq!(UsbSpeed::Usb4.short_name(), "USB4");
    }

    #[test]
    fn test_usb_device_info_display() {
        let info = UsbDeviceInfo {
            device_path: "/dev/sdb".into(),
            vid: 0x1234,
            pid: 0x5678,
            current_speed: UsbSpeed::Usb2High,
            device_capability: UsbSpeed::Usb3Super,
            speed_degraded: true,
            serial: None,
            port: None,
            bus: None,
        };
        let s = format!("{}", info);
        assert!(s.contains("/dev/sdb"));
        assert!(s.contains("1234"));
        assert!(s.contains("USB 3.0")); // capability shown because degraded
    }

    #[test]
    fn test_speed_warning_degraded() {
        let info = UsbDeviceInfo {
            device_path: "/dev/sdb".into(),
            vid: 0,
            pid: 0,
            current_speed: UsbSpeed::Usb2High,
            device_capability: UsbSpeed::Usb3Super,
            speed_degraded: true,
            serial: None,
            port: None,
            bus: None,
        };
        let warning = info.speed_warning();
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("USB 3.0"));
    }

    #[test]
    fn test_speed_warning_not_degraded() {
        let info = UsbDeviceInfo {
            device_path: "/dev/sdb".into(),
            vid: 0,
            pid: 0,
            current_speed: UsbSpeed::Usb3Super,
            device_capability: UsbSpeed::Usb3Super,
            speed_degraded: false,
            serial: None,
            port: None,
            bus: None,
        };
        assert!(info.speed_warning().is_none());
    }

    #[test]
    fn test_estimated_write_secs() {
        let info = UsbDeviceInfo {
            device_path: "/dev/sdb".into(),
            vid: 0,
            pid: 0,
            current_speed: UsbSpeed::Usb2High,
            device_capability: UsbSpeed::Usb2High,
            speed_degraded: false,
            serial: None,
            port: None,
            bus: None,
        };
        // 1 GB at USB 2.0 (60 MB/s * 0.5 = 30 MB/s) ≈ 34 seconds
        let secs = info.estimated_write_secs(1_073_741_824);
        assert!(secs > 30.0);
        assert!(secs < 40.0);
    }

    #[test]
    fn test_estimated_write_secs_unknown() {
        let info = UsbDeviceInfo {
            device_path: "/dev/sdb".into(),
            vid: 0,
            pid: 0,
            current_speed: UsbSpeed::Unknown,
            device_capability: UsbSpeed::Unknown,
            speed_degraded: false,
            serial: None,
            port: None,
            bus: None,
        };
        assert_eq!(info.estimated_write_secs(1_000_000), 0.0);
    }

    #[test]
    fn test_format_eta() {
        assert_eq!(format_eta(0.0), "unknown");
        assert_eq!(format_eta(30.0), "30s");
        assert_eq!(format_eta(90.0), "1m 30s");
        assert_eq!(format_eta(3661.0), "1h 1m");
    }

    #[test]
    fn test_usb_speed_serde() {
        let speed = UsbSpeed::Usb3Super;
        let json = serde_json::to_string(&speed).unwrap();
        let parsed: UsbSpeed = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, UsbSpeed::Usb3Super);
    }

    #[test]
    fn test_usb_device_info_serde() {
        let info = UsbDeviceInfo {
            device_path: "/dev/sdc".into(),
            vid: 0x0781,
            pid: 0x5583,
            current_speed: UsbSpeed::Usb3Super,
            device_capability: UsbSpeed::Usb3Super,
            speed_degraded: false,
            serial: Some("SN123456".into()),
            port: Some(3),
            bus: Some(2),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: UsbDeviceInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.vid, 0x0781);
        assert_eq!(parsed.serial, Some("SN123456".into()));
    }
}
