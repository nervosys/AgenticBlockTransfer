// Large FAT32 formatter — format drives >32 GB as FAT32 with custom cluster sizes.
//
// Windows artificially limits FAT32 formatting to 32 GB. Rufus's fat32format
// (based on Ridgecrop Consultants' fat32format) bypasses this by writing FAT32
// structures directly. This module provides a pure-Rust FAT32 formatter that
// works on drives up to 2 TB with user-selectable cluster sizes.
//
// FAT32 on large drives is commonly needed for:
//   - Game consoles (PS3, PS4 external, Nintendo Switch in some scenarios)
//   - Car infotainment systems
//   - Camera card readers
//   - Cross-platform file sharing (FAT32 is universally supported)
//   - Embedded systems and microcontrollers
//
// Reference: Microsoft FAT32 File System Specification (fatgen103.doc)
//            Rufus/src/format_fat32.c (fat32format by Tom Thornhill)

use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Seek, SeekFrom, Write};

/// FAT32 Boot Sector / BPB structure constants.
const FAT32_SIGNATURE: u16 = 0xAA55;
const FAT32_FSINFO_SIGNATURE_1: u32 = 0x41615252;
const FAT32_FSINFO_SIGNATURE_2: u32 = 0x61417272;
const FAT32_FSINFO_SIGNATURE_3: u32 = 0xAA550000;
const FAT_MEDIA_FIXED: u8 = 0xF8;
const FAT_MEDIA_REMOVABLE: u8 = 0xF0;
const BS_OEM_NAME: &[u8; 8] = b"ABT     ";

/// Maximum FAT32 size (2 TiB with 64K clusters).
pub const MAX_FAT32_SIZE: u64 = 2 * 1024 * 1024 * 1024 * 1024; // 2 TiB

/// Cluster size options for FAT32.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClusterSize {
    /// 512 bytes.
    Bytes512,
    /// 1 KiB (1024 bytes).
    Kib1,
    /// 2 KiB.
    Kib2,
    /// 4 KiB (default for most drives).
    Kib4,
    /// 8 KiB.
    Kib8,
    /// 16 KiB.
    Kib16,
    /// 32 KiB.
    Kib32,
    /// 64 KiB (maximum).
    Kib64,
    /// Automatic — choose based on drive size.
    Auto,
}

impl ClusterSize {
    /// Size in bytes.
    pub fn bytes(&self) -> u32 {
        match self {
            ClusterSize::Bytes512 => 512,
            ClusterSize::Kib1 => 1024,
            ClusterSize::Kib2 => 2048,
            ClusterSize::Kib4 => 4096,
            ClusterSize::Kib8 => 8192,
            ClusterSize::Kib16 => 16384,
            ClusterSize::Kib32 => 32768,
            ClusterSize::Kib64 => 65536,
            ClusterSize::Auto => 0, // Determined at format time
        }
    }

    /// Parse from a human-readable string.
    pub fn from_str_lossy(s: &str) -> Self {
        let s = s.trim().to_uppercase();
        match s.as_str() {
            "512" | "512B" => ClusterSize::Bytes512,
            "1K" | "1KB" | "1KIB" | "1024" => ClusterSize::Kib1,
            "2K" | "2KB" | "2KIB" | "2048" => ClusterSize::Kib2,
            "4K" | "4KB" | "4KIB" | "4096" => ClusterSize::Kib4,
            "8K" | "8KB" | "8KIB" | "8192" => ClusterSize::Kib8,
            "16K" | "16KB" | "16KIB" | "16384" => ClusterSize::Kib16,
            "32K" | "32KB" | "32KIB" | "32768" => ClusterSize::Kib32,
            "64K" | "64KB" | "64KIB" | "65536" => ClusterSize::Kib64,
            "AUTO" | "" | "DEFAULT" => ClusterSize::Auto,
            _ => ClusterSize::Auto,
        }
    }
}

impl fmt::Display for ClusterSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClusterSize::Bytes512 => write!(f, "512 B"),
            ClusterSize::Kib1 => write!(f, "1 KiB"),
            ClusterSize::Kib2 => write!(f, "2 KiB"),
            ClusterSize::Kib4 => write!(f, "4 KiB"),
            ClusterSize::Kib8 => write!(f, "8 KiB"),
            ClusterSize::Kib16 => write!(f, "16 KiB"),
            ClusterSize::Kib32 => write!(f, "32 KiB"),
            ClusterSize::Kib64 => write!(f, "64 KiB"),
            ClusterSize::Auto => write!(f, "Auto"),
        }
    }
}

