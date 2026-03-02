// UEFI:NTFS dual-partition boot layout — enables booting NTFS partitions on UEFI systems.
//
// Inspired by Rufus's UEFI:NTFS implementation: when a Windows ISO contains
// files >4 GB (e.g. install.wim), it cannot be placed on a FAT32 partition.
// The solution is a dual-partition layout:
//   1. Small FAT32 ESP (~512 KB - 1 MB) containing a UEFI chainloader
//   2. Large NTFS data partition containing the actual Windows files
//
// The UEFI chainloader (UEFI:NTFS) in the ESP can read NTFS and chain-loads
// the Windows boot manager from the NTFS partition.
//
// This module handles:
//   - Detection of when UEFI:NTFS layout is needed (files >4 GB + UEFI target)
//   - Partition layout calculation (ESP size, alignment, NTFS partition)
//   - ESP content generation (directory structure, boot files)
//   - Windows-To-Go support (portable Windows installation)

#![allow(dead_code)]

use anyhow::Result;
use log::info;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Maximum file size on FAT32 (4 GB - 1 byte).
const FAT32_MAX_FILE_SIZE: u64 = 4 * 1024 * 1024 * 1024 - 1;

/// Default ESP partition size in bytes (1 MB).
const DEFAULT_ESP_SIZE: u64 = 1024 * 1024;

/// Minimum ESP partition size in bytes (512 KB).
const MIN_ESP_SIZE: u64 = 512 * 1024;

/// Partition alignment in bytes (1 MB, standard for modern disks).
const PARTITION_ALIGNMENT: u64 = 1024 * 1024;

/// Boot mode for the target system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BootMode {
    /// Legacy BIOS only.
    Bios,
    /// UEFI only.
    Uefi,
    /// Dual BIOS + UEFI (MBR + ESP).
    Dual,
}

impl std::fmt::Display for BootMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BootMode::Bios => write!(f, "BIOS"),
            BootMode::Uefi => write!(f, "UEFI"),
            BootMode::Dual => write!(f, "BIOS+UEFI"),
        }
    }
}

/// Partition scheme for the target disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartitionScheme {
    Mbr,
    Gpt,
}

impl std::fmt::Display for PartitionScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PartitionScheme::Mbr => write!(f, "MBR"),
            PartitionScheme::Gpt => write!(f, "GPT"),
        }
    }
}

/// Filesystem type for the data partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataFilesystem {
    Fat32,
    Ntfs,
    ExFat,
}

impl std::fmt::Display for DataFilesystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataFilesystem::Fat32 => write!(f, "FAT32"),
            DataFilesystem::Ntfs => write!(f, "NTFS"),
            DataFilesystem::ExFat => write!(f, "exFAT"),
        }
    }
}

/// A partition in the layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionInfo {
    /// Partition index (0-based).
    pub index: usize,
    /// Partition label.
    pub label: String,
    /// Offset from start of disk in bytes.
    pub offset: u64,
    /// Partition size in bytes.
    pub size: u64,
    /// Filesystem type.
    pub filesystem: String,
    /// Whether this is the EFI System Partition.
    pub is_esp: bool,
    /// Whether this is the active/bootable partition.
    pub bootable: bool,
    /// GPT partition type GUID (empty for MBR).
    pub type_guid: String,
}

/// Complete disk layout specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskLayout {
    /// Partition scheme.
    pub scheme: PartitionScheme,
    /// Boot mode.
    pub boot_mode: BootMode,
    /// Total disk size in bytes.
    pub disk_size: u64,
    /// Whether UEFI:NTFS dual-partition is used.
    pub uses_uefi_ntfs: bool,
    /// Partitions in order.
    pub partitions: Vec<PartitionInfo>,
    /// Data filesystem (for the main data partition).
    pub data_filesystem: DataFilesystem,
    /// Windows-To-Go mode.
    pub windows_to_go: bool,
}

/// Analysis result for an ISO/image to determine layout requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutAnalysis {
    /// Whether any file exceeds FAT32 4 GB limit.
    pub has_large_files: bool,
    /// Largest file size found.
    pub largest_file_size: u64,
    /// Largest file name.
    pub largest_file_name: String,
    /// Total extracted size needed.
    pub total_size: u64,
    /// Number of files.
    pub file_count: usize,
    /// Whether UEFI:NTFS layout is recommended.
    pub needs_uefi_ntfs: bool,
    /// Recommended data filesystem.
    pub recommended_filesystem: DataFilesystem,
    /// Recommended boot mode.
    pub recommended_boot_mode: BootMode,
}

