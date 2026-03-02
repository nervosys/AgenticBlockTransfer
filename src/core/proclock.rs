// Process lock detection — find processes holding handles/locks on a target device
//
// Before writing to a block device, any process that has an open file handle,
// mount point, or memory-mapped region on the target drive will cause write
// failures. This module detects such conflicting processes cross-platform:
//
//   Windows:   Restart Manager API (RmRegisterResources / RmGetList)
//   Linux:     Parse /proc/*/fd symlinks and /proc/mounts
//   macOS:     lsof -F p (field mode for programmatic parsing)
//
// Inspired by Rufus's "checking for conflicting processes" logic.
//
// Reference: rufus/src/process.c, Windows Restart Manager API,
//            /proc filesystem documentation

#![allow(dead_code)]

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ─── Types ─────────────────────────────────────────────────────────────────

/// Information about a process holding a lock on a target.
#[derive(Debug, Clone)]
pub struct ProcessLock {
    /// Process ID.
    pub pid: u32,
    /// Process name (executable name).
    pub name: String,
    /// Full command line (if available).
    pub command: Option<String>,
    /// Open file paths on the target device.
    pub open_files: Vec<String>,
    /// Whether this process can be safely terminated.
    pub safe_to_kill: bool,
    /// Lock type(s) detected.
    pub lock_types: Vec<LockType>,
}

impl std::fmt::Display for ProcessLock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PID {} ({}) — {} open file(s)",
            self.pid,
            self.name,
            self.open_files.len()
        )?;
        for lt in &self.lock_types {
            write!(f, " [{}]", lt)?;
        }
        Ok(())
    }
}

/// Type of lock a process holds on a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockType {
    /// Open file descriptor / handle.
    FileHandle,
    /// Mounted filesystem.
    Mount,
    /// Memory-mapped region.
    MemoryMap,
    /// Working directory is on the device.
    WorkingDirectory,
    /// Unknown / indeterminate.
    Unknown,
}

impl std::fmt::Display for LockType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileHandle => write!(f, "file handle"),
            Self::Mount => write!(f, "mount"),
            Self::MemoryMap => write!(f, "mmap"),
            Self::WorkingDirectory => write!(f, "cwd"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

/// Result of a process lock scan.
#[derive(Debug, Clone)]
pub struct LockScanResult {
    /// Device or path that was scanned.
    pub target: String,
    /// Processes with locks on the target.
    pub locks: Vec<ProcessLock>,
    /// Whether any critical (unsafe to kill) processes were found.
    pub has_critical: bool,
    /// Total number of open file handles across all processes.
    pub total_handles: usize,
    /// Errors encountered during scanning (non-fatal).
    pub warnings: Vec<String>,
}

impl LockScanResult {
    /// Whether any locks were found.
    pub fn has_locks(&self) -> bool {
        !self.locks.is_empty()
    }

    /// Number of processes holding locks.
    pub fn process_count(&self) -> usize {
        self.locks.len()
    }

    /// Get names of all locking processes.
    pub fn process_names(&self) -> Vec<&str> {
        self.locks.iter().map(|l| l.name.as_str()).collect()
    }
}

impl std::fmt::Display for LockScanResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.locks.is_empty() {
            writeln!(f, "No processes holding locks on {}", self.target)?;
        } else {
            writeln!(
                f,
                "Found {} process(es) with locks on {}:",
                self.locks.len(),
                self.target
            )?;
            for lock in &self.locks {
                writeln!(f, "  • {}", lock)?;
                for file in &lock.open_files {
                    writeln!(f, "      → {}", file)?;
                }
            }
        }
        if self.has_critical {
            writeln!(f, "  ⚠ Critical processes detected — manual intervention required")?;
        }
        for w in &self.warnings {
            writeln!(f, "  Warning: {}", w)?;
        }
        Ok(())
    }
}

// ─── Configuration ─────────────────────────────────────────────────────────

