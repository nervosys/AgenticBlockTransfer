use anyhow::Result;
use std::path::Path;

use crate::core::device::{DeviceEnumerator, DeviceInfo};
use crate::core::types::DeviceType;

pub struct LinuxDeviceEnumerator;

impl LinuxDeviceEnumerator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl DeviceEnumerator for LinuxDeviceEnumerator {
    async fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let mut devices = Vec::new();

        // Read from /sys/block/ to enumerate block devices
        let block_dir = Path::new("/sys/block");
        if !block_dir.exists() {
            return Ok(devices);
        }

        for entry in std::fs::read_dir(block_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip loop, ram, zram, dm devices
            if name.starts_with("loop")
                || name.starts_with("ram")
                || name.starts_with("zram")
                || name.starts_with("dm-")
            {
                continue;
            }

            if let Ok(info) = read_device_info(&name) {
                devices.push(info);
            }
        }

        Ok(devices)
    }

    async fn get_device(&self, path: &str) -> Result<DeviceInfo> {
        let name = Path::new(path)
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Invalid device path: {}", path))?
            .to_string_lossy()
            .to_string();
        read_device_info(&name)
    }

    async fn unmount_device(&self, path: &str) -> Result<()> {
        // Find and unmount all partitions
        let name = Path::new(path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        let output = tokio::process::Command::new("lsblk")
            .args(["-nlo", "MOUNTPOINT", path])
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let mp = line.trim();
            if !mp.is_empty() {
                log::info!("Unmounting {}", mp);
                tokio::process::Command::new("umount")
                    .arg(mp)
                    .output()
                    .await?;
            }
        }

        Ok(())
    }

    async fn eject_device(&self, path: &str) -> Result<()> {
        self.unmount_device(path).await?;
        tokio::process::Command::new("eject")
            .arg(path)
            .output()
            .await?;
        Ok(())
    }
}

fn read_device_info(name: &str) -> Result<DeviceInfo> {
    let sys_path = format!("/sys/block/{}", name);
    let dev_path = format!("/dev/{}", name);

    let size_str = read_sysfs(&format!("{}/size", sys_path)).unwrap_or_default();
    let size: u64 = size_str.trim().parse::<u64>().unwrap_or(0) * 512;

    let removable_str = read_sysfs(&format!("{}/removable", sys_path)).unwrap_or_default();
    let removable = removable_str.trim() == "1";

    let ro_str = read_sysfs(&format!("{}/ro", sys_path)).unwrap_or_default();
    let read_only = ro_str.trim() == "1";

    let model = read_sysfs(&format!("{}/device/model", sys_path))
        .unwrap_or_else(|_| name.to_string())
        .trim()
        .to_string();

    let vendor = read_sysfs(&format!("{}/device/vendor", sys_path))
        .unwrap_or_default()
        .trim()
        .to_string();

    let sector_size_str =
        read_sysfs(&format!("{}/queue/logical_block_size", sys_path)).unwrap_or_default();
    let sector_size: u32 = sector_size_str.trim().parse().unwrap_or(512);

    let phys_sector_str =
        read_sysfs(&format!("{}/queue/physical_block_size", sys_path)).unwrap_or_default();
    let physical_sector_size: u32 = phys_sector_str.trim().parse().unwrap_or(512);

    // Determine device type
    let device_type = if name.starts_with("sd") {
        if removable {
            DeviceType::Usb
        } else {
            DeviceType::Sata
        }
    } else if name.starts_with("nvme") {
        DeviceType::Nvme
    } else if name.starts_with("mmcblk") {
        DeviceType::Mmc
    } else if name.starts_with("vd") {
        DeviceType::Virtual
    } else {
        DeviceType::Unknown
    };

    // Check if system drive (has / mounted)
    let is_system = check_is_system_drive(&dev_path);

    // Get mount points
    let mount_points = get_mount_points(&dev_path);

    Ok(DeviceInfo {
        path: dev_path,
        name: model,
        vendor,
        serial: None,
        size,
        sector_size,
        physical_sector_size,
        removable,
        read_only,
        is_system,
        device_type,
        mount_points,
        transport: format!("{}", device_type),
    })
}

fn read_sysfs(path: &str) -> Result<String> {
    Ok(std::fs::read_to_string(path)?)
}

fn check_is_system_drive(dev_path: &str) -> bool {
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                // Check if any partition of this device is mounted at /
                if parts[0].starts_with(dev_path) && parts[1] == "/" {
                    return true;
                }
            }
        }
    }
    false
}

fn get_mount_points(dev_path: &str) -> Vec<String> {
    let mut mounts = Vec::new();
    if let Ok(proc_mounts) = std::fs::read_to_string("/proc/mounts") {
        for line in proc_mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[0].starts_with(dev_path) {
                mounts.push(parts[1].to_string());
            }
        }
    }
    mounts
}