/// Analyze files in a directory to determine layout requirements.
pub fn analyze_directory(path: &Path) -> Result<LayoutAnalysis> {
    let mut largest_size = 0u64;
    let mut largest_name = String::new();
    let mut total_size = 0u64;
    let mut file_count = 0usize;

    scan_directory(path, &mut largest_size, &mut largest_name, &mut total_size, &mut file_count)?;

    let has_large_files = largest_size > FAT32_MAX_FILE_SIZE;
    let needs_uefi_ntfs = has_large_files; // Simplified: UEFI:NTFS needed when files exceed FAT32 limit

    let recommended_filesystem = if has_large_files {
        DataFilesystem::Ntfs
    } else {
        DataFilesystem::Fat32
    };

    Ok(LayoutAnalysis {
        has_large_files,
        largest_file_size: largest_size,
        largest_file_name: largest_name,
        total_size,
        file_count,
        needs_uefi_ntfs,
        recommended_filesystem,
        recommended_boot_mode: BootMode::Uefi,
    })
}

/// Recursively scan a directory for file sizes.
fn scan_directory(
    path: &Path,
    largest_size: &mut u64,
    largest_name: &mut String,
    total_size: &mut u64,
    file_count: &mut usize,
) -> Result<()> {
    if !path.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if metadata.is_file() {
            let size = metadata.len();
            *total_size += size;
            *file_count += 1;

            if size > *largest_size {
                *largest_size = size;
                *largest_name = entry.file_name().to_string_lossy().to_string();
            }
        } else if metadata.is_dir() {
            scan_directory(&entry.path(), largest_size, largest_name, total_size, file_count)?;
        }
    }

    Ok(())
}

/// Calculate a UEFI:NTFS dual-partition layout.
pub fn calculate_uefi_ntfs_layout(
    disk_size: u64,
    boot_mode: BootMode,
    windows_to_go: bool,
) -> Result<DiskLayout> {
    if disk_size < DEFAULT_ESP_SIZE + PARTITION_ALIGNMENT * 2 {
        anyhow::bail!("Disk too small for UEFI:NTFS layout (need at least {} bytes)", 
            DEFAULT_ESP_SIZE + PARTITION_ALIGNMENT * 2);
    }

    let scheme = match boot_mode {
        BootMode::Uefi | BootMode::Dual => PartitionScheme::Gpt,
        BootMode::Bios => PartitionScheme::Mbr,
    };

    let mut partitions = Vec::new();

    // ESP partition (always first)
    let esp_offset = align_up(PARTITION_ALIGNMENT, PARTITION_ALIGNMENT); // after GPT header area
    let esp_size = align_up(DEFAULT_ESP_SIZE, PARTITION_ALIGNMENT);
    partitions.push(PartitionInfo {
        index: 0,
        label: "EFI".into(),
        offset: esp_offset,
        size: esp_size,
        filesystem: "FAT32".into(),
        is_esp: true,
        bootable: true,
        type_guid: "C12A7328-F81F-11D2-BA4B-00A0C93EC93B".into(), // EFI System Partition
    });

    // NTFS data partition
    let data_offset = align_up(esp_offset + esp_size, PARTITION_ALIGNMENT);
    let data_size = disk_size - data_offset - PARTITION_ALIGNMENT; // leave room at end

    let data_type_guid = if windows_to_go {
        "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7".into() // Microsoft Basic Data
    } else {
        "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7".into() // Microsoft Basic Data
    };

    partitions.push(PartitionInfo {
        index: 1,
        label: if windows_to_go { "WindowsToGo".into() } else { "WINDOWS".into() },
        offset: data_offset,
        size: data_size,
        filesystem: "NTFS".into(),
        is_esp: false,
        bootable: false,
        type_guid: data_type_guid,
    });

    info!(
        "UEFI:NTFS layout: ESP={}MB @ {:X}, NTFS={}MB @ {:X}",
        esp_size / (1024 * 1024),
        esp_offset,
        data_size / (1024 * 1024),
        data_offset
    );

    Ok(DiskLayout {
        scheme,
        boot_mode,
        disk_size,
        uses_uefi_ntfs: true,
        partitions,
        data_filesystem: DataFilesystem::Ntfs,
        windows_to_go,
    })
}

