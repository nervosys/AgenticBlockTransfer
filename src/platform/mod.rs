// Platform-specific implementations

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "freebsd")]
pub mod freebsd;

// Stub for unsupported platforms
#[cfg(not(any(
    target_os = "linux",
    target_os = "macos",
    target_os = "windows",
    target_os = "freebsd"
)))]
pub mod stub;

use crate::core::device::DeviceEnumerator;

pub fn create_device_enumerator() -> Box<dyn DeviceEnumerator> {
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxDeviceEnumerator::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacDeviceEnumerator::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsDeviceEnumerator::new())
    }
    #[cfg(target_os = "freebsd")]
    {
        Box::new(freebsd::FreeBsdDeviceEnumerator::new())
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows",
        target_os = "freebsd"
    )))]
    {
        Box::new(stub::StubDeviceEnumerator)
    }
}

/// Check if the current process has elevated/root privileges.
pub fn is_elevated() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(windows)]
    {
        // Check if running as admin on Windows
        windows::is_elevated()
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}
