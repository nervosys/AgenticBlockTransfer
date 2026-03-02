use anyhow::Result;

use crate::core::device::{DeviceEnumerator, DeviceInfo};
use crate::core::types::DeviceType;

pub struct MacDeviceEnumerator;

impl MacDeviceEnumerator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl DeviceEnumerator for MacDeviceEnumerator {
    async fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        let output = tokio::process::Command::new("diskutil")
            .args(["list", "-plist"])
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!("diskutil failed");
        }

        // Parse diskutil output for whole disks
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut devices = Vec::new();

        // Simpler approach: use diskutil info for each disk
        let list_output = tokio::process::Command::new("diskutil")
            .args(["list"])
            .output()
            .await?;

        let list_str = String::from_utf8_lossy(&list_output.stdout);
        for line in list_str.lines() {
            if line.starts_with("/dev/disk") {
                let disk_path = line.split_whitespace().next().unwrap_or("");
                let disk_path = disk_path.trim_end_matches(':');
                if let Ok(info) = self.get_device(disk_path).await {
                    devices.push(info);
                }
            }
        }

        Ok(devices)
    }

    async fn get_device(&self, path: &str) -> Result<DeviceInfo> {
        let output = tokio::process::Command::new("diskutil")
            .args(["info", path])
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut name = String::new();
        let mut size: u64 = 0;
        let mut removable = false;
        let mut read_only = false;
        let mut is_internal = false;

        for line in stdout.lines() {
            let line = line.trim();
            if let Some((key, val)) = line.split_once(':') {
                let key = key.trim();
                let val = val.trim();
                match key {
                    "Device / Media Name" => name = val.to_string(),
                    "Disk Size" => {
                        // Parse "X.X GB (NNNN Bytes)..."
                        if let Some(bytes_str) = val.split('(').nth(1) {
                            if let Some(num) = bytes_str.split_whitespace().next() {
                                size = num.parse().unwrap_or(0);
                            }
                        }
                    }
                    "Removable Media" => removable = val.contains("Removable"),
                    "Read-Only Media" => read_only = val == "Yes",
                    "Internal" => is_internal = val == "Yes",
                    _ => {}
                }
            }
        }

        let device_type = if removable {
            DeviceType::Usb
        } else if is_internal {
            DeviceType::Nvme
        } else {
            DeviceType::Unknown
        };

        Ok(DeviceInfo {
            path: path.to_string(),
            name,
            vendor: String::new(),
            serial: None,
            size,
            sector_size: 512,
            physical_sector_size: 4096,
            removable,
            read_only,
            is_system: is_internal && path == "/dev/disk0",
            device_type,
            mount_points: Vec::new(),
            transport: format!("{}", device_type),
        })
    }

    async fn unmount_device(&self, path: &str) -> Result<()> {
        let output = tokio::process::Command::new("diskutil")
            .args(["unmountDisk", path])
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to unmount: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }

    async fn eject_device(&self, path: &str) -> Result<()> {
        let output = tokio::process::Command::new("diskutil")
            .args(["eject", path])
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "Failed to eject: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(())
    }
}