/// Calculate a simple single-partition FAT32 layout.
pub fn calculate_simple_layout(
    disk_size: u64,
    boot_mode: BootMode,
) -> Result<DiskLayout> {
    let scheme = match boot_mode {
        BootMode::Uefi | BootMode::Dual => PartitionScheme::Gpt,
        BootMode::Bios => PartitionScheme::Mbr,
    };

    let data_offset = align_up(PARTITION_ALIGNMENT, PARTITION_ALIGNMENT);
    let data_size = disk_size - data_offset - PARTITION_ALIGNMENT;

    let partitions = vec![PartitionInfo {
        index: 0,
        label: "BOOT".into(),
        offset: data_offset,
        size: data_size,
        filesystem: "FAT32".into(),
        is_esp: boot_mode != BootMode::Bios,
        bootable: true,
        type_guid: if boot_mode == BootMode::Bios {
            String::new()
        } else {
            "C12A7328-F81F-11D2-BA4B-00A0C93EC93B".into()
        },
    }];

    Ok(DiskLayout {
        scheme,
        boot_mode,
        disk_size,
        uses_uefi_ntfs: false,
        partitions,
        data_filesystem: DataFilesystem::Fat32,
        windows_to_go: false,
    })
}

/// Choose the best layout for a given scenario.
pub fn choose_layout(
    disk_size: u64,
    boot_mode: BootMode,
    has_large_files: bool,
    windows_to_go: bool,
) -> Result<DiskLayout> {
    if has_large_files && (boot_mode == BootMode::Uefi || boot_mode == BootMode::Dual) {
        info!("Large files detected + UEFI mode: using UEFI:NTFS dual-partition layout");
        calculate_uefi_ntfs_layout(disk_size, boot_mode, windows_to_go)
    } else {
        info!("Using simple single-partition layout");
        calculate_simple_layout(disk_size, boot_mode)
    }
}

/// ESP directory structure for UEFI:NTFS.
///
/// The ESP must contain at minimum:
///   /EFI/BOOT/BOOTX64.EFI  (for amd64)
///   /EFI/BOOT/BOOTAA64.EFI (for arm64)
///
/// The actual UEFI:NTFS chainloader binary would be:
///   /EFI/Rufus/ntfs_x64.efi (Rufus convention)
/// or our own:
///   /EFI/abt/uefi_ntfs.efi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EspContents {
    /// Architecture-specific boot file path (relative to ESP root).
    pub boot_file: String,
    /// UEFI:NTFS chainloader path.
    pub chainloader: String,
    /// Additional files to include.
    pub extra_files: Vec<(String, Vec<u8>)>,
}

impl EspContents {
    /// Create ESP contents for amd64.
    pub fn for_amd64() -> Self {
        Self {
            boot_file: "EFI/BOOT/BOOTX64.EFI".into(),
            chainloader: "EFI/abt/uefi_ntfs_x64.efi".into(),
            extra_files: vec![],
        }
    }

    /// Create ESP contents for arm64.
    pub fn for_arm64() -> Self {
        Self {
            boot_file: "EFI/BOOT/BOOTAA64.EFI".into(),
            chainloader: "EFI/abt/uefi_ntfs_aa64.efi".into(),
            extra_files: vec![],
        }
    }

    /// Get all required directory paths.
    pub fn required_directories(&self) -> Vec<String> {
        let mut dirs = vec!["EFI".to_string(), "EFI/BOOT".to_string(), "EFI/abt".to_string()];
        // Add directories from extra files
        for (path, _) in &self.extra_files {
            if let Some(parent) = Path::new(path).parent() {
                let p = parent.to_string_lossy().to_string();
                if !p.is_empty() && !dirs.contains(&p) {
                    dirs.push(p);
                }
            }
        }
        dirs
    }
}

/// Align a value up to the given alignment boundary.
fn align_up(value: u64, alignment: u64) -> u64 {
    (value + alignment - 1) / alignment * alignment
}

