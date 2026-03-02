// Persistent storage — create a persistent storage partition alongside a Linux live image.
// Allows data to survive reboots when booting from a live USB.
// Inspired by Rufus and Ventoy persistent partition support.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Persistent storage filesystem type.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PersistFs {
    /// ext4 — standard Linux filesystem (most compatible with Ubuntu/Debian casper).
    Ext4,
    /// ext3 — older Linux filesystem.
    Ext3,
    /// FAT32 — cross-platform but 4GB file limit.
    Fat32,
    /// NTFS — cross-platform, no file size limit.
    Ntfs,
    /// exFAT — modern cross-platform.
    ExFat,
    /// Btrfs — CoW filesystem with snapshots.
    Btrfs,
}

impl PersistFs {
    /// mkfs command for this filesystem.
    pub fn mkfs_command(&self) -> &str {
        match self {
            Self::Ext4 => "mkfs.ext4",
            Self::Ext3 => "mkfs.ext3",
            Self::Fat32 => "mkfs.vfat",
            Self::Ntfs => "mkfs.ntfs",
            Self::ExFat => "mkfs.exfat",
            Self::Btrfs => "mkfs.btrfs",
        }
    }

    /// Default label for persistent storage.
    pub fn default_label(&self) -> &str {
        match self {
            Self::Ext4 | Self::Ext3 | Self::Btrfs => "casper-rw",
            Self::Fat32 | Self::Ntfs | Self::ExFat => "PERSISTENCE",
        }
    }

    /// Whether this filesystem supports Linux persistence overlay natively.
    pub fn supports_casper(&self) -> bool {
        matches!(self, Self::Ext4 | Self::Ext3 | Self::Btrfs)
    }
}

impl fmt::Display for PersistFs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ext4 => write!(f, "ext4"),
            Self::Ext3 => write!(f, "ext3"),
            Self::Fat32 => write!(f, "FAT32"),
            Self::Ntfs => write!(f, "NTFS"),
            Self::ExFat => write!(f, "exFAT"),
            Self::Btrfs => write!(f, "btrfs"),
        }
    }
}

/// Persistence mode for Linux live USB distributions.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PersistenceMode {
    /// Ubuntu/Debian casper-rw overlay (ext4 partition labeled "casper-rw").
    Casper,
    /// Fedora live persistence with an overlay file.
    FedoraOverlay,
    /// Generic persistence.conf based (writable=yes/no).
    PersistenceConf,
    /// Ventoy-style persistence image file.
    VentoyDat,
}

impl PersistenceMode {
    /// Configuration content needed for this mode.
    pub fn config_content(&self) -> Option<String> {
        match self {
            Self::Casper => None, // No config file needed, it's label-based
            Self::FedoraOverlay => Some("/ union\n".into()),
            Self::PersistenceConf => Some("/ union\n".into()),
            Self::VentoyDat => None, // Ventoy uses its own detection
        }
    }

    /// Expected partition label for this mode.
    pub fn expected_label(&self) -> &str {
        match self {
            Self::Casper => "casper-rw",
            Self::FedoraOverlay => "LIVE",
            Self::PersistenceConf => "persistence",
            Self::VentoyDat => "vtoyefi",
        }
    }

    /// Auto-detect persistence mode from distro name.
    pub fn detect_from_distro(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("ubuntu")
            || lower.contains("mint")
            || lower.contains("debian")
            || lower.contains("elementary")
            || lower.contains("pop")
        {
            Self::Casper
        } else if lower.contains("fedora") {
            Self::FedoraOverlay
        } else if lower.contains("ventoy") {
            Self::VentoyDat
        } else {
            Self::PersistenceConf
        }
    }
}

impl fmt::Display for PersistenceMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Casper => write!(f, "casper-rw (Ubuntu/Debian)"),
            Self::FedoraOverlay => write!(f, "Fedora overlay"),
            Self::PersistenceConf => write!(f, "persistence.conf"),
            Self::VentoyDat => write!(f, "Ventoy persistence"),
        }
    }
}