/// FAT32 format options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fat32FormatOpts {
    /// Device path to format.
    pub device: String,
    /// Volume label (up to 11 chars).
    pub label: String,
    /// Cluster size.
    pub cluster_size: ClusterSize,
    /// Sector size (usually 512).
    pub sector_size: u32,
    /// Total size of the partition/device in bytes.
    pub total_size: u64,
    /// Quick format (zero only FATs and root dir) vs full (zero entire partition).
    pub quick: bool,
    /// Whether the device is removable media.
    pub removable: bool,
}

impl Default for Fat32FormatOpts {
    fn default() -> Self {
        Self {
            device: String::new(),
            label: "ABT_USB".to_string(),
            cluster_size: ClusterSize::Auto,
            sector_size: 512,
            total_size: 0,
            quick: true,
            removable: true,
        }
    }
}

/// Result of a FAT32 format operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fat32FormatResult {
    /// The cluster size that was used.
    pub cluster_size: u32,
    /// Number of clusters.
    pub total_clusters: u32,
    /// FAT size in sectors.
    pub fat_size_sectors: u32,
    /// Number of FATs.
    pub num_fats: u8,
    /// Reserved sectors.
    pub reserved_sectors: u16,
    /// Total data capacity (usable space) in bytes.
    pub data_capacity: u64,
    /// Volume serial number.
    pub volume_serial: u32,
}

/// Select the optimal cluster size for a given drive size.
///
/// Based on Microsoft's FAT32 specification and Rufus's defaults:
///   - < 64 MiB:      512 B
///   - < 128 MiB:     1 KiB
///   - < 256 MiB:     2 KiB
///   - < 8 GiB:       4 KiB
///   - < 16 GiB:      8 KiB
///   - < 32 GiB:     16 KiB
///   - < 2 TiB:      32 KiB  (Rufus uses 32K for >32GB, some tools use 64K)
///   - >= 2 TiB:       not supported
pub fn auto_cluster_size(total_bytes: u64) -> ClusterSize {
    let mib = total_bytes / (1024 * 1024);
    let gib = total_bytes / (1024 * 1024 * 1024);

    if total_bytes > MAX_FAT32_SIZE {
        warn!("Drive size {}B exceeds FAT32 max, using 64K clusters", total_bytes);
        return ClusterSize::Kib64;
    }

    if mib < 64 {
        ClusterSize::Bytes512
    } else if mib < 128 {
        ClusterSize::Kib1
    } else if mib < 256 {
        ClusterSize::Kib2
    } else if gib < 8 {
        ClusterSize::Kib4
    } else if gib < 16 {
        ClusterSize::Kib8
    } else if gib < 32 {
        ClusterSize::Kib16
    } else {
        ClusterSize::Kib32
    }
}

/// List available cluster sizes for a given device size.
pub fn available_cluster_sizes(total_bytes: u64) -> Vec<(ClusterSize, bool)> {
    let auto = auto_cluster_size(total_bytes);
    let sizes = [
        ClusterSize::Bytes512,
        ClusterSize::Kib1,
        ClusterSize::Kib2,
        ClusterSize::Kib4,
        ClusterSize::Kib8,
        ClusterSize::Kib16,
        ClusterSize::Kib32,
        ClusterSize::Kib64,
    ];

    sizes
        .iter()
        .filter(|&&cs| {
            let bytes = cs.bytes() as u64;
            // Cluster must be >= sector size and produce < 2^28 clusters
            let clusters = total_bytes / bytes;
            clusters >= 65525 && clusters < 0x0FFFFFF7 && bytes >= 512
        })
        .map(|&cs| (cs, cs == auto))
        .collect()
}

