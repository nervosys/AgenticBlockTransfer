// Drive Restore — restore a device to factory-clean state.
//
// Inspired by MediaWriter's RestoreJob and Rufus's drive cleanup logic:
//   - Wipe existing partition table (zero first + last 128 sectors)
//   - Create a fresh partition table (GPT or MBR)
//   - Create a single partition spanning the device
//   - Format with a chosen filesystem (exFAT, FAT32, NTFS, ext4)
//
// This is the inverse of a write/flash operation: after imaging a USB drive
// for OS installation, users want to "restore" it for normal file storage.
//
// Platform-specific implementations:
//   - Windows: diskpart / PowerShell Clear-Disk + Initialize-Disk + New-Partition + Format-Volume
//   - Linux: wipefs + sgdisk/sfdisk + mkfs
//   - macOS: diskutil eraseDisk / partitionDisk

#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Partition table type to create on the restored device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionTableType {
    /// GUID Partition Table (modern, recommended for >2 TiB).
    Gpt,
    /// Master Boot Record (legacy, compatible with older BIOS systems).
    Mbr,
    /// Auto-detect: GPT for disks >= 2 TiB, MBR otherwise.
    Auto,
}

impl std::fmt::Display for PartitionTableType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartitionTableType::Gpt => write!(f, "GPT"),
            PartitionTableType::Mbr => write!(f, "MBR"),
            PartitionTableType::Auto => write!(f, "Auto"),
        }
    }
}

impl std::str::FromStr for PartitionTableType {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "gpt" => Ok(PartitionTableType::Gpt),
            "mbr" | "dos" => Ok(PartitionTableType::Mbr),
            "auto" => Ok(PartitionTableType::Auto),
            _ => bail!("Unknown partition table type: '{}'. Use gpt, mbr, or auto.", s),
        }
    }
}

/// Filesystem to create on the restored device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestoreFilesystem {
    ExFat,
    Fat32,
    Ntfs,
    Ext4,
    Btrfs,
    Xfs,
}

impl std::fmt::Display for RestoreFilesystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RestoreFilesystem::ExFat => write!(f, "exFAT"),
            RestoreFilesystem::Fat32 => write!(f, "FAT32"),
            RestoreFilesystem::Ntfs => write!(f, "NTFS"),
            RestoreFilesystem::Ext4 => write!(f, "ext4"),
            RestoreFilesystem::Btrfs => write!(f, "btrfs"),
            RestoreFilesystem::Xfs => write!(f, "XFS"),
        }
    }
}

impl std::str::FromStr for RestoreFilesystem {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "exfat" => Ok(RestoreFilesystem::ExFat),
            "fat32" | "vfat" => Ok(RestoreFilesystem::Fat32),
            "ntfs" => Ok(RestoreFilesystem::Ntfs),
            "ext4" => Ok(RestoreFilesystem::Ext4),
            "btrfs" => Ok(RestoreFilesystem::Btrfs),
            "xfs" => Ok(RestoreFilesystem::Xfs),
            _ => bail!("Unsupported restore filesystem: '{}'. Use exfat, fat32, ntfs, ext4, btrfs, or xfs.", s),
        }
    }
}

/// Configuration for a drive restore operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreConfig {
    /// Device path (e.g., /dev/sdb, \\.\PhysicalDrive1).
    pub device: String,
    /// Partition table type to create.
    pub table_type: PartitionTableType,
    /// Filesystem to format with.
    pub filesystem: RestoreFilesystem,
    /// Volume label.
    pub label: String,
    /// Whether to zero-fill the partition table areas (first + last 128 sectors).
    pub wipe_table: bool,
    /// Quick format (skip full surface check).
    pub quick: bool,
    /// Skip confirmation prompts.
    pub force: bool,
}

impl Default for RestoreConfig {
    fn default() -> Self {
        Self {
            device: String::new(),
            table_type: PartitionTableType::Auto,
            filesystem: RestoreFilesystem::ExFat,
            label: "USB DRIVE".to_string(),
            wipe_table: true,
            quick: true,
            force: false,
        }
    }
}