/// Configuration for creating persistent storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistConfig {
    /// Target device path.
    pub device: String,
    /// Size of the persistent partition (in bytes). 0 = use all remaining space.
    pub size: u64,
    /// Filesystem for the persistent partition.
    pub filesystem: PersistFs,
    /// Partition label.
    pub label: String,
    /// Persistence mode.
    pub mode: PersistenceMode,
    /// Whether to encrypt the persistent partition (LUKS).
    pub encrypt: bool,
}

impl Default for PersistConfig {
    fn default() -> Self {
        Self {
            device: String::new(),
            size: 0,
            filesystem: PersistFs::Ext4,
            label: "casper-rw".into(),
            mode: PersistenceMode::Casper,
            encrypt: false,
        }
    }
}

/// Result of creating persistent storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistResult {
    /// Device path.
    pub device: String,
    /// Partition number created.
    pub partition_number: u32,
    /// Partition device path (e.g., /dev/sdb3).
    pub partition_path: String,
    /// Filesystem used.
    pub filesystem: String,
    /// Label applied.
    pub label: String,
    /// Size of the persistent partition in bytes.
    pub size: u64,
    /// Persistence mode used.
    pub mode: String,
    /// Whether encryption was enabled.
    pub encrypted: bool,
}

impl PersistResult {
    pub fn format_text(&self) -> String {
        let mut out = String::new();
        out.push_str("Persistent Storage Created\n");
        out.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
        out.push_str(&format!("Device:     {}\n", self.device));
        out.push_str(&format!("Partition:  {} ({})\n", self.partition_path, self.partition_number));
        out.push_str(&format!("Filesystem: {}\n", self.filesystem));
        out.push_str(&format!("Label:      {}\n", self.label));
        out.push_str(&format!(
            "Size:       {}\n",
            humansize::format_size(self.size, humansize::BINARY)
        ));
        out.push_str(&format!("Mode:       {}\n", self.mode));
        if self.encrypted {
            out.push_str("Encrypted:  yes (LUKS)\n");
        }
        out
    }
}

/// Calculate the available space after the last partition on a device.
/// Returns (start_offset, available_bytes).
pub fn find_free_space(device: &str, device_size: u64) -> Result<(u64, u64)> {
    // Parse the partition table to find where the last partition ends
    let mut file = std::fs::File::open(device)?;
    let mut mbr = [0u8; 512];
    file.read_exact(&mut mbr)?;

    // Check for GPT
    let has_gpt = {
        let mut lba1 = [0u8; 512];
        file.seek(SeekFrom::Start(512))?;
        file.read_exact(&mut lba1).is_ok() && &lba1[0..8] == b"EFI PART"
    };

    if has_gpt {
        // For GPT, the secondary header is at the end of the disk
        // Free space is after the last partition entry
        // Simplified: assume the last 33 sectors are reserved for backup GPT
        let backup_gpt_size = 33 * 512;
        let usable_end = device_size.saturating_sub(backup_gpt_size as u64);

        // Read partition entries to find last used sector
        file.seek(SeekFrom::Start(1024))?; // Partition entries start at LBA 2
        let mut last_end: u64 = 0;
        let mut entry = [0u8; 128];
        for _ in 0..128 {
            if file.read_exact(&mut entry).is_err() {
                break;
            }
            // Check if entry is empty (all zeros in type GUID)
            if entry[0..16].iter().all(|&b| b == 0) {
                continue;
            }
            // Ending LBA at offset 40 (little-endian u64)
            let end_lba = u64::from_le_bytes(entry[40..48].try_into().unwrap_or([0; 8]));
            let end_byte = (end_lba + 1) * 512;
            if end_byte > last_end {
                last_end = end_byte;
            }
        }

        if last_end == 0 {
            return Err(anyhow!("no partitions found on device"));
        }

        let available = usable_end.saturating_sub(last_end);
        Ok((last_end, available))
    } else {
        // MBR: check partition entries at offsets 446, 462, 478, 494
        let mut last_end: u64 = 0;
        for i in 0..4 {
            let offset = 446 + i * 16;
            let part_type = mbr[offset + 4];
            if part_type == 0 {
                continue; // Empty entry
            }
            let start_lba = u32::from_le_bytes(mbr[offset + 8..offset + 12].try_into().unwrap_or([0; 4]));
            let size_lba = u32::from_le_bytes(mbr[offset + 12..offset + 16].try_into().unwrap_or([0; 4]));
            let end_byte = (start_lba as u64 + size_lba as u64) * 512;
            if end_byte > last_end {
                last_end = end_byte;
            }
        }

        if last_end == 0 {
            return Err(anyhow!("no partitions found on device"));
        }

        let available = device_size.saturating_sub(last_end);
        Ok((last_end, available))
    }
}