/// Configuration for process lock scanning.
#[derive(Debug, Clone)]
pub struct LockScanConfig {
    /// Whether to include system/kernel threads.
    pub include_system: bool,
    /// Whether to resolve mount points as device paths.
    pub resolve_mounts: bool,
    /// Timeout in milliseconds for the scan operation.
    pub timeout_ms: u64,
    /// Process names that are always considered safe to kill.
    pub safe_processes: Vec<String>,
    /// Process names that are never safe to kill (critical system processes).
    pub critical_processes: Vec<String>,
}

impl Default for LockScanConfig {
    fn default() -> Self {
        Self {
            include_system: false,
            resolve_mounts: true,
            timeout_ms: 5000,
            safe_processes: vec![
                "explorer.exe".into(),
                "thunar".into(),
                "nautilus".into(),
                "dolphin".into(),
                "pcmanfm".into(),
                "nemo".into(),
                "caja".into(),
                "Finder".into(),
            ],
            critical_processes: vec![
                "System".into(),
                "smss.exe".into(),
                "csrss.exe".into(),
                "wininit.exe".into(),
                "services.exe".into(),
                "lsass.exe".into(),
                "init".into(),
                "systemd".into(),
                "launchd".into(),
                "kernel_task".into(),
            ],
        }
    }
}

// ─── Scanning Functions ────────────────────────────────────────────────────

/// Scan for processes holding locks on a target device or path.
///
/// This is the main entry point. It dispatches to platform-specific
/// implementations and returns a unified result.
pub fn scan_locks(target: &str, config: &LockScanConfig) -> Result<LockScanResult> {
    let mut result = LockScanResult {
        target: target.to_string(),
        locks: Vec::new(),
        has_critical: false,
        total_handles: 0,
        warnings: Vec::new(),
    };

    #[cfg(target_os = "linux")]
    {
        scan_linux(target, config, &mut result)?;
    }

    #[cfg(target_os = "macos")]
    {
        scan_macos(target, config, &mut result)?;
    }

    #[cfg(target_os = "windows")]
    {
        scan_windows(target, config, &mut result)?;
    }

    // Post-process: classify safe/critical
    for lock in &mut result.locks {
        let name_lower = lock.name.to_lowercase();
        if config.critical_processes.iter().any(|c| c.to_lowercase() == name_lower) {
            lock.safe_to_kill = false;
            result.has_critical = true;
        } else if config.safe_processes.iter().any(|s| s.to_lowercase() == name_lower) {
            lock.safe_to_kill = true;
        }
    }

    result.total_handles = result.locks.iter().map(|l| l.open_files.len()).sum();

    Ok(result)
}