/// Calculate FAT32 filesystem parameters.
pub fn calculate_fat32_params(opts: &Fat32FormatOpts) -> Result<Fat32FormatResult> {
    let total_bytes = opts.total_size;
    if total_bytes < 512 * 65525 {
        anyhow::bail!("Device too small for FAT32 (minimum ~33 MB)");
    }

    let cluster_size = if opts.cluster_size == ClusterSize::Auto {
        auto_cluster_size(total_bytes)
    } else {
        opts.cluster_size
    };

    let cluster_bytes = cluster_size.bytes();
    let sector_size = opts.sector_size;
    let sectors_per_cluster = cluster_bytes / sector_size;
    let total_sectors = (total_bytes / sector_size as u64) as u32;

    // Reserved sectors: 32 is standard for FAT32
    let reserved_sectors: u16 = 32;

    // Number of FATs: always 2
    let num_fats: u8 = 2;

    // Calculate FAT size using the formula from the FAT32 spec:
    // FATSz = (TotSec - RsvdSecCnt - RootDirSectors) / (NumFATs * 128 + SectorsPerCluster)
    // For FAT32, RootDirSectors = 0
    let tmp_val1 = total_sectors - reserved_sectors as u32;
    let tmp_val2 = (128 * num_fats as u32) + sectors_per_cluster;
    let fat_size_sectors = (tmp_val1 + tmp_val2 - 1) / tmp_val2;

    // Total data sectors
    let data_sectors = total_sectors
        - reserved_sectors as u32
        - (num_fats as u32 * fat_size_sectors);

    let total_clusters = data_sectors / sectors_per_cluster;

    // Validate: FAT32 requires >= 65525 clusters
    if total_clusters < 65525 {
        anyhow::bail!(
            "Cluster size {} too large for this drive: only {} clusters (need >= 65525)",
            cluster_size,
            total_clusters
        );
    }

    // Generate volume serial number (based on date/time like DOS)
    let now = chrono::Utc::now();
    let volume_serial = ((now.timestamp() & 0xFFFF) as u32) << 16
        | ((now.timestamp_subsec_millis() & 0xFFFF) as u32);

    let data_capacity = total_clusters as u64 * cluster_bytes as u64;

    Ok(Fat32FormatResult {
        cluster_size: cluster_bytes,
        total_clusters,
        fat_size_sectors,
        num_fats,
        reserved_sectors,
        data_capacity,
        volume_serial,
    })
}

/// Build a FAT32 boot sector (512 bytes).
pub fn build_boot_sector(opts: &Fat32FormatOpts, params: &Fat32FormatResult) -> Vec<u8> {
    let mut bs = vec![0u8; 512];

    let cluster_bytes = params.cluster_size;
    let sector_size = opts.sector_size;
    let sectors_per_cluster = (cluster_bytes / sector_size) as u8;
    let total_sectors = (opts.total_size / sector_size as u64) as u32;

    // Jump instruction
    bs[0] = 0xEB;
    bs[1] = 0x58;
    bs[2] = 0x90;

    // OEM Name
    bs[3..11].copy_from_slice(BS_OEM_NAME);

    // BPB (BIOS Parameter Block)
    bs[11..13].copy_from_slice(&(sector_size as u16).to_le_bytes());   // BytsPerSec
    bs[13] = sectors_per_cluster;                                       // SecPerClus
    bs[14..16].copy_from_slice(&params.reserved_sectors.to_le_bytes()); // RsvdSecCnt
    bs[16] = params.num_fats;                                           // NumFATs
    bs[17..19].copy_from_slice(&0u16.to_le_bytes());                   // RootEntCnt (0 for FAT32)
    bs[19..21].copy_from_slice(&0u16.to_le_bytes());                   // TotSec16 (0 for FAT32)
    bs[21] = if opts.removable { FAT_MEDIA_REMOVABLE } else { FAT_MEDIA_FIXED };
    bs[22..24].copy_from_slice(&0u16.to_le_bytes());                   // FATSz16 (0 for FAT32)
    bs[24..26].copy_from_slice(&63u16.to_le_bytes());                  // SecPerTrk
    bs[26..28].copy_from_slice(&255u16.to_le_bytes());                 // NumHeads
    bs[28..32].copy_from_slice(&0u32.to_le_bytes());                   // HiddSec
    bs[32..36].copy_from_slice(&total_sectors.to_le_bytes());          // TotSec32

    // FAT32-specific BPB
    bs[36..40].copy_from_slice(&params.fat_size_sectors.to_le_bytes()); // FATSz32
    bs[40..42].copy_from_slice(&0u16.to_le_bytes());                   // ExtFlags
    bs[42..44].copy_from_slice(&0u16.to_le_bytes());                   // FSVer (0.0)
    bs[44..48].copy_from_slice(&2u32.to_le_bytes());                   // RootClus (cluster 2)
    bs[48..50].copy_from_slice(&1u16.to_le_bytes());                   // FSInfo (sector 1)
    bs[50..52].copy_from_slice(&6u16.to_le_bytes());                   // BkBootSec (sector 6)
    // bs[52..64] reserved (already zeroed)

    // Extended boot record
    bs[64] = 0x80; // DrvNum
    bs[65] = 0x00; // Reserved1
    bs[66] = 0x29; // BootSig
    bs[67..71].copy_from_slice(&params.volume_serial.to_le_bytes());   // VolID

    // Volume label (11 bytes, padded with spaces)
    let mut label_bytes = [b' '; 11];
    let label = opts.label.as_bytes();
    let copy_len = label.len().min(11);
    label_bytes[..copy_len].copy_from_slice(&label[..copy_len]);
    bs[71..82].copy_from_slice(&label_bytes);

    // File system type
    bs[82..90].copy_from_slice(b"FAT32   ");

    // Signature
    bs[510..512].copy_from_slice(&FAT32_SIGNATURE.to_le_bytes());

    bs
}

