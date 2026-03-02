use anyhow::Result;

use crate::core::device::{DeviceEnumerator, DeviceInfo};

/// Stub enumerator for unsupported platforms.
pub struct StubDeviceEnumerator;

#[async_trait::async_trait]
impl DeviceEnumerator for StubDeviceEnumerator {
    async fn list_devices(&self) -> Result<Vec<DeviceInfo>> {
        log::warn!("Device enumeration not supported on this platform");
        Ok(Vec::new())
    }

    async fn get_device(&self, _path: &str) -> Result<DeviceInfo> {
        anyhow::bail!("Device enumeration not supported on this platform")
    }

    async fn unmount_device(&self, _path: &str) -> Result<()> {
        Ok(())
    }

    async fn eject_device(&self, _path: &str) -> Result<()> {
        Ok(())
    }
}