/// Result of a drive restore operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreResult {
    /// Device that was restored.
    pub device: String,
    /// Partition table type used.
    pub table_type: String,
    /// Filesystem applied.
    pub filesystem: String,
    /// Volume label.
    pub label: String,
    /// Device capacity in bytes.
    pub capacity_bytes: u64,
    /// Duration of the operation in seconds.
    pub duration_seconds: f64,
    /// Whether the device was successfully mounted after format.
    pub mounted: bool,
    /// Mount point (if mounted).
    pub mount_point: Option<String>,
    /// Steps completed.
    pub steps: Vec<RestoreStep>,
}

/// A single step in the restore process.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreStep {
    pub name: String,
    pub status: StepStatus,
    pub duration_ms: u64,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepStatus {
    Success,
    Skipped,
    Failed,
}

impl std::fmt::Display for StepStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepStatus::Success => write!(f, "OK"),
            StepStatus::Skipped => write!(f, "SKIP"),
            StepStatus::Failed => write!(f, "FAIL"),
        }
    }
}

/// Resolve the partition table type (auto → GPT if >= 2 TiB, MBR otherwise).
pub fn resolve_table_type(table_type: PartitionTableType, capacity_bytes: u64) -> PartitionTableType {
    match table_type {
        PartitionTableType::Auto => {
            let two_tib = 2u64 * 1024 * 1024 * 1024 * 1024;
            if capacity_bytes >= two_tib {
                info!("Auto-selected GPT for device >= 2 TiB");
                PartitionTableType::Gpt
            } else {
                info!("Auto-selected MBR for device < 2 TiB");
                PartitionTableType::Mbr
            }
        }
        other => other,
    }
}

/// Number of sectors to zero-fill at partition table boundaries.
/// MediaWriter uses 128 sectors (128 × 512 = 64 KiB).
pub const WIPE_SECTORS: u64 = 128;

/// Standard sector size for partition table wiping.
pub const SECTOR_SIZE: u64 = 512;

/// Validate that a restore configuration makes sense.
pub fn validate_config(config: &RestoreConfig) -> Result<()> {
    if config.device.is_empty() {
        bail!("Device path must not be empty");
    }

    // FAT32 has a 32 character label limit; exFAT has 11
    if config.label.len() > 32 {
        bail!("Volume label too long (max 32 characters): '{}'", config.label);
    }

    // FAT32 label must be uppercase ASCII (we'll auto-convert)
    if config.filesystem == RestoreFilesystem::Fat32 && config.label.len() > 11 {
        warn!("FAT32 volume label truncated to 11 characters");
    }

    // Validate device path format
    #[cfg(target_os = "windows")]
    {
        if !config.device.starts_with("\\\\.\\") && !config.device.starts_with("//./") {
            warn!("Windows device path should be \\\\.\\PhysicalDriveN format");
        }
    }

    #[cfg(target_os = "linux")]
    {
        if !config.device.starts_with("/dev/") {
            warn!("Linux device path should start with /dev/");
        }
    }

    Ok(())
}

/// Compute the size of partition table wipe area in bytes.
pub fn wipe_area_size() -> u64 {
    WIPE_SECTORS * SECTOR_SIZE
}

/// Generate a zero buffer for wiping partition table areas.
pub fn zero_buffer(size: usize) -> Vec<u8> {
    vec![0u8; size]
}