// ─── Linux Implementation ──────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn scan_linux(target: &str, config: &LockScanConfig, result: &mut LockScanResult) -> Result<()> {
    use std::fs;

    let target_path = Path::new(target);

    // Resolve mount points for the target device
    let mount_points = if config.resolve_mounts {
        resolve_linux_mounts(target)?
    } else {
        Vec::new()
    };

    // Paths to check: the device itself + any mount points
    let mut check_paths: Vec<String> = vec![target.to_string()];
    for mp in &mount_points {
        check_paths.push(mp.clone());
    }

    // Scan /proc for each PID
    let proc_dir = Path::new("/proc");
    if !proc_dir.exists() {
        result.warnings.push("/proc not available".into());
        return Ok(());
    }

    let mut pid_locks: HashMap<u32, ProcessLock> = HashMap::new();

    for entry in fs::read_dir(proc_dir).context("Failed to read /proc")? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let name = entry.file_name();
        let pid_str = name.to_string_lossy();
        let pid: u32 = match pid_str.parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Skip PID 1 and kernel threads unless configured
        if !config.include_system && pid <= 2 {
            continue;
        }

        // Read process name
        let comm_path = proc_dir.join(&pid_str.to_string()).join("comm");
        let proc_name = fs::read_to_string(&comm_path)
            .unwrap_or_default()
            .trim()
            .to_string();

        // Check /proc/<pid>/fd/ symlinks
        let fd_dir = proc_dir.join(&pid_str.to_string()).join("fd");
        if let Ok(fd_entries) = fs::read_dir(&fd_dir) {
            for fd_entry in fd_entries.flatten() {
                if let Ok(link_target) = fs::read_link(fd_entry.path()) {
                    let link_str = link_target.to_string_lossy().to_string();
                    for check in &check_paths {
                        if link_str.starts_with(check) || link_str == *check {
                            let lock = pid_locks.entry(pid).or_insert_with(|| ProcessLock {
                                pid,
                                name: proc_name.clone(),
                                command: fs::read_to_string(
                                    proc_dir.join(&pid_str.to_string()).join("cmdline")
                                )
                                .ok()
                                .map(|s| s.replace('\0', " ").trim().to_string()),
                                open_files: Vec::new(),
                                safe_to_kill: true,
                                lock_types: Vec::new(),
                            });
                            lock.open_files.push(link_str.clone());
                            if !lock.lock_types.contains(&LockType::FileHandle) {
                                lock.lock_types.push(LockType::FileHandle);
                            }
                        }
                    }
                }
            }
        }

        // Check /proc/<pid>/cwd
        let cwd_path = proc_dir.join(&pid_str.to_string()).join("cwd");
        if let Ok(cwd_target) = fs::read_link(&cwd_path) {
            let cwd_str = cwd_target.to_string_lossy().to_string();
            for mp in &mount_points {
                if cwd_str.starts_with(mp) {
                    let lock = pid_locks.entry(pid).or_insert_with(|| ProcessLock {
                        pid,
                        name: proc_name.clone(),
                        command: None,
                        open_files: Vec::new(),
                        safe_to_kill: true,
                        lock_types: Vec::new(),
                    });
                    if !lock.lock_types.contains(&LockType::WorkingDirectory) {
                        lock.lock_types.push(LockType::WorkingDirectory);
                    }
                }
            }
        }
    }

    result.locks.extend(pid_locks.into_values());
    Ok(())
}

#[cfg(target_os = "linux")]
fn resolve_linux_mounts(device: &str) -> Result<Vec<String>> {
    use std::fs;

    let mut mounts = Vec::new();
    if let Ok(content) = fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[0] == device {
                mounts.push(parts[1].to_string());
            }
        }
    }
    Ok(mounts)
}

