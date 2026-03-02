// Privilege elevation — re-launch the current process with elevated privileges
//
// Many disk operations (writing block devices, formatting, erasing) require
// root/administrator privileges. This module provides cross-platform
// privilege detection and elevation:
//
//   Windows:  ShellExecuteW with "runas" verb (UAC prompt)
//   Linux:    pkexec, polkit, or sudo (with terminal)
//   macOS:    Authorization Services or osascript
//
// Design: The module checks if the current process has sufficient privileges.
// If not, it can re-launch the same process with the same arguments under
// an elevated context, then exit the un-elevated instance.
//
// Reference: Etcher child writer spawn, MediaWriter helper elevation,
//            Rufus privilege escalation logic

#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use std::env;
use std::path::PathBuf;

// ─── Types ─────────────────────────────────────────────────────────────────

/// Elevation method used or available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationMethod {
    /// Windows UAC (ShellExecute "runas").
    Uac,
    /// Linux pkexec (Polkit).
    Pkexec,
    /// Linux/macOS sudo in a terminal.
    Sudo,
    /// macOS Authorization Services / osascript.
    Osascript,
    /// Already running with elevated privileges.
    AlreadyElevated,
    /// No elevation method available.
    None,
}

impl std::fmt::Display for ElevationMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Uac => write!(f, "UAC (Windows User Account Control)"),
            Self::Pkexec => write!(f, "pkexec (Polkit)"),
            Self::Sudo => write!(f, "sudo"),
            Self::Osascript => write!(f, "osascript (macOS)"),
            Self::AlreadyElevated => write!(f, "Already elevated"),
            Self::None => write!(f, "None available"),
        }
    }
}

/// Result of an elevation attempt.
#[derive(Debug, Clone)]
pub struct ElevationResult {
    /// Whether elevation was successful.
    pub success: bool,
    /// Method used (or attempted).
    pub method: ElevationMethod,
    /// Error message (if failed).
    pub error: Option<String>,
    /// Whether the current process should exit (elevation re-launched it).
    pub should_exit: bool,
}

impl std::fmt::Display for ElevationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.success {
            write!(f, "Elevation successful via {}", self.method)?;
            if self.should_exit {
                write!(f, " — current process should exit")?;
            }
        } else {
            write!(f, "Elevation failed")?;
            if let Some(ref err) = self.error {
                write!(f, ": {}", err)?;
            }
        }
        Ok(())
    }
}

/// Elevation status report.
#[derive(Debug, Clone)]
pub struct ElevationStatus {
    /// Whether the current process is elevated.
    pub is_elevated: bool,
    /// Current user name.
    pub username: String,
    /// Available elevation methods.
    pub available_methods: Vec<ElevationMethod>,
    /// Preferred elevation method.
    pub preferred_method: ElevationMethod,
    /// Whether elevation is needed for device operations.
    pub elevation_needed: bool,
}

impl std::fmt::Display for ElevationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Elevation Status:")?;
        writeln!(f, "  Elevated:          {}", self.is_elevated)?;
        writeln!(f, "  User:              {}", self.username)?;
        writeln!(f, "  Needs elevation:   {}", self.elevation_needed)?;
        writeln!(f, "  Preferred method:  {}", self.preferred_method)?;
        write!(f, "  Available methods: ")?;
        for (i, m) in self.available_methods.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", m)?;
        }
        Ok(())
    }
}

// ─── Detection ─────────────────────────────────────────────────────────────