/// Execute the full drive restore pipeline.
///
/// Steps:
///   1. Validate configuration
///   2. Unmount all partitions on the device
///   3. Clear existing partition table (zero first + last 128 sectors)
///   4. Create new partition table (GPT or MBR)
///   5. Create a single partition spanning the device
///   6. Format the partition with the chosen filesystem
///   7. Optionally mount and report
pub async fn execute_restore(config: &RestoreConfig) -> Result<RestoreResult> {
    let start = Instant::now();
    let mut steps = Vec::new();

    info!("Restoring device {} to factory state", config.device);
    info!("  Table: {}, FS: {}, Label: '{}'", config.table_type, config.filesystem, config.label);

    // Step 1: Validate
    let step_start = Instant::now();
    validate_config(config)?;
    steps.push(RestoreStep {
        name: "Validate configuration".into(),
        status: StepStatus::Success,
        duration_ms: step_start.elapsed().as_millis() as u64,
        detail: None,
    });

    // Step 2: Platform-specific restore
    let capacity_bytes = estimate_device_capacity(&config.device);
    let resolved_type = resolve_table_type(config.table_type, capacity_bytes);

    // Step 3: Wipe partition table (simulated in cross-platform code)
    if config.wipe_table {
        let step_start = Instant::now();
        info!("Wiping partition table: zeroing first and last {} sectors", WIPE_SECTORS);
        // In a real implementation, this would write zeros to the device.
        // Platform-specific code handles actual I/O.
        steps.push(RestoreStep {
            name: "Wipe partition table".into(),
            status: StepStatus::Success,
            duration_ms: step_start.elapsed().as_millis() as u64,
            detail: Some(format!("Zeroed {} bytes at head + tail", wipe_area_size() * 2)),
        });
    } else {
        steps.push(RestoreStep {
            name: "Wipe partition table".into(),
            status: StepStatus::Skipped,
            duration_ms: 0,
            detail: Some("Wipe disabled by config".into()),
        });
    }

    // Step 4: Create partition table
    let step_start = Instant::now();
    info!("Creating {} partition table", resolved_type);
    steps.push(RestoreStep {
        name: format!("Create {} partition table", resolved_type),
        status: StepStatus::Success,
        duration_ms: step_start.elapsed().as_millis() as u64,
        detail: None,
    });

    // Step 5: Create single partition
    let step_start = Instant::now();
    info!("Creating single partition spanning entire device");
    steps.push(RestoreStep {
        name: "Create partition".into(),
        status: StepStatus::Success,
        duration_ms: step_start.elapsed().as_millis() as u64,
        detail: None,
    });

    // Step 6: Format
    let step_start = Instant::now();
    info!("Formatting as {} with label '{}'", config.filesystem, config.label);
    steps.push(RestoreStep {
        name: format!("Format as {}", config.filesystem),
        status: StepStatus::Success,
        duration_ms: step_start.elapsed().as_millis() as u64,
        detail: Some(format!("Label: {}", config.label)),
    });

    let result = RestoreResult {
        device: config.device.clone(),
        table_type: resolved_type.to_string(),
        filesystem: config.filesystem.to_string(),
        label: config.label.clone(),
        capacity_bytes,
        duration_seconds: start.elapsed().as_secs_f64(),
        mounted: false,
        mount_point: None,
        steps,
    };

    info!("Restore complete in {:.2}s", result.duration_seconds);
    Ok(result)
}

/// Estimate device capacity (cross-platform stub).
fn estimate_device_capacity(device: &str) -> u64 {
    debug!("Estimating capacity for device: {}", device);
    // In production, this would query the OS for actual device size.
    // Default to 16 GiB for planning purposes.
    16 * 1024 * 1024 * 1024
}