/// Format a byte count for display.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_mode_display() {
        assert_eq!(BootMode::Bios.to_string(), "BIOS");
        assert_eq!(BootMode::Uefi.to_string(), "UEFI");
        assert_eq!(BootMode::Dual.to_string(), "BIOS+UEFI");
    }

    #[test]
    fn test_partition_scheme_display() {
        assert_eq!(PartitionScheme::Mbr.to_string(), "MBR");
        assert_eq!(PartitionScheme::Gpt.to_string(), "GPT");
    }

    #[test]
    fn test_data_filesystem_display() {
        assert_eq!(DataFilesystem::Fat32.to_string(), "FAT32");
        assert_eq!(DataFilesystem::Ntfs.to_string(), "NTFS");
        assert_eq!(DataFilesystem::ExFat.to_string(), "exFAT");
    }

    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0, 1024), 0);
        assert_eq!(align_up(1, 1024), 1024);
        assert_eq!(align_up(1024, 1024), 1024);
        assert_eq!(align_up(1025, 1024), 2048);
        assert_eq!(align_up(512, 1024 * 1024), 1024 * 1024);
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_size(1536 * 1024), "1.5 MB");
    }

    #[test]
    fn test_fat32_max_file_size() {
        assert_eq!(FAT32_MAX_FILE_SIZE, 4294967295);
    }

    #[test]
    fn test_calculate_uefi_ntfs_layout() {
        let disk_size = 16u64 * 1024 * 1024 * 1024; // 16 GB
        let layout = calculate_uefi_ntfs_layout(disk_size, BootMode::Uefi, false).unwrap();

        assert!(layout.uses_uefi_ntfs);
        assert_eq!(layout.scheme, PartitionScheme::Gpt);
        assert_eq!(layout.boot_mode, BootMode::Uefi);
        assert_eq!(layout.partitions.len(), 2);

        // First partition is ESP
        assert!(layout.partitions[0].is_esp);
        assert_eq!(layout.partitions[0].filesystem, "FAT32");
        assert!(layout.partitions[0].bootable);

        // Second partition is NTFS data
        assert!(!layout.partitions[1].is_esp);
        assert_eq!(layout.partitions[1].filesystem, "NTFS");
        assert_eq!(layout.partitions[1].label, "WINDOWS");
    }

    #[test]
    fn test_calculate_uefi_ntfs_layout_wtg() {
        let disk_size = 32u64 * 1024 * 1024 * 1024;
        let layout = calculate_uefi_ntfs_layout(disk_size, BootMode::Uefi, true).unwrap();
        assert!(layout.windows_to_go);
        assert_eq!(layout.partitions[1].label, "WindowsToGo");
    }

    #[test]
    fn test_calculate_uefi_ntfs_layout_too_small() {
        let disk_size = 512 * 1024; // 512 KB — too small
        assert!(calculate_uefi_ntfs_layout(disk_size, BootMode::Uefi, false).is_err());
    }

    #[test]
    fn test_calculate_simple_layout_uefi() {
        let disk_size = 8u64 * 1024 * 1024 * 1024;
        let layout = calculate_simple_layout(disk_size, BootMode::Uefi).unwrap();

        assert!(!layout.uses_uefi_ntfs);
        assert_eq!(layout.scheme, PartitionScheme::Gpt);
        assert_eq!(layout.partitions.len(), 1);
        assert_eq!(layout.partitions[0].filesystem, "FAT32");
        assert!(layout.partitions[0].is_esp);
    }

    #[test]
    fn test_calculate_simple_layout_bios() {
        let disk_size = 8u64 * 1024 * 1024 * 1024;
        let layout = calculate_simple_layout(disk_size, BootMode::Bios).unwrap();

        assert_eq!(layout.scheme, PartitionScheme::Mbr);
        assert!(!layout.partitions[0].is_esp);
        assert!(layout.partitions[0].type_guid.is_empty());
    }

    #[test]
    fn test_choose_layout_large_files_uefi() {
        let disk_size = 16u64 * 1024 * 1024 * 1024;
        let layout = choose_layout(disk_size, BootMode::Uefi, true, false).unwrap();
        assert!(layout.uses_uefi_ntfs);
        assert_eq!(layout.partitions.len(), 2);
    }

    #[test]
    fn test_choose_layout_small_files_uefi() {
        let disk_size = 8u64 * 1024 * 1024 * 1024;
        let layout = choose_layout(disk_size, BootMode::Uefi, false, false).unwrap();
        assert!(!layout.uses_uefi_ntfs);
        assert_eq!(layout.partitions.len(), 1);
    }

    #[test]
    fn test_choose_layout_large_files_bios() {
        let disk_size = 16u64 * 1024 * 1024 * 1024;
        let layout = choose_layout(disk_size, BootMode::Bios, true, false).unwrap();
        // BIOS mode doesn't need UEFI:NTFS even with large files
        assert!(!layout.uses_uefi_ntfs);
    }

    #[test]
    fn test_esp_contents_amd64() {
        let esp = EspContents::for_amd64();
        assert!(esp.boot_file.contains("BOOTX64"));
        assert!(esp.chainloader.contains("x64"));
    }

    #[test]
    fn test_esp_contents_arm64() {
        let esp = EspContents::for_arm64();
        assert!(esp.boot_file.contains("BOOTAA64"));
        assert!(esp.chainloader.contains("aa64"));
    }

    #[test]
    fn test_esp_required_directories() {
        let esp = EspContents::for_amd64();
        let dirs = esp.required_directories();
        assert!(dirs.contains(&"EFI".to_string()));
        assert!(dirs.contains(&"EFI/BOOT".to_string()));
        assert!(dirs.contains(&"EFI/abt".to_string()));
    }

    #[test]
    fn test_partition_alignment() {
        let disk_size = 16u64 * 1024 * 1024 * 1024;
        let layout = calculate_uefi_ntfs_layout(disk_size, BootMode::Uefi, false).unwrap();
        for p in &layout.partitions {
            assert_eq!(p.offset % PARTITION_ALIGNMENT, 0, "Partition {} not aligned", p.label);
        }
    }

    #[test]
    fn test_layout_serde_roundtrip() {
        let layout = DiskLayout {
            scheme: PartitionScheme::Gpt,
            boot_mode: BootMode::Uefi,
            disk_size: 16 * 1024 * 1024 * 1024,
            uses_uefi_ntfs: true,
            partitions: vec![PartitionInfo {
                index: 0,
                label: "EFI".into(),
                offset: 1048576,
                size: 1048576,
                filesystem: "FAT32".into(),
                is_esp: true,
                bootable: true,
                type_guid: "C12A7328-F81F-11D2-BA4B-00A0C93EC93B".into(),
            }],
            data_filesystem: DataFilesystem::Ntfs,
            windows_to_go: false,
        };
        let json = serde_json::to_string(&layout).unwrap();
        let deser: DiskLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.partitions[0].label, "EFI");
        assert!(deser.uses_uefi_ntfs);
    }

    #[test]
    fn test_analyze_directory() {
        let dir = tempfile::tempdir().unwrap();
        // Create some files
        std::fs::write(dir.path().join("small.txt"), "hello").unwrap();
        std::fs::write(dir.path().join("medium.bin"), vec![0u8; 1024]).unwrap();
        let sub = dir.path().join("subdir");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("nested.dat"), vec![0u8; 2048]).unwrap();

        let analysis = analyze_directory(dir.path()).unwrap();
        assert_eq!(analysis.file_count, 3);
        assert!(!analysis.has_large_files);
        assert!(!analysis.needs_uefi_ntfs);
        assert_eq!(analysis.recommended_filesystem, DataFilesystem::Fat32);
        assert_eq!(analysis.total_size, 5 + 1024 + 2048);
    }

    #[test]
    fn test_layout_analysis_serde() {
        let analysis = LayoutAnalysis {
            has_large_files: true,
            largest_file_size: 5_000_000_000,
            largest_file_name: "install.wim".into(),
            total_size: 6_000_000_000,
            file_count: 100,
            needs_uefi_ntfs: true,
            recommended_filesystem: DataFilesystem::Ntfs,
            recommended_boot_mode: BootMode::Uefi,
        };
        let json = serde_json::to_string(&analysis).unwrap();
        let deser: LayoutAnalysis = serde_json::from_str(&json).unwrap();
        assert!(deser.has_large_files);
        assert_eq!(deser.largest_file_name, "install.wim");
    }
}