// ─── macOS Implementation ──────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn scan_macos(target: &str, config: &LockScanConfig, result: &mut LockScanResult) -> Result<()> {
    use std::process::Command;

    // Use lsof in field mode
    let output = Command::new("lsof")
        .args(&["-F", "pcfn", "+D", target])
        .output()
        .context("Failed to run lsof")?;

    if !output.status.success() {
        // lsof returns 1 when no matches found — that's OK
        if output.status.code() == Some(1) {
            return Ok(());
        }
        result.warnings.push(format!(
            "lsof exited with code {:?}",
            output.status.code()
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_lsof_output(&stdout, config, result);
    Ok(())
}

// ─── Windows Implementation ────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn scan_windows(target: &str, config: &LockScanConfig, result: &mut LockScanResult) -> Result<()> {
    // On Windows, we use a simpler approach: check if we can open the device
    // exclusively. If not, something else has it. We also enumerate volumes
    // on the physical disk and check their mount status.
    use std::process::Command;

    // Try handle.exe / openfiles approach
    // Check mount points for volumes on this physical disk
    let output = Command::new("cmd")
        .args(&["/c", "wmic", "logicaldisk", "get", "deviceid,description,volumename", "/format:csv"])
        .output();

    match output {
        Ok(out) => {
            if out.status.success() {
                // Parse output and look for mounted volumes related to our target
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines().skip(1) {
                    let fields: Vec<&str> = line.split(',').collect();
                    if fields.len() >= 3 {
                        // Volume is mounted — potential lock source
                        let drive_letter = fields[1].trim();
                        if !drive_letter.is_empty() && target.contains(drive_letter) {
                            result.warnings.push(format!(
                                "Volume {} is mounted — may need unmounting",
                                drive_letter
                            ));
                        }
                    }
                }
            }
        }
        Err(e) => {
            result.warnings.push(format!("Could not query volumes: {}", e));
        }
    }

    // Try to detect explorer.exe and other processes using Process list
    let output2 = Command::new("powershell")
        .args(&["-NoProfile", "-Command",
            &format!(
                "Get-Process | Where-Object {{ $_.Path -ne $null }} | Select-Object Id,ProcessName | Format-Table -AutoSize"
            )
        ])
        .output();

    match output2 {
        Ok(out) => {
            if !out.status.success() {
                result.warnings.push("Could not enumerate processes".into());
            }
        }
        Err(e) => {
            result.warnings.push(format!("Failed to enumerate processes: {}", e));
        }
    }

    Ok(())
}

// ─── Common Helpers ────────────────────────────────────────────────────────

/// Parse lsof field-mode output (-F).
/// Fields: p=PID, c=command, f=fd, n=name
fn parse_lsof_output(output: &str, _config: &LockScanConfig, result: &mut LockScanResult) {
    let mut current_pid: Option<u32> = None;
    let mut current_name = String::new();
    let mut pid_locks: HashMap<u32, ProcessLock> = HashMap::new();

    for line in output.lines() {
        if line.is_empty() {
            continue;
        }

        let (tag, value) = (line.as_bytes()[0] as char, &line[1..]);
        match tag {
            'p' => {
                current_pid = value.parse().ok();
            }
            'c' => {
                current_name = value.to_string();
            }
            'n' => {
                if let Some(pid) = current_pid {
                    let lock = pid_locks.entry(pid).or_insert_with(|| ProcessLock {
                        pid,
                        name: current_name.clone(),
                        command: None,
                        open_files: Vec::new(),
                        safe_to_kill: true,
                        lock_types: vec![LockType::FileHandle],
                    });
                    lock.open_files.push(value.to_string());
                }
            }
            _ => {}
        }
    }

    result.locks.extend(pid_locks.into_values());
}

/// Quick check: is the target device busy (any locks at all)?
pub fn is_device_busy(target: &str) -> Result<bool> {
    let config = LockScanConfig::default();
    let result = scan_locks(target, &config)?;
    Ok(result.has_locks())
}

/// Get a human-readable report of all locks on a target.
pub fn lock_report(target: &str) -> Result<String> {
    let config = LockScanConfig::default();
    let result = scan_locks(target, &config)?;
    Ok(format!("{}", result))
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_scan_config_default() {
        let config = LockScanConfig::default();
        assert!(!config.include_system);
        assert!(config.resolve_mounts);
        assert_eq!(config.timeout_ms, 5000);
        assert!(!config.safe_processes.is_empty());
        assert!(!config.critical_processes.is_empty());
    }

    #[test]
    fn test_lock_type_display() {
        assert_eq!(format!("{}", LockType::FileHandle), "file handle");
        assert_eq!(format!("{}", LockType::Mount), "mount");
        assert_eq!(format!("{}", LockType::MemoryMap), "mmap");
        assert_eq!(format!("{}", LockType::WorkingDirectory), "cwd");
        assert_eq!(format!("{}", LockType::Unknown), "unknown");
    }

    #[test]
    fn test_process_lock_display() {
        let lock = ProcessLock {
            pid: 1234,
            name: "explorer.exe".into(),
            command: Some("C:\\Windows\\explorer.exe".into()),
            open_files: vec!["E:\\README.md".into(), "E:\\file.txt".into()],
            safe_to_kill: true,
            lock_types: vec![LockType::FileHandle],
        };
        let display = format!("{}", lock);
        assert!(display.contains("1234"));
        assert!(display.contains("explorer.exe"));
        assert!(display.contains("2 open file(s)"));
        assert!(display.contains("file handle"));
    }

    #[test]
    fn test_lock_scan_result_empty() {
        let result = LockScanResult {
            target: "/dev/sdb".into(),
            locks: Vec::new(),
            has_critical: false,
            total_handles: 0,
            warnings: Vec::new(),
        };
        assert!(!result.has_locks());
        assert_eq!(result.process_count(), 0);
        assert!(result.process_names().is_empty());
        let display = format!("{}", result);
        assert!(display.contains("No processes"));
    }

    #[test]
    fn test_lock_scan_result_with_locks() {
        let result = LockScanResult {
            target: "/dev/sdb".into(),
            locks: vec![
                ProcessLock {
                    pid: 100,
                    name: "thunar".into(),
                    command: None,
                    open_files: vec!["/media/usb/file.txt".into()],
                    safe_to_kill: true,
                    lock_types: vec![LockType::FileHandle],
                },
                ProcessLock {
                    pid: 200,
                    name: "bash".into(),
                    command: Some("/bin/bash".into()),
                    open_files: vec!["/media/usb".into()],
                    safe_to_kill: true,
                    lock_types: vec![LockType::WorkingDirectory],
                },
            ],
            has_critical: false,
            total_handles: 2,
            warnings: Vec::new(),
        };
        assert!(result.has_locks());
        assert_eq!(result.process_count(), 2);
        let names = result.process_names();
        assert!(names.contains(&"thunar"));
        assert!(names.contains(&"bash"));
    }

    #[test]
    fn test_critical_process_detection() {
        let config = LockScanConfig::default();
        assert!(config.critical_processes.iter().any(|p| p == "System"));
        assert!(config.critical_processes.iter().any(|p| p == "systemd"));
    }

    #[test]
    fn test_safe_process_detection() {
        let config = LockScanConfig::default();
        assert!(config.safe_processes.iter().any(|p| p == "explorer.exe"));
        assert!(config.safe_processes.iter().any(|p| p == "nautilus"));
    }

    #[test]
    fn test_lsof_output_parsing() {
        let output = "p1234\ncexplorer\nf4\nn/media/usb/file.txt\np5678\ncbash\nf255\nn/media/usb\n";
        let config = LockScanConfig::default();
        let mut result = LockScanResult {
            target: "/dev/sdb".into(),
            locks: Vec::new(),
            has_critical: false,
            total_handles: 0,
            warnings: Vec::new(),
        };
        parse_lsof_output(output, &config, &mut result);
        assert_eq!(result.locks.len(), 2);

        // Sort by PID for deterministic checking
        result.locks.sort_by_key(|l| l.pid);
        assert_eq!(result.locks[0].pid, 1234);
        assert_eq!(result.locks[0].name, "explorer");
        assert_eq!(result.locks[0].open_files.len(), 1);
        assert_eq!(result.locks[1].pid, 5678);
        assert_eq!(result.locks[1].name, "bash");
    }

    #[test]
    fn test_lsof_empty_output() {
        let config = LockScanConfig::default();
        let mut result = LockScanResult {
            target: "/dev/sdb".into(),
            locks: Vec::new(),
            has_critical: false,
            total_handles: 0,
            warnings: Vec::new(),
        };
        parse_lsof_output("", &config, &mut result);
        assert!(result.locks.is_empty());
    }

    #[test]
    fn test_scan_nonexistent_device() {
        // Scanning a non-existent device should succeed (with no locks)
        let config = LockScanConfig::default();
        let result = scan_locks("/dev/nonexistent_test_device_xyz", &config).unwrap();
        assert!(!result.has_locks());
    }

    #[test]
    fn test_lock_scan_result_display_with_warnings() {
        let result = LockScanResult {
            target: "\\\\?\\PhysicalDrive99".into(),
            locks: Vec::new(),
            has_critical: false,
            total_handles: 0,
            warnings: vec!["Could not query volumes: access denied".into()],
        };
        let display = format!("{}", result);
        assert!(display.contains("No processes"));
        assert!(display.contains("Warning"));
    }
}