/// Build a FAT32 FSInfo sector (512 bytes).
pub fn build_fsinfo_sector(params: &Fat32FormatResult) -> Vec<u8> {
    let mut fs = vec![0u8; 512];

    fs[0..4].copy_from_slice(&FAT32_FSINFO_SIGNATURE_1.to_le_bytes());
    // bytes 4..484 reserved
    fs[484..488].copy_from_slice(&FAT32_FSINFO_SIGNATURE_2.to_le_bytes());
    fs[488..492].copy_from_slice(&(params.total_clusters - 1).to_le_bytes()); // Free cluster count
    fs[492..496].copy_from_slice(&3u32.to_le_bytes()); // Next free cluster hint
    // bytes 496..508 reserved
    fs[508..512].copy_from_slice(&FAT32_FSINFO_SIGNATURE_3.to_le_bytes());

    fs
}

/// Build the initial FAT table (first sector contains reserved entries).
pub fn build_fat_initial_sector(media_byte: u8) -> Vec<u8> {
    let mut fat = vec![0u8; 512];

    // First two FAT entries are reserved
    // Entry 0: media byte | 0x0FFFFF00
    let entry0 = 0x0FFFFF00u32 | media_byte as u32;
    fat[0..4].copy_from_slice(&entry0.to_le_bytes());

    // Entry 1: end-of-chain marker
    fat[4..8].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());

    // Entry 2: end-of-chain for root directory cluster
    fat[8..12].copy_from_slice(&0x0FFFFFFFu32.to_le_bytes());

    fat
}

/// Validate that a drive can be formatted as FAT32 with the given options.
pub fn validate_fat32_format(opts: &Fat32FormatOpts) -> Result<()> {
    if opts.total_size == 0 {
        anyhow::bail!("Device size is 0 — cannot format");
    }

    if opts.total_size > MAX_FAT32_SIZE {
        anyhow::bail!(
            "Device size ({} bytes) exceeds FAT32 maximum (2 TiB)",
            opts.total_size
        );
    }

    if opts.total_size < 33 * 1024 * 1024 {
        anyhow::bail!(
            "Device too small for FAT32 (need at least 33 MiB, have {} bytes)",
            opts.total_size
        );
    }

    if opts.label.len() > 11 {
        anyhow::bail!("Volume label too long (max 11 characters)");
    }

    // Validate label contains only valid characters
    for c in opts.label.chars() {
        if !c.is_ascii_alphanumeric() && c != '_' && c != '-' && c != ' ' {
            anyhow::bail!(
                "Invalid character '{}' in volume label (use A-Z, 0-9, _, -, or space)",
                c
            );
        }
    }

    if opts.cluster_size != ClusterSize::Auto {
        let cs = opts.cluster_size.bytes() as u64;
        let clusters = opts.total_size / cs;
        if clusters < 65525 {
            anyhow::bail!(
                "Cluster size {} is too large for this drive ({} clusters, need >= 65525)",
                opts.cluster_size,
                clusters
            );
        }
        if clusters >= 0x0FFFFFF7 {
            anyhow::bail!(
                "Cluster size {} is too small for this drive ({} clusters, max ~268M)",
                opts.cluster_size,
                clusters
            );
        }
    }

    Ok(())
}

