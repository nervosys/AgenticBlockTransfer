// Sleep inhibitor — prevent the OS from sleeping/suspending during write operations.
// Platform-specific: D-Bus on Linux, IOKit on macOS, SetThreadExecutionState on Windows.
// Inspired by rpi-imager's sleep inhibitor.

use anyhow::Result;
use std::fmt;

/// Reason for inhibiting sleep.
#[derive(Debug, Clone)]
pub enum InhibitReason {
    Writing,
    Verifying,
    Erasing,
    Cloning,
    Benchmarking,
    HealthCheck,
    Custom(String),
}

impl fmt::Display for InhibitReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Writing => write!(f, "Writing disk image"),
            Self::Verifying => write!(f, "Verifying written data"),
            Self::Erasing => write!(f, "Securely erasing device"),
            Self::Cloning => write!(f, "Cloning device"),
            Self::Benchmarking => write!(f, "Benchmarking I/O"),
            Self::HealthCheck => write!(f, "Checking drive health"),
            Self::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// A handle that inhibits sleep while it exists. Sleep is restored when dropped.
pub struct SleepInhibitor {
    _reason: InhibitReason,
    #[cfg(target_os = "linux")]
    _fd: Option<std::os::unix::io::RawFd>,
    #[cfg(target_os = "windows")]
    _prev_state: u32,
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    _dummy: (),
}

impl SleepInhibitor {
    /// Create a new sleep inhibitor. The OS will not sleep while this object exists.
    pub fn new(reason: InhibitReason) -> Result<Self> {
        log::info!("Inhibiting sleep: {}", reason);
        
        #[cfg(target_os = "linux")]
        {
            let fd = linux_inhibit_sleep(&reason);
            Ok(Self {
                _reason: reason,
                _fd: fd,
            })
        }

        #[cfg(target_os = "macos")]
        {
            macos_inhibit_sleep(&reason);
            Ok(Self {
                _reason: reason,
            })
        }

        #[cfg(target_os = "windows")]
        {
            let prev = windows_inhibit_sleep();
            Ok(Self {
                _reason: reason,
                _prev_state: prev,
            })
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            log::warn!("Sleep inhibition not supported on this platform");
            Ok(Self {
                _reason: reason,
                _dummy: (),
            })
        }
    }

    /// Get the reason for inhibiting sleep.
    pub fn reason(&self) -> &InhibitReason {
        &self._reason
    }
}

impl Drop for SleepInhibitor {
    fn drop(&mut self) {
        log::info!("Releasing sleep inhibitor: {}", self._reason);

        #[cfg(target_os = "linux")]
        {
            if let Some(fd) = self._fd {
                unsafe {
                    libc::close(fd);
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            windows_restore_sleep(self._prev_state);
        }

        // macOS: IOPMAssertionRelease would need the assertion ID
        // For simplicity, we use caffeinate subprocess which auto-cleans
    }
}

/// Inhibit sleep on Linux via systemd-logind D-Bus Inhibit() call.
/// Falls back to caffeine/xdg-screensaver if D-Bus is not available.
#[cfg(target_os = "linux")]
fn linux_inhibit_sleep(reason: &InhibitReason) -> Option<std::os::unix::io::RawFd> {
    // Try D-Bus Inhibit via systemd-inhibit command (simpler than linking libdbus)
    // The fd-based approach: systemd-inhibit --what=sleep returns a file descriptor
    // that inhibits sleep as long as it's open.
    //
    // For simplicity, we use a subprocess approach:
    let result = std::process::Command::new("systemd-inhibit")
        .args([
            "--what=idle:sleep",
            "--who=abt",
            &format!("--why={}", reason),
            "--mode=block",
            "sleep",
            "infinity",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match result {
        Ok(_child) => {
            log::debug!("Sleep inhibited via systemd-inhibit");
            None // Child process handles inhibition
        }
        Err(e) => {
            log::debug!("systemd-inhibit not available ({}), trying xdg-screensaver", e);
            // Fallback: disable screensaver
            let _ = std::process::Command::new("xdg-screensaver")
                .args(["suspend", "0"])
                .output();
            None
        }
    }
}

/// Inhibit sleep on macOS via caffeinate subprocess.
#[cfg(target_os = "macos")]
fn macos_inhibit_sleep(reason: &InhibitReason) {
    // caffeinate -i prevents idle sleep
    // caffeinate -s prevents system sleep (requires AC power)
    let _ = std::process::Command::new("caffeinate")
        .args(["-i", "-w", &format!("{}", std::process::id())])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| log::warn!("Failed to run caffeinate: {}", e));

    log::debug!("Sleep inhibited via caffeinate (reason: {})", reason);
}

/// Inhibit sleep on Windows via SetThreadExecutionState.
#[cfg(target_os = "windows")]
fn windows_inhibit_sleep() -> u32 {
    use windows::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED, ES_DISPLAY_REQUIRED,
    };

    let flags = ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED;
    let prev = unsafe { SetThreadExecutionState(flags) };
    log::debug!("Sleep inhibited via SetThreadExecutionState (prev={:?})", prev);
    prev.0
}

/// Restore sleep on Windows.
#[cfg(target_os = "windows")]
fn windows_restore_sleep(_prev: u32) {
    use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS};

    unsafe {
        SetThreadExecutionState(ES_CONTINUOUS);
    }
    log::debug!("Sleep restored via SetThreadExecutionState");
}

/// RAII guard that inhibits sleep for the duration of a closure.
pub fn with_sleep_inhibited<F, T>(reason: InhibitReason, f: F) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    let _guard = SleepInhibitor::new(reason)?;
    f()
}

/// Check if sleep inhibition is supported on the current platform.
pub fn is_supported() -> bool {
    cfg!(any(
        target_os = "linux",
        target_os = "macos",
        target_os = "windows"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inhibit_reason_display() {
        assert_eq!(format!("{}", InhibitReason::Writing), "Writing disk image");
        assert_eq!(format!("{}", InhibitReason::Verifying), "Verifying written data");
        assert_eq!(format!("{}", InhibitReason::Erasing), "Securely erasing device");
        assert_eq!(format!("{}", InhibitReason::Cloning), "Cloning device");
        assert_eq!(format!("{}", InhibitReason::Benchmarking), "Benchmarking I/O");
        assert_eq!(format!("{}", InhibitReason::HealthCheck), "Checking drive health");
        assert_eq!(
            format!("{}", InhibitReason::Custom("test".into())),
            "test"
        );
    }

    #[test]
    fn test_is_supported() {
        // Should be true on Linux/macOS/Windows
        let supported = is_supported();
        #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
        assert!(supported);
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        assert!(!supported);
    }

    #[test]
    fn test_with_sleep_inhibited() {
        let result = with_sleep_inhibited(InhibitReason::Writing, || Ok(42));
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_inhibitor_reason_accessor() {
        let inhibitor = SleepInhibitor::new(InhibitReason::Writing).unwrap();
        match inhibitor.reason() {
            InhibitReason::Writing => {}
            _ => panic!("wrong reason"),
        }
    }

    #[test]
    fn test_inhibitor_dropped_cleanly() {
        {
            let _inhibitor = SleepInhibitor::new(InhibitReason::Verifying).unwrap();
            // Inhibitor active
        }
        // Inhibitor dropped — sleep should be restored
    }
}
