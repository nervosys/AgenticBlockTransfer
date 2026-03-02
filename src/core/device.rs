use serde::{Deserialize, Serialize};
use std::fmt;

use super::types::DeviceType;

/// Information about a block device / storage target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Platform device path (e.g., /dev/sdb, \\.\PhysicalDrive1, /dev/disk2)
    pub path: String,
    /// Human-readable device name/model
    pub name: String,
    /// Vendor/manufacturer
    pub vendor: String,
    /// Device serial number
    pub serial: Option<String>,
    /// Total size in bytes
    pub size: u64,
    /// Logical sector size
    pub sector_size: u32,
    /// Physical sector size
    pub physical_sector_size: u32,
    /// Whether the device is removable
    pub removable: bool,
    /// Whether the device is read-only
    pub read_only: bool,
    /// Whether this is a system/boot drive
    pub is_system: bool,
    /// Device type classification
    pub device_type: DeviceType,
    /// Mount points (if any partitions are mounted)
    pub mount_points: Vec<String>,
    /// Transport/bus type description
    pub transport: String,
}

impl fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size = humansize::format_size(self.size, humansize::BINARY);
        write!(
            f,
            "{} - {} {} ({}) [{}]",
            self.path, self.vendor, self.name, size, self.device_type
        )
    }
}

impl DeviceInfo {
    /// Safety check: is this device safe to write to?
    pub fn is_safe_target(&self) -> bool {
        !self.is_system && !self.read_only
    }

    /// Is this a removable device (USB, SD, etc.)?
    pub fn is_removable_media(&self) -> bool {
        self.removable
            || matches!(
                self.device_type,
                DeviceType::Usb | DeviceType::Sd | DeviceType::Mmc
            )
    }
}

/// Trait for platform-specific device enumeration.
#[async_trait::async_trait]
pub trait DeviceEnumerator: Send + Sync {
    /// List all block devices on the system.
    async fn list_devices(&self) -> anyhow::Result<Vec<DeviceInfo>>;

    /// Get info for a specific device path.
    async fn get_device(&self, path: &str) -> anyhow::Result<DeviceInfo>;

    /// Unmount all partitions on a device.
    async fn unmount_device(&self, path: &str) -> anyhow::Result<()>;

    /// Eject / safely remove a device.
    #[allow(dead_code)]
    async fn eject_device(&self, path: &str) -> anyhow::Result<()>;
}

/// Create the platform-appropriate device enumerator.
pub fn create_enumerator() -> Box<dyn DeviceEnumerator> {
    crate::platform::create_device_enumerator()
}