/// Format a file (for testing) or device with FAT32.
///
/// This writes the boot sector, FSInfo, backup boot sector, and FAT tables.
/// For full format, it also zeros the data area.
pub fn format_fat32<W: Write + Seek>(
    writer: &mut W,
    opts: &Fat32FormatOpts,
    progress_cb: &mut dyn FnMut(u64, u64),
) -> Result<Fat32FormatResult> {
    validate_fat32_format(opts)?;

    let params = calculate_fat32_params(opts)?;

    info!(
        "Formatting as FAT32: cluster_size={}, clusters={}, FAT_sectors={}",
        params.cluster_size, params.total_clusters, params.fat_size_sectors
    );

    let media = if opts.removable { FAT_MEDIA_REMOVABLE } else { FAT_MEDIA_FIXED };

    // Write boot sector at sector 0
    let boot_sector = build_boot_sector(opts, &params);
    writer.seek(SeekFrom::Start(0))?;
    writer.write_all(&boot_sector)?;

    // Write FSInfo at sector 1
    let fsinfo = build_fsinfo_sector(&params);
    writer.seek(SeekFrom::Start(opts.sector_size as u64))?;
    writer.write_all(&fsinfo)?;

    // Write backup boot sector at sector 6
    writer.seek(SeekFrom::Start(6 * opts.sector_size as u64))?;
    writer.write_all(&boot_sector)?;

    // Write backup FSInfo at sector 7
    writer.seek(SeekFrom::Start(7 * opts.sector_size as u64))?;
    writer.write_all(&fsinfo)?;

    // Write FAT tables
    let fat_initial = build_fat_initial_sector(media);
    let fat_zero = vec![0u8; opts.sector_size as usize];

    let total_work = if opts.quick {
        // Quick: only FATs
        (params.num_fats as u64) * (params.fat_size_sectors as u64) * opts.sector_size as u64
    } else {
        opts.total_size
    };

    let mut progress: u64 = 0;

    for fat_num in 0..params.num_fats {
        let fat_start = params.reserved_sectors as u64
            + (fat_num as u64 * params.fat_size_sectors as u64);

        for sector in 0..params.fat_size_sectors {
            let offset = (fat_start + sector as u64) * opts.sector_size as u64;
            writer.seek(SeekFrom::Start(offset))?;

            if sector == 0 {
                writer.write_all(&fat_initial)?;
            } else {
                writer.write_all(&fat_zero)?;
            }

            progress += opts.sector_size as u64;
            progress_cb(progress, total_work);
        }
    }

    // Full format: zero the data area
    if !opts.quick {
        let data_start = (params.reserved_sectors as u64
            + params.num_fats as u64 * params.fat_size_sectors as u64)
            * opts.sector_size as u64;

        let zero_buf = vec![0u8; 65536]; // 64K zero buffer
        let mut pos = data_start;

        while pos < opts.total_size {
            let write_len = zero_buf.len().min((opts.total_size - pos) as usize);
            writer.seek(SeekFrom::Start(pos))?;
            writer.write_all(&zero_buf[..write_len])?;
            pos += write_len as u64;
            progress += write_len as u64;
            progress_cb(progress, total_work);
        }
    }

    writer.flush()?;

    info!(
        "FAT32 format complete: {} data capacity, serial {:08X}",
        humanize_size(params.data_capacity),
        params.volume_serial
    );

    Ok(params)
}