/// Create a persistence.conf file content for the given mode.
pub fn generate_persistence_conf(mode: &PersistenceMode) -> Option<String> {
    mode.config_content()
}

/// Estimate the minimum recommended size for persistent storage.
pub fn recommended_min_size(mode: &PersistenceMode) -> u64 {
    match mode {
        PersistenceMode::Casper => 256 * 1024 * 1024,      // 256 MiB
        PersistenceMode::FedoraOverlay => 512 * 1024 * 1024, // 512 MiB
        PersistenceMode::PersistenceConf => 256 * 1024 * 1024,
        PersistenceMode::VentoyDat => 128 * 1024 * 1024,    // 128 MiB
    }
}

/// Create a persistence image file (for Ventoy-style file-based persistence).
pub fn create_persistence_image(
    path: &Path,
    size: u64,
    _fs: PersistFs,
) -> Result<()> {
    // Create a sparse file of the requested size
    let file = std::fs::File::create(path)?;
    file.set_len(size)?;

    // Write an ext4 superblock signature at the expected offset
    // This is a minimal marker — real formatting would use mkfs.ext4
    let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
    // Write the ext4 magic number at offset 0x438 (superblock offset 0x38)
    f.seek(SeekFrom::Start(0x438))?;
    f.write_all(&[0x53, 0xEF])?; // EXT4_SUPER_MAGIC
    f.sync_all()?;

    log::info!(
        "Created persistence image: {} ({})",
        path.display(),
        humansize::format_size(size, humansize::BINARY)
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_persist_fs_display() {
        assert_eq!(format!("{}", PersistFs::Ext4), "ext4");
        assert_eq!(format!("{}", PersistFs::Fat32), "FAT32");
        assert_eq!(format!("{}", PersistFs::Ntfs), "NTFS");
        assert_eq!(format!("{}", PersistFs::ExFat), "exFAT");
        assert_eq!(format!("{}", PersistFs::Btrfs), "btrfs");
    }

    #[test]
    fn test_persist_fs_mkfs() {
        assert_eq!(PersistFs::Ext4.mkfs_command(), "mkfs.ext4");
        assert_eq!(PersistFs::Fat32.mkfs_command(), "mkfs.vfat");
        assert_eq!(PersistFs::Btrfs.mkfs_command(), "mkfs.btrfs");
    }

    #[test]
    fn test_persist_fs_default_label() {
        assert_eq!(PersistFs::Ext4.default_label(), "casper-rw");
        assert_eq!(PersistFs::Fat32.default_label(), "PERSISTENCE");
    }

    #[test]
    fn test_persist_fs_supports_casper() {
        assert!(PersistFs::Ext4.supports_casper());
        assert!(PersistFs::Ext3.supports_casper());
        assert!(PersistFs::Btrfs.supports_casper());
        assert!(!PersistFs::Fat32.supports_casper());
        assert!(!PersistFs::Ntfs.supports_casper());
    }

    #[test]
    fn test_persistence_mode_display() {
        assert_eq!(
            format!("{}", PersistenceMode::Casper),
            "casper-rw (Ubuntu/Debian)"
        );
        assert_eq!(
            format!("{}", PersistenceMode::FedoraOverlay),
            "Fedora overlay"
        );
    }

    #[test]
    fn test_persistence_mode_detect_ubuntu() {
        assert_eq!(
            PersistenceMode::detect_from_distro("Ubuntu 24.04"),
            PersistenceMode::Casper
        );
    }

    #[test]
    fn test_persistence_mode_detect_fedora() {
        assert_eq!(
            PersistenceMode::detect_from_distro("Fedora Workstation 40"),
            PersistenceMode::FedoraOverlay
        );
    }

    #[test]
    fn test_persistence_mode_detect_mint() {
        assert_eq!(
            PersistenceMode::detect_from_distro("Linux Mint 22"),
            PersistenceMode::Casper
        );
    }

    #[test]
    fn test_persistence_mode_detect_generic() {
        assert_eq!(
            PersistenceMode::detect_from_distro("Arch Linux"),
            PersistenceMode::PersistenceConf
        );
    }

    #[test]
    fn test_persistence_mode_config() {
        assert!(PersistenceMode::Casper.config_content().is_none());
        assert_eq!(
            PersistenceMode::FedoraOverlay.config_content().unwrap(),
            "/ union\n"
        );
        assert_eq!(
            PersistenceMode::PersistenceConf.config_content().unwrap(),
            "/ union\n"
        );
    }

    #[test]
    fn test_persistence_mode_label() {
        assert_eq!(PersistenceMode::Casper.expected_label(), "casper-rw");
        assert_eq!(PersistenceMode::FedoraOverlay.expected_label(), "LIVE");
        assert_eq!(PersistenceMode::PersistenceConf.expected_label(), "persistence");
    }

    #[test]
    fn test_default_config() {
        let cfg = PersistConfig::default();
        assert_eq!(cfg.filesystem, PersistFs::Ext4);
        assert_eq!(cfg.label, "casper-rw");
        assert_eq!(cfg.mode, PersistenceMode::Casper);
        assert!(!cfg.encrypt);
    }

    #[test]
    fn test_persist_result_text() {
        let result = PersistResult {
            device: "/dev/sdb".into(),
            partition_number: 3,
            partition_path: "/dev/sdb3".into(),
            filesystem: "ext4".into(),
            label: "casper-rw".into(),
            size: 2 * 1024 * 1024 * 1024,
            mode: "casper-rw (Ubuntu/Debian)".into(),
            encrypted: false,
        };
        let text = result.format_text();
        assert!(text.contains("Persistent Storage Created"));
        assert!(text.contains("/dev/sdb3"));
        assert!(text.contains("casper-rw"));
    }

    #[test]
    fn test_recommended_min_size() {
        assert_eq!(
            recommended_min_size(&PersistenceMode::Casper),
            256 * 1024 * 1024
        );
        assert_eq!(
            recommended_min_size(&PersistenceMode::FedoraOverlay),
            512 * 1024 * 1024
        );
    }

    #[test]
    fn test_generate_persistence_conf() {
        assert!(generate_persistence_conf(&PersistenceMode::Casper).is_none());
        assert!(generate_persistence_conf(&PersistenceMode::PersistenceConf).is_some());
    }

    #[test]
    fn test_create_persistence_image() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("persist.img");
        let size = 64 * 1024 * 1024; // 64 MiB

        create_persistence_image(&path, size, PersistFs::Ext4).unwrap();

        assert!(path.exists());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), size);

        // Verify ext4 magic at offset 0x438
        let data = std::fs::read(&path).unwrap();
        assert_eq!(data[0x438], 0x53);
        assert_eq!(data[0x439], 0xEF);
    }

    #[test]
    fn test_find_free_space_no_partitions() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), vec![0u8; 2048]).unwrap();
        // No valid partition table — should error
        let result = find_free_space(tmp.path().to_str().unwrap(), 2048);
        assert!(result.is_err());
    }
}