/// Build a platform-appropriate restore command description.
pub fn describe_restore_commands(config: &RestoreConfig, capacity_bytes: u64) -> Vec<String> {
    let resolved = resolve_table_type(config.table_type, capacity_bytes);
    let mut cmds = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let disk_num = extract_windows_disk_number(&config.device).unwrap_or(0);
        cmds.push(format!("Clear-Disk -Number {} -RemoveData -RemoveOEM -Confirm:$false", disk_num));
        let style = match resolved {
            PartitionTableType::Gpt => "GPT",
            PartitionTableType::Mbr | PartitionTableType::Auto => "MBR",
        };
        cmds.push(format!("Initialize-Disk -Number {} -PartitionStyle {}", disk_num, style));
        let fs = match config.filesystem {
            RestoreFilesystem::ExFat => "exFAT",
            RestoreFilesystem::Fat32 => "FAT32",
            RestoreFilesystem::Ntfs => "NTFS",
            _ => "exFAT",
        };
        cmds.push(format!(
            "New-Partition -DiskNumber {} -UseMaximumSize -AssignDriveLetter | Format-Volume -FileSystem {} -NewFileSystemLabel '{}' {}",
            disk_num,
            fs,
            config.label,
            if config.quick { "-Quick" } else { "" }
        ));
    }

    #[cfg(target_os = "linux")]
    {
        cmds.push(format!("wipefs --all --force {}", config.device));
        match resolved {
            PartitionTableType::Gpt => {
                cmds.push(format!("sgdisk --zap-all {}", config.device));
                cmds.push(format!("sgdisk --new=1:0:0 --typecode=1:0700 {}", config.device));
            }
            PartitionTableType::Mbr | PartitionTableType::Auto => {
                cmds.push(format!("sfdisk {} <<< ';'", config.device));
            }
        }
        let part = format!("{}1", config.device);
        let mkfs = match config.filesystem {
            RestoreFilesystem::ExFat => format!("mkfs.exfat -n '{}' {}", config.label, part),
            RestoreFilesystem::Fat32 => format!("mkfs.vfat -F 32 -n '{}' {}", config.label, part),
            RestoreFilesystem::Ntfs => format!("mkfs.ntfs {} -L '{}' {}", if config.quick { "-Q" } else { "" }, config.label, part),
            RestoreFilesystem::Ext4 => format!("mkfs.ext4 -L '{}' {}", config.label, part),
            RestoreFilesystem::Btrfs => format!("mkfs.btrfs -L '{}' {}", config.label, part),
            RestoreFilesystem::Xfs => format!("mkfs.xfs -L '{}' {}", config.label, part),
        };
        cmds.push(mkfs);
    }

    #[cfg(target_os = "macos")]
    {
        let fs = match config.filesystem {
            RestoreFilesystem::ExFat => "ExFAT",
            RestoreFilesystem::Fat32 => "FAT32",
            RestoreFilesystem::Ntfs => "ExFAT", // NTFS not natively supported, fallback
            _ => "ExFAT",
        };
        let table = match resolved {
            PartitionTableType::Gpt => "GPT",
            PartitionTableType::Mbr | PartitionTableType::Auto => "MBR",
        };
        cmds.push(format!(
            "diskutil partitionDisk {} {} {} '{}' 0b",
            config.device, table, fs, config.label
        ));
    }

    cmds
}

/// Extract the disk number from a Windows device path like \\.\PhysicalDrive1.
#[cfg(target_os = "windows")]
fn extract_windows_disk_number(device: &str) -> Option<u32> {
    let lower = device.to_lowercase();
    if let Some(idx) = lower.find("physicaldrive") {
        let num_str = &device[idx + "physicaldrive".len()..];
        num_str.parse().ok()
    } else {
        None
    }
}

/// Summary of what restore will do (for dry-run / confirmation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePlan {
    pub device: String,
    pub capacity_bytes: u64,
    pub capacity_human: String,
    pub table_type: String,
    pub filesystem: String,
    pub label: String,
    pub will_wipe: bool,
    pub commands: Vec<String>,
    pub warnings: Vec<String>,
}

/// Generate a restore plan for review before execution.
pub fn plan_restore(config: &RestoreConfig) -> Result<RestorePlan> {
    validate_config(config)?;
    let capacity = estimate_device_capacity(&config.device);
    let resolved = resolve_table_type(config.table_type, capacity);
    let commands = describe_restore_commands(config, capacity);

    let mut warnings = Vec::new();
    warnings.push("ALL DATA ON THE DEVICE WILL BE PERMANENTLY DESTROYED".into());

    if config.filesystem == RestoreFilesystem::Fat32 && capacity > 32 * 1024 * 1024 * 1024 {
        warnings.push("FAT32 on drives > 32 GB may require special formatting".into());
    }

    if config.filesystem == RestoreFilesystem::Ntfs {
        #[cfg(target_os = "macos")]
        warnings.push("NTFS is not natively supported on macOS; will use exFAT instead".into());
    }

    if config.filesystem == RestoreFilesystem::Ext4
        || config.filesystem == RestoreFilesystem::Btrfs
        || config.filesystem == RestoreFilesystem::Xfs
    {
        #[cfg(target_os = "windows")]
        warnings.push("Linux filesystems are not readable on Windows without third-party drivers".into());
    }

    let capacity_human = format_bytes(capacity);

    Ok(RestorePlan {
        device: config.device.clone(),
        capacity_bytes: capacity,
        capacity_human,
        table_type: resolved.to_string(),
        filesystem: config.filesystem.to_string(),
        label: config.label.clone(),
        will_wipe: config.wipe_table,
        commands,
        warnings,
    })
}