fn humanize_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    for &unit in UNITS {
        if size < 1024.0 {
            return format!("{:.1} {}", size, unit);
        }
        size /= 1024.0;
    }
    format!("{:.1} PiB", size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_auto_cluster_size() {
        assert_eq!(auto_cluster_size(50 * 1024 * 1024), ClusterSize::Bytes512);
        assert_eq!(auto_cluster_size(100 * 1024 * 1024), ClusterSize::Kib1);
        assert_eq!(auto_cluster_size(200 * 1024 * 1024), ClusterSize::Kib2);
        assert_eq!(auto_cluster_size(4 * 1024 * 1024 * 1024), ClusterSize::Kib4);
        assert_eq!(auto_cluster_size(10 * 1024 * 1024 * 1024), ClusterSize::Kib8);
        assert_eq!(auto_cluster_size(20 * 1024 * 1024 * 1024), ClusterSize::Kib16);
        assert_eq!(auto_cluster_size(64 * 1024 * 1024 * 1024), ClusterSize::Kib32);
    }

    #[test]
    fn test_cluster_size_bytes() {
        assert_eq!(ClusterSize::Bytes512.bytes(), 512);
        assert_eq!(ClusterSize::Kib4.bytes(), 4096);
        assert_eq!(ClusterSize::Kib64.bytes(), 65536);
        assert_eq!(ClusterSize::Auto.bytes(), 0);
    }

    #[test]
    fn test_cluster_size_from_str() {
        assert_eq!(ClusterSize::from_str_lossy("4K"), ClusterSize::Kib4);
        assert_eq!(ClusterSize::from_str_lossy("4KB"), ClusterSize::Kib4);
        assert_eq!(ClusterSize::from_str_lossy("4096"), ClusterSize::Kib4);
        assert_eq!(ClusterSize::from_str_lossy("512"), ClusterSize::Bytes512);
        assert_eq!(ClusterSize::from_str_lossy("64K"), ClusterSize::Kib64);
        assert_eq!(ClusterSize::from_str_lossy("auto"), ClusterSize::Auto);
        assert_eq!(ClusterSize::from_str_lossy("unknown"), ClusterSize::Auto);
    }

    #[test]
    fn test_cluster_size_display() {
        assert_eq!(format!("{}", ClusterSize::Kib4), "4 KiB");
        assert_eq!(format!("{}", ClusterSize::Kib64), "64 KiB");
        assert_eq!(format!("{}", ClusterSize::Bytes512), "512 B");
    }

    #[test]
    fn test_validate_fat32_format_ok() {
        let opts = Fat32FormatOpts {
            device: "/dev/sdb".into(),
            label: "TEST".into(),
            cluster_size: ClusterSize::Auto,
            sector_size: 512,
            total_size: 64 * 1024 * 1024 * 1024, // 64 GB
            quick: true,
            removable: true,
        };
        assert!(validate_fat32_format(&opts).is_ok());
    }

    #[test]
    fn test_validate_fat32_format_too_small() {
        let opts = Fat32FormatOpts {
            total_size: 10 * 1024 * 1024, // 10 MB
            ..Default::default()
        };
        assert!(validate_fat32_format(&opts).is_err());
    }

    #[test]
    fn test_validate_fat32_format_too_large() {
        let opts = Fat32FormatOpts {
            total_size: 3 * 1024 * 1024 * 1024 * 1024, // 3 TiB
            ..Default::default()
        };
        assert!(validate_fat32_format(&opts).is_err());
    }

    #[test]
    fn test_validate_fat32_label_too_long() {
        let opts = Fat32FormatOpts {
            total_size: 64 * 1024 * 1024 * 1024,
            label: "ABCDEFGHIJKL".into(), // 12 chars, max is 11
            ..Default::default()
        };
        assert!(validate_fat32_format(&opts).is_err());
    }

    #[test]
    fn test_validate_fat32_label_invalid_char() {
        let opts = Fat32FormatOpts {
            total_size: 64 * 1024 * 1024 * 1024,
            label: "ABC!@#".into(),
            ..Default::default()
        };
        assert!(validate_fat32_format(&opts).is_err());
    }

    #[test]
    fn test_calculate_fat32_params() {
        let opts = Fat32FormatOpts {
            device: "/dev/sdb".into(),
            label: "TEST".into(),
            cluster_size: ClusterSize::Kib32,
            sector_size: 512,
            total_size: 64 * 1024 * 1024 * 1024, // 64 GB
            quick: true,
            removable: true,
        };
        let params = calculate_fat32_params(&opts).unwrap();
        assert_eq!(params.cluster_size, 32768);
        assert!(params.total_clusters >= 65525);
        assert_eq!(params.num_fats, 2);
        assert_eq!(params.reserved_sectors, 32);
        assert!(params.data_capacity > 0);
    }

    #[test]
    fn test_build_boot_sector() {
        let opts = Fat32FormatOpts {
            device: "test".into(),
            label: "MYUSB".into(),
            cluster_size: ClusterSize::Kib4,
            sector_size: 512,
            total_size: 1024 * 1024 * 1024, // 1 GB
            quick: true,
            removable: true,
        };
        let params = calculate_fat32_params(&opts).unwrap();
        let bs = build_boot_sector(&opts, &params);

        assert_eq!(bs.len(), 512);
        // Check jump instruction
        assert_eq!(bs[0], 0xEB);
        // Check OEM name
        assert_eq!(&bs[3..11], BS_OEM_NAME);
        // Check signature
        assert_eq!(&bs[510..512], &FAT32_SIGNATURE.to_le_bytes());
        // Check filesystem type string
        assert_eq!(&bs[82..90], b"FAT32   ");
        // Check sector size
        assert_eq!(u16::from_le_bytes([bs[11], bs[12]]), 512);
    }

    #[test]
    fn test_build_fsinfo_sector() {
        let params = Fat32FormatResult {
            cluster_size: 4096,
            total_clusters: 200000,
            fat_size_sectors: 1600,
            num_fats: 2,
            reserved_sectors: 32,
            data_capacity: 200000 * 4096,
            volume_serial: 0x12345678,
        };
        let fs = build_fsinfo_sector(&params);

        assert_eq!(fs.len(), 512);
        // Check signatures
        assert_eq!(
            u32::from_le_bytes([fs[0], fs[1], fs[2], fs[3]]),
            FAT32_FSINFO_SIGNATURE_1
        );
        assert_eq!(
            u32::from_le_bytes([fs[484], fs[485], fs[486], fs[487]]),
            FAT32_FSINFO_SIGNATURE_2
        );
    }

    #[test]
    fn test_build_fat_initial_sector() {
        let fat = build_fat_initial_sector(FAT_MEDIA_REMOVABLE);
        assert_eq!(fat.len(), 512);

        let entry0 = u32::from_le_bytes([fat[0], fat[1], fat[2], fat[3]]);
        assert_eq!(entry0, 0x0FFFFF00 | FAT_MEDIA_REMOVABLE as u32);

        let entry1 = u32::from_le_bytes([fat[4], fat[5], fat[6], fat[7]]);
        assert_eq!(entry1, 0x0FFFFFFF);

        let entry2 = u32::from_le_bytes([fat[8], fat[9], fat[10], fat[11]]);
        assert_eq!(entry2, 0x0FFFFFFF);
    }

    #[test]
    fn test_format_fat32_small_image() {
        // Create a 34 MB in-memory "device"
        let size: u64 = 34 * 1024 * 1024;
        let mut buf = vec![0u8; size as usize];
        let mut cursor = Cursor::new(&mut buf[..]);

        let opts = Fat32FormatOpts {
            device: "memory".into(),
            label: "TEST".into(),
            cluster_size: ClusterSize::Bytes512,
            sector_size: 512,
            total_size: size,
            quick: true,
            removable: true,
        };

        let mut progress_calls = 0u32;
        let result = format_fat32(&mut cursor, &opts, &mut |_, _| {
            progress_calls += 1;
        });

        assert!(result.is_ok());
        let params = result.unwrap();
        assert_eq!(params.cluster_size, 512);
        assert!(params.total_clusters >= 65525);
        assert!(progress_calls > 0);

        // Verify boot sector signature
        assert_eq!(buf[510], 0x55);
        assert_eq!(buf[511], 0xAA);
    }

    #[test]
    fn test_available_cluster_sizes() {
        let sizes = available_cluster_sizes(64 * 1024 * 1024 * 1024); // 64 GB
        assert!(!sizes.is_empty());
        // Should have a recommended size
        assert!(sizes.iter().any(|(_, recommended)| *recommended));
    }

    #[test]
    fn test_max_fat32_size() {
        assert_eq!(MAX_FAT32_SIZE, 2 * 1024 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_fat32_format_result_serde() {
        let result = Fat32FormatResult {
            cluster_size: 4096,
            total_clusters: 100000,
            fat_size_sectors: 800,
            num_fats: 2,
            reserved_sectors: 32,
            data_capacity: 100000 * 4096,
            volume_serial: 0xDEADBEEF,
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: Fat32FormatResult = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.cluster_size, 4096);
        assert_eq!(parsed.volume_serial, 0xDEADBEEF);
    }

    #[test]
    fn test_fat32_format_opts_default() {
        let opts = Fat32FormatOpts::default();
        assert_eq!(opts.label, "ABT_USB");
        assert_eq!(opts.cluster_size, ClusterSize::Auto);
        assert_eq!(opts.sector_size, 512);
        assert!(opts.quick);
        assert!(opts.removable);
    }

    #[test]
    fn test_humanize_size() {
        assert_eq!(humanize_size(0), "0.0 B");
        assert_eq!(humanize_size(1024), "1.0 KiB");
        assert_eq!(humanize_size(1024 * 1024), "1.0 MiB");
        assert_eq!(humanize_size(1024 * 1024 * 1024), "1.0 GiB");
    }
}
