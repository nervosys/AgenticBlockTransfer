// FreeBSD platform support — device enumeration via sysctl/geom.
//
// Uses `sysctl kern.disks` for disk name enumeration and `geom disk list`
// for detailed device attributes (mediasize, sectorsize, descr, ident).

use anyhow::{Context, Result};
use std::process::Command;

use crate::core::device::{DeviceEnumerator, DeviceInfo};
use crate::core::types::DeviceType;

pub struct FreeBsdDeviceEnumerator;

impl FreeBsdDeviceEnumerator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl DeviceEnumerator for FreeBsdDeviceEnumerator {
    async fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        // Get disk names from `sysctl -n kern.disks`
        let output = Command::new("sysctl")
            .args(["-n", "kern.disks"])
            .output()
            .context("Failed to run sysctl — is this FreeBSD?")?;

        if !output.status.success() {
            return Ok(devices);
        }

        let disk_names = String::from_utf8_lossy(&output.stdout);
        for name in disk_names.split_whitespace() {
            // Skip md (memory disk) and cd (CDROM) devices
            if name.starts_with("md") || name.starts_with("cd") {
                continue;
            }

            match read_geom_info(name) {
                Ok(info) => devices.push(info),
                Err(e) => log::debug!("Skipping device {}: {}", name, e),
            }
        }

        Ok(devices)
    }

    async fn get_device(&self, path: &str) -> Result<DeviceInfo> {
        // Extract device name from path (e.g., /dev/da0 → da0)
        let name = std::path::Path::new(path)
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid device path: {}", path))?
            .to_string_lossy();

        read_geom_info(&name)
    }

    async fn unmount_device(&self, path: &str) -> Result<()> {
        // Find mount points for this device's partitions
        let output = Command::new("mount").output()?;
        let mount_output = String::from_utf8_lossy(&output.stdout);
        let dev_name = std::path::Path::new(path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        for line in mount_output.lines() {
            // Mount lines look like: /dev/da0p1 on /mnt type ufs (...)
            if line.starts_with(&format!("/dev/{}", dev_name))
                || line.starts_with(&format!("/dev/{}p", dev_name))
                || line.starts_with(&format!("/dev/{}s", dev_name))
            {
                if let Some(mount_point) = line.split(" on ").nth(1) {
                    let mount_point = mount_point.split(' ').next().unwrap_or("");
                    if !mount_point.is_empty() {
                        log::info!("Unmounting {}", mount_point);
                        let _ = Command::new("umount").arg(mount_point).status();
                    }
                }
            }
        }

        Ok(())
    }

    async fn eject_device(&self, _path: &str) -> Result<()> {
        // FreeBSD does not have a standard eject for USB mass storage.
        // We just ensure it's unmounted.
        Ok(())
    }
}

/// Read device information using `geom disk list <name>`.
fn read_geom_info(name: &str) -> Result<DeviceInfo> {
    let output = Command::new("geom")
        .args(["disk", "list", name])
        .output()
        .with_context(|| format!("Failed to run geom disk list {}", name))?;

    if !output.status.success() {
        anyhow::bail!("geom disk list {} failed", name);
    }

    let geom_output = String::from_utf8_lossy(&output.stdout);

    let mut size: u64 = 0;
    let mut sector_size: u32 = 512;
    let mut descr = String::new();
    let mut ident = String::new();
    let mut rotation_rate: Option<u32> = None;

    for line in geom_output.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("Mediasize:") {
            // "Mediasize: 31914983424 (30G)"
            let val = val.trim();
            if let Some(num_str) = val.split_whitespace().next() {
                size = num_str.parse().unwrap_or(0);
            }
        } else if let Some(val) = line.strip_prefix("Sectorsize:") {
            sector_size = val.trim().parse().unwrap_or(512);
        } else if let Some(val) = line.strip_prefix("descr:") {
            descr = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("ident:") {
            ident = val.trim().to_string();
        } else if let Some(val) = line.strip_prefix("rotationrate:") {
            rotation_rate = val.trim().parse().ok();
        }
    }

    // Determine device type from name pattern
    let device_type = classify_freebsd_device(name, rotation_rate);

    // Determine if removable — da* devices (SCSI disk, including USB) are typically removable
    let removable = name.starts_with("da");

    // Extract vendor from descr (usually "Vendor Model")
    let (vendor, model) = if let Some(idx) = descr.find(' ') {
        (descr[..idx].to_string(), descr[idx + 1..].to_string())
    } else {
        (String::new(), descr.clone())
    };

    // Check mount points
    let mount_points = get_mount_points(name)?;

    // System drive heuristic: has root mount or is ada0/nvd0
    let is_system = mount_points.iter().any(|mp| mp == "/")
        || name == "ada0"
        || name == "nvd0"
        || name == "da0" && !removable;

    Ok(DeviceInfo {
        path: format!("/dev/{}", name),
        name: if model.is_empty() {
            name.to_string()
        } else {
            model
        },
        vendor,
        serial: if ident.is_empty() {
            None
        } else {
            Some(ident)
        },
        size,
        sector_size,
        physical_sector_size: sector_size,
        removable,
        read_only: false,
        is_system,
        device_type,
        mount_points,
        transport: device_type_to_transport(name),
    })
}

/// Classify FreeBSD device name to DeviceType.
fn classify_freebsd_device(name: &str, rotation_rate: Option<u32>) -> DeviceType {
    if name.starts_with("da") {
        DeviceType::Usb // SCSI disk — usually USB mass storage
    } else if name.starts_with("nvd") || name.starts_with("nvme") {
        DeviceType::Nvme
    } else if name.starts_with("ada") || name.starts_with("ad") {
        match rotation_rate {
            Some(0) | None => DeviceType::Sata, // 0 = SSD, None = unknown
            Some(_) => DeviceType::Sata,
        }
    } else if name.starts_with("mmcsd") {
        DeviceType::Mmc
    } else {
        DeviceType::Other
    }
}

/// Determine transport string from device name.
fn device_type_to_transport(name: &str) -> String {
    if name.starts_with("da") {
        "USB/SCSI".to_string()
    } else if name.starts_with("nvd") || name.starts_with("nvme") {
        "NVMe".to_string()
    } else if name.starts_with("ada") {
        "SATA".to_string()
    } else if name.starts_with("ad") {
        "ATA".to_string()
    } else if name.starts_with("mmcsd") {
        "MMC/SD".to_string()
    } else {
        "Unknown".to_string()
    }
}

/// Get mount points for a device and its partitions.
fn get_mount_points(name: &str) -> Result<Vec<String>> {
    let output = Command::new("mount").output()?;
    let mount_output = String::from_utf8_lossy(&output.stdout);
    let mut mounts = Vec::new();

    for line in mount_output.lines() {
        if line.starts_with(&format!("/dev/{}", name))
            || line.starts_with(&format!("/dev/{}p", name))
            || line.starts_with(&format!("/dev/{}s", name))
        {
            if let Some(rest) = line.split(" on ").nth(1) {
                if let Some(mp) = rest.split(' ').next() {
                    mounts.push(mp.to_string());
                }
            }
        }
    }

    Ok(mounts)
}

/// Check if running as root on FreeBSD.
pub fn is_elevated() -> bool {
    // SAFETY: geteuid() is a simple getter with no memory safety implications.
    unsafe { libc::geteuid() == 0 }
}