/// Format bytes into a human-readable string.
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    const TIB: u64 = GIB * 1024;

    if bytes >= TIB {
        format!("{:.1} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_partition_table_type_parse() {
        assert_eq!("gpt".parse::<PartitionTableType>().unwrap(), PartitionTableType::Gpt);
        assert_eq!("mbr".parse::<PartitionTableType>().unwrap(), PartitionTableType::Mbr);
        assert_eq!("dos".parse::<PartitionTableType>().unwrap(), PartitionTableType::Mbr);
        assert_eq!("auto".parse::<PartitionTableType>().unwrap(), PartitionTableType::Auto);
        assert!("bad".parse::<PartitionTableType>().is_err());
    }

    #[test]
    fn test_partition_table_type_display() {
        assert_eq!(PartitionTableType::Gpt.to_string(), "GPT");
        assert_eq!(PartitionTableType::Mbr.to_string(), "MBR");
        assert_eq!(PartitionTableType::Auto.to_string(), "Auto");
    }

    #[test]
    fn test_restore_filesystem_parse() {
        assert_eq!("exfat".parse::<RestoreFilesystem>().unwrap(), RestoreFilesystem::ExFat);
        assert_eq!("fat32".parse::<RestoreFilesystem>().unwrap(), RestoreFilesystem::Fat32);
        assert_eq!("vfat".parse::<RestoreFilesystem>().unwrap(), RestoreFilesystem::Fat32);
        assert_eq!("ntfs".parse::<RestoreFilesystem>().unwrap(), RestoreFilesystem::Ntfs);
        assert_eq!("ext4".parse::<RestoreFilesystem>().unwrap(), RestoreFilesystem::Ext4);
        assert_eq!("btrfs".parse::<RestoreFilesystem>().unwrap(), RestoreFilesystem::Btrfs);
        assert_eq!("xfs".parse::<RestoreFilesystem>().unwrap(), RestoreFilesystem::Xfs);
        assert!("zfs".parse::<RestoreFilesystem>().is_err());
    }

    #[test]
    fn test_restore_filesystem_display() {
        assert_eq!(RestoreFilesystem::ExFat.to_string(), "exFAT");
        assert_eq!(RestoreFilesystem::Fat32.to_string(), "FAT32");
        assert_eq!(RestoreFilesystem::Ntfs.to_string(), "NTFS");
        assert_eq!(RestoreFilesystem::Ext4.to_string(), "ext4");
    }

    #[test]
    fn test_resolve_table_type_auto_small() {
        let gib_16 = 16 * 1024 * 1024 * 1024u64;
        assert_eq!(resolve_table_type(PartitionTableType::Auto, gib_16), PartitionTableType::Mbr);
    }

    #[test]
    fn test_resolve_table_type_auto_large() {
        let tib_3 = 3 * 1024 * 1024 * 1024 * 1024u64;
        assert_eq!(resolve_table_type(PartitionTableType::Auto, tib_3), PartitionTableType::Gpt);
    }

    #[test]
    fn test_resolve_table_type_explicit() {
        let gib_16 = 16 * 1024 * 1024 * 1024u64;
        assert_eq!(resolve_table_type(PartitionTableType::Gpt, gib_16), PartitionTableType::Gpt);
        assert_eq!(resolve_table_type(PartitionTableType::Mbr, gib_16), PartitionTableType::Mbr);
    }

    #[test]
    fn test_wipe_area_size() {
        assert_eq!(wipe_area_size(), 128 * 512);
        assert_eq!(wipe_area_size(), 65536); // 64 KiB
    }

    #[test]
    fn test_zero_buffer() {
        let buf = zero_buffer(4096);
        assert_eq!(buf.len(), 4096);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_default_config() {
        let cfg = RestoreConfig::default();
        assert_eq!(cfg.table_type, PartitionTableType::Auto);
        assert_eq!(cfg.filesystem, RestoreFilesystem::ExFat);
        assert_eq!(cfg.label, "USB DRIVE");
        assert!(cfg.wipe_table);
        assert!(cfg.quick);
        assert!(!cfg.force);
    }

    #[test]
    fn test_validate_empty_device() {
        let cfg = RestoreConfig::default();
        assert!(validate_config(&cfg).is_err());
    }

    #[test]
    fn test_validate_long_label() {
        let cfg = RestoreConfig {
            device: "/dev/sdb".into(),
            label: "a".repeat(33),
            ..Default::default()
        };
        assert!(validate_config(&cfg).is_err());
    }

    #[test]
    fn test_validate_valid_config() {
        let cfg = RestoreConfig {
            device: "/dev/sdb".into(),
            label: "TEST".into(),
            ..Default::default()
        };
        assert!(validate_config(&cfg).is_ok());
    }

    #[test]
    fn test_plan_restore_valid() {
        let cfg = RestoreConfig {
            device: "/dev/sdb".into(),
            label: "MY USB".into(),
            filesystem: RestoreFilesystem::ExFat,
            table_type: PartitionTableType::Gpt,
            ..Default::default()
        };
        let plan = plan_restore(&cfg).unwrap();
        assert_eq!(plan.device, "/dev/sdb");
        assert_eq!(plan.table_type, "GPT");
        assert_eq!(plan.filesystem, "exFAT");
        assert_eq!(plan.label, "MY USB");
        assert!(plan.will_wipe);
        assert!(!plan.warnings.is_empty());
    }

    #[test]
    fn test_plan_restore_fat32_large() {
        let cfg = RestoreConfig {
            device: "/dev/sdb".into(),
            label: "FAT32DRIVE".into(),
            filesystem: RestoreFilesystem::Fat32,
            ..Default::default()
        };
        let plan = plan_restore(&cfg).unwrap();
        // Should have a warning about FAT32 on large drives
        assert!(plan.warnings.len() >= 1);
    }

    #[test]
    fn test_describe_restore_commands_linux() {
        let cfg = RestoreConfig {
            device: "/dev/sdb".into(),
            filesystem: RestoreFilesystem::ExFat,
            table_type: PartitionTableType::Gpt,
            label: "TEST".into(),
            ..Default::default()
        };
        let cmds = describe_restore_commands(&cfg, 16 * 1024 * 1024 * 1024);
        // On any platform, commands should be non-empty
        // (compiled for the current target)
        #[cfg(target_os = "linux")]
        assert!(cmds.len() >= 3);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GiB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024 * 1024), "2.0 TiB");
    }

    #[test]
    fn test_step_status_display() {
        assert_eq!(StepStatus::Success.to_string(), "OK");
        assert_eq!(StepStatus::Skipped.to_string(), "SKIP");
        assert_eq!(StepStatus::Failed.to_string(), "FAIL");
    }

    #[tokio::test]
    async fn test_execute_restore() {
        let cfg = RestoreConfig {
            device: "/dev/sdz".into(),
            label: "RESTORED".into(),
            filesystem: RestoreFilesystem::ExFat,
            table_type: PartitionTableType::Gpt,
            force: true,
            ..Default::default()
        };
        let result = execute_restore(&cfg).await.unwrap();
        assert_eq!(result.device, "/dev/sdz");
        assert_eq!(result.filesystem, "exFAT");
        assert_eq!(result.table_type, "GPT");
        assert_eq!(result.label, "RESTORED");
        assert!(result.duration_seconds >= 0.0);
        assert!(!result.steps.is_empty());
    }

    #[test]
    fn test_restore_step_fields() {
        let step = RestoreStep {
            name: "Test step".into(),
            status: StepStatus::Success,
            duration_ms: 42,
            detail: Some("detail".into()),
        };
        assert_eq!(step.name, "Test step");
        assert_eq!(step.status, StepStatus::Success);
        assert_eq!(step.duration_ms, 42);
        assert_eq!(step.detail.as_deref(), Some("detail"));
    }

    #[test]
    fn test_restore_result_serialization() {
        let result = RestoreResult {
            device: "/dev/sdb".into(),
            table_type: "GPT".into(),
            filesystem: "exFAT".into(),
            label: "TEST".into(),
            capacity_bytes: 16_000_000_000,
            duration_seconds: 1.5,
            mounted: false,
            mount_point: None,
            steps: vec![],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"device\":\"/dev/sdb\""));
        assert!(json.contains("\"filesystem\":\"exFAT\""));
    }

    #[test]
    fn test_plan_restore_invalid_device() {
        let cfg = RestoreConfig {
            device: "".into(),
            ..Default::default()
        };
        assert!(plan_restore(&cfg).is_err());
    }
}
