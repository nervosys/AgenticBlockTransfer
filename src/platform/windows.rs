use anyhow::Result;

use crate::core::device::{DeviceEnumerator, DeviceInfo};
use crate::core::types::DeviceType;

pub struct WindowsDeviceEnumerator;

impl WindowsDeviceEnumerator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl DeviceEnumerator for WindowsDeviceEnumerator {
    async fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        // Use PowerShell Get-Disk for device enumeration
        let output = tokio::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Get-Disk | Select-Object Number,FriendlyName,SerialNumber,Size,BusType,IsSystem,IsReadOnly,PartitionStyle | ConvertTo-Json",
            ])
            .output()
            .await?;

        if !output.status.success() {
            anyhow::bail!(
                "Get-Disk failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut devices = Vec::new();

        // Parse JSON output
        if let Ok(disks) = serde_json::from_str::<serde_json::Value>(&stdout) {
            let disk_list = match &disks {
                serde_json::Value::Array(arr) => arr.clone(),
                obj @ serde_json::Value::Object(_) => vec![obj.clone()],
                _ => Vec::new(),
            };

            for disk in &disk_list {
                let number = disk["Number"].as_u64().unwrap_or(0);
                let name = disk["FriendlyName"]
                    .as_str()
                    .unwrap_or("Unknown")
                    .to_string();
                let serial = disk["SerialNumber"].as_str().map(|s| s.trim().to_string());
                let size = disk["Size"].as_u64().unwrap_or(0);
                let bus_type = disk["BusType"].as_str().unwrap_or("Unknown");
                let is_system = disk["IsSystem"].as_bool().unwrap_or(false);
                let is_read_only = disk["IsReadOnly"].as_bool().unwrap_or(false);

                let device_type = match bus_type {
                    "USB" => DeviceType::Usb,
                    "NVMe" => DeviceType::Nvme,
                    "SATA" => DeviceType::Sata,
                    "SCSI" | "SAS" => DeviceType::Scsi,
                    "SD" | "MMC" => DeviceType::Sd,
                    "Virtual" => DeviceType::Virtual,
                    _ => DeviceType::Unknown,
                };

                let removable = matches!(device_type, DeviceType::Usb | DeviceType::Sd);

                devices.push(DeviceInfo {
                    path: format!(r"\\.\PhysicalDrive{}", number),
                    name,
                    vendor: String::new(),
                    serial,
                    size,
                    sector_size: 512,
                    physical_sector_size: 4096,
                    removable,
                    read_only: is_read_only,
                    is_system,
                    device_type,
                    mount_points: Vec::new(),
                    transport: bus_type.to_string(),
                });
            }
        }

        Ok(devices)
    }

    async fn get_device(&self, path: &str) -> Result<DeviceInfo> {
        let devices = self.list_devices().await?;
        devices
            .into_iter()
            .find(|d| d.path == path)
            .ok_or_else(|| anyhow::anyhow!("Device not found: {}", path))
    }

    async fn unmount_device(&self, path: &str) -> Result<()> {
        // Extract disk number from path like \\.\PhysicalDrive1
        let disk_num = path
            .rsplit("PhysicalDrive")
            .next()
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| anyhow::anyhow!("Invalid device path: {}", path))?;

        let script = format!(
            "Get-Partition -DiskNumber {} | Where-Object {{ $_.DriveLetter }} | ForEach-Object {{ \
             $vol = Get-Volume -Partition $_; \
             if ($vol) {{ mountvol \"$($_.DriveLetter):\" /D }} }}",
            disk_num
        );

        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &script])
            .output()
            .await?;

        if !output.status.success() {
            log::warn!(
                "Unmount may have partially failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        Ok(())
    }

    async fn eject_device(&self, path: &str) -> Result<()> {
        self.unmount_device(path).await?;
        // Additional eject logic could use WMI or Setup API
        Ok(())
    }
}

/// Check if the current process is running with elevated (admin) privileges.
pub fn is_elevated() -> bool {
    // Use PowerShell to check — simpler than Win32 API for now
    let output = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)",
        ])
        .output();

    match output {
        Ok(o) => String::from_utf8_lossy(&o.stdout).trim() == "True",
        Err(_) => false,
    }
}