/// Check if the current process is running with elevated privileges.
pub fn is_elevated() -> bool {
    #[cfg(target_os = "windows")]
    {
        is_elevated_windows()
    }
    #[cfg(target_os = "linux")]
    {
        is_elevated_unix()
    }
    #[cfg(target_os = "macos")]
    {
        is_elevated_unix()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

#[cfg(target_os = "windows")]
fn is_elevated_windows() -> bool {
    // Check elevation by attempting "net session" which requires admin
    use std::process::Command;
    match Command::new("net").arg("session").output() {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn is_elevated_unix() -> bool {
    // On Unix, root has UID 0
    unsafe { libc::geteuid() == 0 }
}

/// Get the current username.
pub fn current_username() -> String {
    env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .or_else(|_| env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

/// Get the current executable path.
pub fn current_exe() -> Result<PathBuf> {
    env::current_exe().context("Failed to determine current executable path")
}

/// Get the command line arguments (excluding the program name).
pub fn current_args() -> Vec<String> {
    env::args().skip(1).collect()
}

// ─── Available Methods ─────────────────────────────────────────────────────

/// Detect available elevation methods on the current platform.
pub fn available_methods() -> Vec<ElevationMethod> {
    let mut methods = Vec::new();

    if is_elevated() {
        methods.push(ElevationMethod::AlreadyElevated);
        return methods;
    }

    #[cfg(target_os = "windows")]
    {
        methods.push(ElevationMethod::Uac);
    }

    #[cfg(target_os = "linux")]
    {
        if command_exists("pkexec") {
            methods.push(ElevationMethod::Pkexec);
        }
        if command_exists("sudo") {
            methods.push(ElevationMethod::Sudo);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if command_exists("osascript") {
            methods.push(ElevationMethod::Osascript);
        }
        if command_exists("sudo") {
            methods.push(ElevationMethod::Sudo);
        }
    }

    if methods.is_empty() {
        methods.push(ElevationMethod::None);
    }

    methods
}

/// Get the preferred elevation method.
pub fn preferred_method() -> ElevationMethod {
    let methods = available_methods();
    methods.into_iter().next().unwrap_or(ElevationMethod::None)
}

/// Get a complete elevation status report.
pub fn status() -> ElevationStatus {
    let elevated = is_elevated();
    let methods = available_methods();
    let preferred = methods.first().copied().unwrap_or(ElevationMethod::None);

    ElevationStatus {
        is_elevated: elevated,
        username: current_username(),
        available_methods: methods,
        preferred_method: preferred,
        elevation_needed: !elevated,
    }
}

// ─── Elevation ─────────────────────────────────────────────────────────────

/// Attempt to re-launch the current process with elevated privileges.
///
/// On success, returns `ElevationResult { should_exit: true }`,
/// meaning the caller should exit the current un-elevated process.
///
/// The elevated child process will run with the same command-line arguments.
pub fn elevate() -> Result<ElevationResult> {
    if is_elevated() {
        return Ok(ElevationResult {
            success: true,
            method: ElevationMethod::AlreadyElevated,
            error: None,
            should_exit: false,
        });
    }

    let method = preferred_method();
    match method {
        ElevationMethod::AlreadyElevated => Ok(ElevationResult {
            success: true,
            method,
            error: None,
            should_exit: false,
        }),
        #[cfg(target_os = "windows")]
        ElevationMethod::Uac => elevate_uac(),
        #[cfg(target_os = "linux")]
        ElevationMethod::Pkexec => elevate_pkexec(),
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        ElevationMethod::Sudo => elevate_sudo(),
        #[cfg(target_os = "macos")]
        ElevationMethod::Osascript => elevate_osascript(),
        ElevationMethod::None => Ok(ElevationResult {
            success: false,
            method,
            error: Some("No elevation method available on this platform".into()),
            should_exit: false,
        }),
        _ => Ok(ElevationResult {
            success: false,
            method,
            error: Some(format!("Elevation method {} not supported on this platform", method)),
            should_exit: false,
        }),
    }
}

/// Attempt elevation with a specific method.
pub fn elevate_with(method: ElevationMethod) -> Result<ElevationResult> {
    match method {
        ElevationMethod::AlreadyElevated => Ok(ElevationResult {
            success: is_elevated(),
            method,
            error: if is_elevated() {
                None
            } else {
                Some("Not actually elevated".into())
            },
            should_exit: false,
        }),
        #[cfg(target_os = "windows")]
        ElevationMethod::Uac => elevate_uac(),
        #[cfg(target_os = "linux")]
        ElevationMethod::Pkexec => elevate_pkexec(),
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        ElevationMethod::Sudo => elevate_sudo(),
        #[cfg(target_os = "macos")]
        ElevationMethod::Osascript => elevate_osascript(),
        _ => Ok(ElevationResult {
            success: false,
            method,
            error: Some(format!("{} not available", method)),
            should_exit: false,
        }),
    }
}

// ─── Platform Implementations ──────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn elevate_uac() -> Result<ElevationResult> {
    use std::process::Command;

    let exe = current_exe()?;
    let args = current_args().join(" ");

    // Use PowerShell Start-Process -Verb RunAs for UAC elevation
    let status = Command::new("powershell")
        .args(&[
            "-NoProfile",
            "-Command",
            &format!(
                "Start-Process -FilePath '{}' -ArgumentList '{}' -Verb RunAs -Wait",
                exe.display(),
                args.replace("'", "''")
            ),
        ])
        .status()
        .context("Failed to launch UAC elevation via PowerShell")?;

    Ok(ElevationResult {
        success: status.success(),
        method: ElevationMethod::Uac,
        error: if status.success() {
            None
        } else {
            Some(format!("UAC elevation failed with exit code {:?}", status.code()))
        },
        should_exit: status.success(),
    })
}

#[cfg(target_os = "linux")]
fn elevate_pkexec() -> Result<ElevationResult> {
    use std::process::Command;

    let exe = current_exe()?;
    let args = current_args();

    let status = Command::new("pkexec")
        .arg(&exe)
        .args(&args)
        .status()
        .context("Failed to launch pkexec elevation")?;

    Ok(ElevationResult {
        success: status.success(),
        method: ElevationMethod::Pkexec,
        error: if status.success() {
            None
        } else {
            Some(format!("pkexec failed with exit code {:?}", status.code()))
        },
        should_exit: status.success(),
    })
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn elevate_sudo() -> Result<ElevationResult> {
    use std::process::Command;

    let exe = current_exe()?;
    let args = current_args();

    let status = Command::new("sudo")
        .arg("--")
        .arg(&exe)
        .args(&args)
        .status()
        .context("Failed to launch sudo elevation")?;

    Ok(ElevationResult {
        success: status.success(),
        method: ElevationMethod::Sudo,
        error: if status.success() {
            None
        } else {
            Some(format!("sudo failed with exit code {:?}", status.code()))
        },
        should_exit: status.success(),
    })
}

#[cfg(target_os = "macos")]
fn elevate_osascript() -> Result<ElevationResult> {
    use std::process::Command;

    let exe = current_exe()?;
    let args = current_args();
    let full_cmd = format!(
        "{} {}",
        exe.display(),
        args.iter()
            .map(|a| shell_escape(a))
            .collect::<Vec<_>>()
            .join(" ")
    );

    let status = Command::new("osascript")
        .args(&[
            "-e",
            &format!(
                "do shell script \"{}\" with administrator privileges",
                full_cmd.replace("\\", "\\\\").replace("\"", "\\\"")
            ),
        ])
        .status()
        .context("Failed to launch osascript elevation")?;

    Ok(ElevationResult {
        success: status.success(),
        method: ElevationMethod::Osascript,
        error: if status.success() {
            None
        } else {
            Some(format!("osascript failed with exit code {:?}", status.code()))
        },
        should_exit: status.success(),
    })
}

// ─── Helpers ───────────────────────────────────────────────────────────────

/// Check if a command exists in PATH.
fn command_exists(cmd: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("where")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// Escape a string for shell use.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn shell_escape(s: &str) -> String {
    if s.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"' || c == '\\') {
        format!("'{}'", s.replace("'", "'\\''"))
    } else {
        s.to_string()
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elevation_method_display() {
        assert!(format!("{}", ElevationMethod::Uac).contains("UAC"));
        assert!(format!("{}", ElevationMethod::Pkexec).contains("pkexec"));
        assert!(format!("{}", ElevationMethod::Sudo).contains("sudo"));
        assert!(format!("{}", ElevationMethod::Osascript).contains("osascript"));
        assert!(format!("{}", ElevationMethod::AlreadyElevated).contains("Already"));
        assert!(format!("{}", ElevationMethod::None).contains("None"));
    }

    #[test]
    fn test_is_elevated() {
        // Just check that it returns a boolean without panicking
        let elevated = is_elevated();
        // In CI, typically not elevated; in dev, might be
        let _ = elevated;
    }

    #[test]
    fn test_current_username() {
        let username = current_username();
        assert!(!username.is_empty());
        // Should return something other than "unknown" on most systems
    }

    #[test]
    fn test_current_exe() {
        let exe = current_exe();
        assert!(exe.is_ok());
        let path = exe.unwrap();
        assert!(path.exists() || path.to_string_lossy().contains("test"));
    }

    #[test]
    fn test_available_methods() {
        let methods = available_methods();
        assert!(!methods.is_empty());
        // Should have at least one method or AlreadyElevated
    }

    #[test]
    fn test_preferred_method() {
        let method = preferred_method();
        // Should return something valid
        let _ = format!("{}", method); // shouldn't panic
    }

    #[test]
    fn test_status_report() {
        let s = status();
        let display = format!("{}", s);
        assert!(display.contains("Elevation Status"));
        assert!(display.contains("Elevated"));
        assert!(display.contains("User"));
    }

    #[test]
    fn test_elevation_result_display_success() {
        let result = ElevationResult {
            success: true,
            method: ElevationMethod::AlreadyElevated,
            error: None,
            should_exit: false,
        };
        let display = format!("{}", result);
        assert!(display.contains("successful"));
        assert!(display.contains("Already"));
    }

    #[test]
    fn test_elevation_result_display_failure() {
        let result = ElevationResult {
            success: false,
            method: ElevationMethod::None,
            error: Some("No method available".into()),
            should_exit: false,
        };
        let display = format!("{}", result);
        assert!(display.contains("failed"));
        assert!(display.contains("No method"));
    }

    #[test]
    fn test_elevate_already_elevated() {
        // If we're already elevated, this should succeed immediately
        if is_elevated() {
            let result = elevate().unwrap();
            assert!(result.success);
            assert_eq!(result.method, ElevationMethod::AlreadyElevated);
            assert!(!result.should_exit);
        }
    }

    #[test]
    fn test_elevate_with_already_elevated() {
        let result = elevate_with(ElevationMethod::AlreadyElevated).unwrap();
        if is_elevated() {
            assert!(result.success);
        } else {
            assert!(!result.success);
        }
    }

    #[test]
    fn test_elevation_status_fields() {
        let s = status();
        let _ = s.is_elevated;
        assert!(!s.username.is_empty() || s.username == "unknown");
        assert!(!s.available_methods.is_empty());
    }

    #[test]
    fn test_command_exists() {
        // Common commands that should exist on test systems
        #[cfg(target_os = "windows")]
        {
            assert!(command_exists("cmd"));
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert!(command_exists("ls"));
        }
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn test_shell_escape() {
        assert_eq!(shell_escape("simple"), "simple");
        assert_eq!(shell_escape("has space"), "'has space'");
        assert_eq!(shell_escape("has'quote"), "'has'\\''quote'");
    }
}
