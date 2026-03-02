// Filesystem detection — identify filesystem type from superblock magic bytes.
// Reads raw device/partition headers to detect FAT12/16/32, exFAT, NTFS, ReFS,
// ext2/3/4, XFS, Btrfs, UDF, ISO9660, and other filesystem types without mounting.
// Inspired by Rufus's GetFsName() drive detection logic.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Recognized filesystem types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FsType {
    /// FAT12 (floppy disks, small media).
    Fat12,
    /// FAT16 (older USB drives).
    Fat16,
    /// FAT32.
    Fat32,
    /// exFAT (modern removable media).
    ExFat,
    /// NTFS (Windows default).
    Ntfs,
    /// ReFS (Windows Resilient File System).
    ReFs,
    /// ext2 (Linux, no journal).
    Ext2,
    /// ext3 (Linux, with journal).
    Ext3,
    /// ext4 (modern Linux).
    Ext4,
    /// XFS (Linux high-performance).
    Xfs,
    /// Btrfs (Linux CoW filesystem).
    Btrfs,
    /// UDF (Universal Disk Format, optical media).
    Udf,
    /// ISO 9660 (CD/DVD filesystem).
    Iso9660,
    /// HFS+ (macOS legacy).
    HfsPlus,
    /// APFS (modern macOS).
    Apfs,
    /// swap (Linux swap partition).
    LinuxSwap,
    /// Unknown or unrecognized filesystem.
    Unknown,
}

impl fmt::Display for FsType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fat12 => write!(f, "FAT12"),
            Self::Fat16 => write!(f, "FAT16"),
            Self::Fat32 => write!(f, "FAT32"),
            Self::ExFat => write!(f, "exFAT"),
            Self::Ntfs => write!(f, "NTFS"),
            Self::ReFs => write!(f, "ReFS"),
            Self::Ext2 => write!(f, "ext2"),
            Self::Ext3 => write!(f, "ext3"),
            Self::Ext4 => write!(f, "ext4"),
            Self::Xfs => write!(f, "XFS"),
            Self::Btrfs => write!(f, "Btrfs"),
            Self::Udf => write!(f, "UDF"),
            Self::Iso9660 => write!(f, "ISO 9660"),
            Self::HfsPlus => write!(f, "HFS+"),
            Self::Apfs => write!(f, "APFS"),
            Self::LinuxSwap => write!(f, "Linux swap"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

impl FsType {
    /// Whether this filesystem is typically writable.
    pub fn is_writable(&self) -> bool {
        !matches!(self, Self::Iso9660 | Self::Udf | Self::Unknown)
    }

    /// Whether this filesystem is a Windows native format.
    pub fn is_windows_native(&self) -> bool {
        matches!(
            self,
            Self::Fat12 | Self::Fat16 | Self::Fat32 | Self::ExFat | Self::Ntfs | Self::ReFs
        )
    }

    /// Whether this filesystem is a Linux native format.
    pub fn is_linux_native(&self) -> bool {
        matches!(
            self,
            Self::Ext2 | Self::Ext3 | Self::Ext4 | Self::Xfs | Self::Btrfs | Self::LinuxSwap
        )
    }

    /// Whether this filesystem is a macOS native format.
    pub fn is_macos_native(&self) -> bool {
        matches!(self, Self::HfsPlus | Self::Apfs)
    }

    /// Whether this is a FAT variant.
    pub fn is_fat(&self) -> bool {
        matches!(self, Self::Fat12 | Self::Fat16 | Self::Fat32)
    }

    /// Whether this is a Linux ext variant.
    pub fn is_ext(&self) -> bool {
        matches!(self, Self::Ext2 | Self::Ext3 | Self::Ext4)
    }

    /// Maximum file size supported (approximate, None if variable or unlimited).
    pub fn max_file_size(&self) -> Option<u64> {
        match self {
            Self::Fat12 => Some(32 * 1024 * 1024),            // 32 MiB practical
            Self::Fat16 => Some(2 * 1024 * 1024 * 1024),      // 2 GiB
            Self::Fat32 => Some(4 * 1024 * 1024 * 1024 - 1),  // 4 GiB - 1
            Self::ExFat => Some(16 * 1024 * 1024 * 1024 * 1024), // 16 EiB theoretical
            _ => None,
        }
    }
}

/// Result of filesystem detection on a device or partition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsDetectResult {
    /// Detected filesystem type.
    pub fs_type: FsType,
    /// OEM name / volume system ID string (if found in superblock).
    pub oem_name: Option<String>,
    /// Volume label (if found).
    pub volume_label: Option<String>,
    /// Volume serial / UUID (hex string if found).
    pub volume_id: Option<String>,
    /// Cluster/block size in bytes.
    pub block_size: Option<u32>,
    /// Total volume size in bytes (from superblock).
    pub total_size: Option<u64>,
    /// Confidence level (0.0 - 1.0).
    pub confidence: f64,
    /// Raw magic bytes detected (hex-encoded).
    pub magic_hex: String,
}

impl fmt::Display for FsDetectResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.fs_type)?;
        if let Some(ref label) = self.volume_label {
            write!(f, " \"{}\"", label.trim())?;
        }
        if let Some(ref oem) = self.oem_name {
            write!(f, " (OEM: {})", oem.trim())?;
        }
        if let Some(bs) = self.block_size {
            write!(f, " [block={}]", bs)?;
        }
        Ok(())
    }
}

// Superblock magic offsets and values for detection.

/// Minimum bytes needed for detection (covers ext superblock at 0x438).
const MIN_DETECT_BYTES: usize = 0x10400; // ~65 KiB covers UDF BEA01 at 0xC001

/// FAT boot sector OEM name offset.
const FAT_OEM_OFFSET: usize = 0x03;
/// FAT boot sector OEM name length.
const FAT_OEM_LEN: usize = 8;

/// exFAT magic: "EXFAT   " at offset 3.
const EXFAT_MAGIC: &[u8; 8] = b"EXFAT   ";
/// NTFS magic: "NTFS    " at offset 3.
const NTFS_MAGIC: &[u8; 8] = b"NTFS    ";
/// ReFS magic: "ReFS\0\0\0\0" at offset 3.
const REFS_MAGIC: &[u8] = b"ReFS";

/// ext2/3/4 superblock magic at offset 0x438 (1080 bytes in).
const EXT_SUPER_MAGIC_OFFSET: usize = 0x438;
const EXT_SUPER_MAGIC: u16 = 0xEF53;

/// XFS superblock magic "XFSB" at offset 0.
const XFS_MAGIC: &[u8; 4] = b"XFSB";

/// Btrfs superblock magic "_BHRfS_M" at offset 0x10040 (64 KiB + 64 bytes).
const BTRFS_MAGIC_OFFSET: usize = 0x10040;
const BTRFS_MAGIC: &[u8; 8] = b"_BHRfS_M";

/// ISO 9660 magic "CD001" at offset 0x8001 (32 KiB + 1).
const ISO9660_MAGIC_OFFSET: usize = 0x8001;
const ISO9660_MAGIC: &[u8; 5] = b"CD001";

/// UDF BEA01 detection at offset 0xC001 (48 KiB + 1).
const UDF_BEA_OFFSET: usize = 0xC001;
const UDF_BEA_MAGIC: &[u8; 5] = b"BEA01";

/// FAT filesystem type string at offset 0x36 (FAT12/16) or 0x52 (FAT32).
const FAT16_FSTYPE_OFFSET: usize = 0x36;
const FAT32_FSTYPE_OFFSET: usize = 0x52;

/// HFS+ magic at offset 0x400.
const HFSPLUS_MAGIC_OFFSET: usize = 0x400;
const HFSPLUS_MAGIC: &[u8; 2] = b"H+";
const HFSX_MAGIC: &[u8; 2] = b"HX";

/// Linux swap magic at offset 0xFF6 (4086).
const SWAP_MAGIC_OFFSET_4K: usize = 0xFF6;
const SWAP_MAGIC: &[u8; 10] = b"SWAPSPACE2";
const SWAP_MAGIC_OLD: &[u8; 10] = b"SWAP-SPACE";

/// Detect the filesystem type by reading raw bytes from a seekable reader.
///
/// The reader should be positioned at the start of the partition/device.
/// Reads up to ~65 KiB to cover all superblock locations.
pub fn detect_filesystem<R: Read + Seek>(reader: &mut R) -> Result<FsDetectResult> {
    let start_pos = reader.stream_position()?;

    // Read enough bytes for all superblock checks
    let mut buf = vec![0u8; MIN_DETECT_BYTES];
    let bytes_read = read_at_most(reader, &mut buf)?;
    if bytes_read < 512 {
        return Err(anyhow!(
            "Not enough data to detect filesystem (read {} bytes, need at least 512)",
            bytes_read
        ));
    }

    // Seek back to start
    reader.seek(SeekFrom::Start(start_pos))?;

    // Try detection in order of specificity
    if let Some(result) = try_detect_exfat(&buf, bytes_read) {
        return Ok(result);
    }
    if let Some(result) = try_detect_ntfs(&buf, bytes_read) {
        return Ok(result);
    }
    if let Some(result) = try_detect_refs(&buf, bytes_read) {
        return Ok(result);
    }
    if let Some(result) = try_detect_fat(&buf, bytes_read) {
        return Ok(result);
    }
    if bytes_read > EXT_SUPER_MAGIC_OFFSET + 2 {
        if let Some(result) = try_detect_ext(&buf, bytes_read) {
            return Ok(result);
        }
    }
    if let Some(result) = try_detect_xfs(&buf, bytes_read) {
        return Ok(result);
    }
    if bytes_read > BTRFS_MAGIC_OFFSET + 8 {
        if let Some(result) = try_detect_btrfs(&buf, bytes_read) {
            return Ok(result);
        }
    }
    if bytes_read > ISO9660_MAGIC_OFFSET + 5 {
        if let Some(result) = try_detect_iso9660(&buf, bytes_read) {
            return Ok(result);
        }
    }
    if bytes_read > UDF_BEA_OFFSET + 5 {
        if let Some(result) = try_detect_udf(&buf, bytes_read) {
            return Ok(result);
        }
    }
    if bytes_read > HFSPLUS_MAGIC_OFFSET + 2 {
        if let Some(result) = try_detect_hfsplus(&buf, bytes_read) {
            return Ok(result);
        }
    }
    if bytes_read > SWAP_MAGIC_OFFSET_4K + SWAP_MAGIC.len() {
        if let Some(result) = try_detect_swap(&buf, bytes_read) {
            return Ok(result);
        }
    }

    // Unknown
    Ok(FsDetectResult {
        fs_type: FsType::Unknown,
        oem_name: None,
        volume_label: None,
        volume_id: None,
        block_size: None,
        total_size: None,
        confidence: 0.0,
        magic_hex: hex::encode(&buf[..16.min(bytes_read)]),
    })
}

/// Detect filesystem from a file path.
pub fn detect_filesystem_path(path: &Path) -> Result<FsDetectResult> {
    let mut file = std::fs::File::open(path)?;
    detect_filesystem(&mut file)
}

/// Read as many bytes as possible, returning count actually read.
fn read_at_most<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut total = 0;
    loop {
        match reader.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                if total >= buf.len() {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(total)
}

/// Extract a trimmed ASCII string from a byte slice.
fn extract_ascii_string(buf: &[u8], offset: usize, len: usize) -> Option<String> {
    if offset + len > buf.len() {
        return None;
    }
    let slice = &buf[offset..offset + len];
    let s: String = slice
        .iter()
        .map(|&b| if b >= 0x20 && b < 0x7F { b as char } else { ' ' })
        .collect();
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Read a little-endian u16 from buffer.
fn read_le_u16(buf: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([buf[offset], buf[offset + 1]])
}

/// Read a little-endian u32 from buffer.
fn read_le_u32(buf: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
}

/// Read a big-endian u32 from buffer.
fn read_be_u32(buf: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([buf[offset], buf[offset + 1], buf[offset + 2], buf[offset + 3]])
}

// --- Individual filesystem detectors ---

fn try_detect_exfat(buf: &[u8], len: usize) -> Option<FsDetectResult> {
    if len < 512 {
        return None;
    }
    if &buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 8] == EXFAT_MAGIC {
        let sector_size_shift = buf[108] as u32;
        let cluster_shift = buf[109] as u32;
        let sector_size = 1u32 << sector_size_shift;
        let cluster_size = sector_size << cluster_shift;
        let total_sectors = u64::from_le_bytes([
            buf[72], buf[73], buf[74], buf[75], buf[76], buf[77], buf[78], buf[79],
        ]);
        let serial = read_le_u32(buf, 100);
        Some(FsDetectResult {
            fs_type: FsType::ExFat,
            oem_name: Some("EXFAT".into()),
            volume_label: None, // exFAT label is in directory entry, not superblock
            volume_id: Some(format!("{:08X}", serial)),
            block_size: Some(cluster_size),
            total_size: Some(total_sectors * sector_size as u64),
            confidence: 0.95,
            magic_hex: hex::encode(&buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 8]),
        })
    } else {
        None
    }
}

fn try_detect_ntfs(buf: &[u8], len: usize) -> Option<FsDetectResult> {
    if len < 512 {
        return None;
    }
    if &buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 8] == NTFS_MAGIC {
        let bytes_per_sector = read_le_u16(buf, 0x0B) as u32;
        let sectors_per_cluster = buf[0x0D] as u32;
        let cluster_size = bytes_per_sector * sectors_per_cluster;
        let total_sectors = u64::from_le_bytes([
            buf[0x28], buf[0x29], buf[0x2A], buf[0x2B], buf[0x2C], buf[0x2D], buf[0x2E],
            buf[0x2F],
        ]);
        let serial = u64::from_le_bytes([
            buf[0x48], buf[0x49], buf[0x4A], buf[0x4B], buf[0x4C], buf[0x4D], buf[0x4E],
            buf[0x4F],
        ]);
        Some(FsDetectResult {
            fs_type: FsType::Ntfs,
            oem_name: Some("NTFS".into()),
            volume_label: None, // NTFS label is in MFT, not BPB
            volume_id: Some(format!("{:016X}", serial)),
            block_size: Some(cluster_size),
            total_size: Some(total_sectors * bytes_per_sector as u64),
            confidence: 0.95,
            magic_hex: hex::encode(&buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 8]),
        })
    } else {
        None
    }
}

fn try_detect_refs(buf: &[u8], len: usize) -> Option<FsDetectResult> {
    if len < 512 {
        return None;
    }
    if &buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 4] == REFS_MAGIC
        && buf[FAT_OEM_OFFSET + 4] == 0
        && buf[FAT_OEM_OFFSET + 5] == 0
        && buf[FAT_OEM_OFFSET + 6] == 0
        && buf[FAT_OEM_OFFSET + 7] == 0
    {
        Some(FsDetectResult {
            fs_type: FsType::ReFs,
            oem_name: Some("ReFS".into()),
            volume_label: None,
            volume_id: None,
            block_size: None,
            total_size: None,
            confidence: 0.90,
            magic_hex: hex::encode(&buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 8]),
        })
    } else {
        None
    }
}

fn try_detect_fat(buf: &[u8], len: usize) -> Option<FsDetectResult> {
    if len < 512 {
        return None;
    }

    // Check media byte (must be 0xF0 or 0xF8-0xFF for valid FAT)
    let media = buf[0x15];
    if media != 0xF0 && media < 0xF8 {
        return None;
    }

    // Validate basic BPB fields
    let bytes_per_sector = read_le_u16(buf, 0x0B);
    if bytes_per_sector == 0
        || !bytes_per_sector.is_power_of_two()
        || bytes_per_sector < 512
        || bytes_per_sector > 4096
    {
        return None;
    }

    let sectors_per_cluster = buf[0x0D];
    if sectors_per_cluster == 0 || !sectors_per_cluster.is_power_of_two() {
        return None;
    }

    let total_sectors_16 = read_le_u16(buf, 0x13);
    let total_sectors_32 = read_le_u32(buf, 0x20);
    let fat_size_16 = read_le_u16(buf, 0x16);
    let fat_size_32 = read_le_u32(buf, 0x24);
    let root_entry_count = read_le_u16(buf, 0x11);
    let reserved_sectors = read_le_u16(buf, 0x0E);
    let num_fats = buf[0x10];

    if reserved_sectors == 0 || num_fats == 0 {
        return None;
    }

    let total_sectors = if total_sectors_16 != 0 {
        total_sectors_16 as u64
    } else {
        total_sectors_32 as u64
    };

    let fat_size = if fat_size_16 != 0 {
        fat_size_16 as u64
    } else {
        fat_size_32 as u64
    };

    let root_dir_sectors =
        ((root_entry_count as u64 * 32) + (bytes_per_sector as u64 - 1)) / bytes_per_sector as u64;
    let data_sectors = total_sectors
        .saturating_sub(reserved_sectors as u64)
        .saturating_sub(num_fats as u64 * fat_size)
        .saturating_sub(root_dir_sectors);
    let total_clusters = data_sectors / sectors_per_cluster as u64;

    // Determine FAT variant by cluster count (per Microsoft spec)
    let (fs_type, fs_type_offset) = if total_clusters < 4085 {
        (FsType::Fat12, FAT16_FSTYPE_OFFSET)
    } else if total_clusters < 65525 {
        (FsType::Fat16, FAT16_FSTYPE_OFFSET)
    } else {
        (FsType::Fat32, FAT32_FSTYPE_OFFSET)
    };

    let oem_name = extract_ascii_string(buf, FAT_OEM_OFFSET, FAT_OEM_LEN);

    // Volume label and serial from Extended BIOS Parameter Block
    let (label_offset, serial_offset) = if fs_type == FsType::Fat32 {
        (0x47usize, 0x43usize)
    } else {
        (0x2B, 0x27)
    };
    let volume_label = extract_ascii_string(buf, label_offset, 11);
    let serial = read_le_u32(buf, serial_offset);

    // Check filesystem type string for confirmation
    let fs_type_str = extract_ascii_string(buf, fs_type_offset, 8);
    let confidence = if fs_type_str
        .as_ref()
        .map(|s| s.starts_with("FAT"))
        .unwrap_or(false)
    {
        0.95
    } else {
        0.75
    };

    let cluster_size = bytes_per_sector as u32 * sectors_per_cluster as u32;

    Some(FsDetectResult {
        fs_type,
        oem_name,
        volume_label,
        volume_id: Some(format!("{:08X}", serial)),
        block_size: Some(cluster_size),
        total_size: Some(total_sectors * bytes_per_sector as u64),
        confidence,
        magic_hex: hex::encode(&buf[..16]),
    })
}

fn try_detect_ext(buf: &[u8], _len: usize) -> Option<FsDetectResult> {
    let magic = read_le_u16(buf, EXT_SUPER_MAGIC_OFFSET);
    if magic != EXT_SUPER_MAGIC {
        return None;
    }

    // ext superblock starts at 1024 bytes (offset 0x400)
    let sb = 0x400;
    let s_log_block_size = read_le_u32(buf, sb + 0x18);
    let block_size = 1024u32 << s_log_block_size;
    let s_blocks_count_lo = read_le_u32(buf, sb + 0x04);
    let s_blocks_count_hi = read_le_u32(buf, sb + 0x150);
    let total_blocks = (s_blocks_count_hi as u64) << 32 | s_blocks_count_lo as u64;
    let total_size = total_blocks * block_size as u64;

    // Feature flags determine ext2/3/4
    let s_feature_compat = read_le_u32(buf, sb + 0x5C);
    let s_feature_incompat = read_le_u32(buf, sb + 0x60);
    let s_feature_ro_compat = read_le_u32(buf, sb + 0x64);

    // ext4 indicators: extents (incompat bit 6), flex_bg (incompat bit 9), 64bit (incompat bit 7)
    let has_extents = s_feature_incompat & 0x0040 != 0;
    let has_flex_bg = s_feature_incompat & 0x0200 != 0;
    let has_64bit = s_feature_incompat & 0x0080 != 0;
    // ext3 indicator: has_journal (compat bit 2)
    let has_journal = s_feature_compat & 0x0004 != 0;
    // ext4 ro_compat indicators
    let has_huge_file = s_feature_ro_compat & 0x0008 != 0;

    let fs_type = if has_extents || has_flex_bg || has_64bit || has_huge_file {
        FsType::Ext4
    } else if has_journal {
        FsType::Ext3
    } else {
        FsType::Ext2
    };

    // Volume label at sb + 0x78, 16 bytes
    let volume_label = extract_ascii_string(buf, sb + 0x78, 16);
    // UUID at sb + 0x68, 16 bytes
    let uuid_bytes = &buf[sb + 0x68..sb + 0x78];
    let uuid_str = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        uuid_bytes[0], uuid_bytes[1], uuid_bytes[2], uuid_bytes[3],
        uuid_bytes[4], uuid_bytes[5],
        uuid_bytes[6], uuid_bytes[7],
        uuid_bytes[8], uuid_bytes[9],
        uuid_bytes[10], uuid_bytes[11], uuid_bytes[12], uuid_bytes[13], uuid_bytes[14], uuid_bytes[15],
    );

    Some(FsDetectResult {
        fs_type,
        oem_name: None,
        volume_label,
        volume_id: Some(uuid_str),
        block_size: Some(block_size),
        total_size: Some(total_size),
        confidence: 0.95,
        magic_hex: hex::encode(&buf[EXT_SUPER_MAGIC_OFFSET..EXT_SUPER_MAGIC_OFFSET + 2]),
    })
}

fn try_detect_xfs(buf: &[u8], len: usize) -> Option<FsDetectResult> {
    if len < 512 {
        return None;
    }
    if &buf[0..4] == XFS_MAGIC {
        let block_size = read_be_u32(buf, 4);
        let total_blocks = u64::from_be_bytes([
            buf[8], buf[9], buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
        ]);
        let uuid_bytes = &buf[32..48];
        let uuid_str = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            uuid_bytes[0], uuid_bytes[1], uuid_bytes[2], uuid_bytes[3],
            uuid_bytes[4], uuid_bytes[5],
            uuid_bytes[6], uuid_bytes[7],
            uuid_bytes[8], uuid_bytes[9],
            uuid_bytes[10], uuid_bytes[11], uuid_bytes[12], uuid_bytes[13], uuid_bytes[14], uuid_bytes[15],
        );
        // XFS volume label at offset 108, 12 bytes
        let volume_label = extract_ascii_string(buf, 108, 12);
        Some(FsDetectResult {
            fs_type: FsType::Xfs,
            oem_name: None,
            volume_label,
            volume_id: Some(uuid_str),
            block_size: Some(block_size),
            total_size: Some(total_blocks * block_size as u64),
            confidence: 0.95,
            magic_hex: hex::encode(&buf[0..4]),
        })
    } else {
        None
    }
}

fn try_detect_btrfs(buf: &[u8], _len: usize) -> Option<FsDetectResult> {
    if &buf[BTRFS_MAGIC_OFFSET..BTRFS_MAGIC_OFFSET + 8] == BTRFS_MAGIC {
        // Btrfs superblock at 0x10000 (64 KiB)
        let sb = 0x10000;
        let total_bytes = u64::from_le_bytes([
            buf[sb + 0x70],
            buf[sb + 0x71],
            buf[sb + 0x72],
            buf[sb + 0x73],
            buf[sb + 0x74],
            buf[sb + 0x75],
            buf[sb + 0x76],
            buf[sb + 0x77],
        ]);
        let node_size = read_le_u32(buf, sb + 0x40);
        let sector_size = read_le_u32(buf, sb + 0x44);
        // UUID at sb + 0x20, 16 bytes
        let uuid_bytes = &buf[sb + 0x20..sb + 0x30];
        let uuid_str = format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            uuid_bytes[0], uuid_bytes[1], uuid_bytes[2], uuid_bytes[3],
            uuid_bytes[4], uuid_bytes[5],
            uuid_bytes[6], uuid_bytes[7],
            uuid_bytes[8], uuid_bytes[9],
            uuid_bytes[10], uuid_bytes[11], uuid_bytes[12], uuid_bytes[13], uuid_bytes[14], uuid_bytes[15],
        );
        // Label at sb + 0x12B, 256 bytes
        let volume_label = extract_ascii_string(buf, sb + 0x12B, 256);
        Some(FsDetectResult {
            fs_type: FsType::Btrfs,
            oem_name: None,
            volume_label,
            volume_id: Some(uuid_str),
            block_size: Some(node_size.max(sector_size)),
            total_size: Some(total_bytes),
            confidence: 0.95,
            magic_hex: hex::encode(&buf[BTRFS_MAGIC_OFFSET..BTRFS_MAGIC_OFFSET + 8]),
        })
    } else {
        None
    }
}

fn try_detect_iso9660(buf: &[u8], _len: usize) -> Option<FsDetectResult> {
    if &buf[ISO9660_MAGIC_OFFSET..ISO9660_MAGIC_OFFSET + 5] == ISO9660_MAGIC {
        // Volume descriptor at 0x8000
        let vd = 0x8000;
        // System ID at vd + 8, 32 bytes
        let system_id = extract_ascii_string(buf, vd + 8, 32);
        // Volume ID at vd + 40, 32 bytes
        let volume_label = extract_ascii_string(buf, vd + 40, 32);
        // Logical block size at vd + 128, little-endian u16
        let block_size = read_le_u16(buf, vd + 128) as u32;
        // Volume space size at vd + 80, little-endian u32
        let volume_space = read_le_u32(buf, vd + 80);
        Some(FsDetectResult {
            fs_type: FsType::Iso9660,
            oem_name: system_id,
            volume_label,
            volume_id: None,
            block_size: Some(if block_size > 0 { block_size } else { 2048 }),
            total_size: Some(
                volume_space as u64 * if block_size > 0 { block_size as u64 } else { 2048 },
            ),
            confidence: 0.95,
            magic_hex: hex::encode(&buf[ISO9660_MAGIC_OFFSET..ISO9660_MAGIC_OFFSET + 5]),
        })
    } else {
        None
    }
}

fn try_detect_udf(buf: &[u8], _len: usize) -> Option<FsDetectResult> {
    if &buf[UDF_BEA_OFFSET..UDF_BEA_OFFSET + 5] == UDF_BEA_MAGIC {
        Some(FsDetectResult {
            fs_type: FsType::Udf,
            oem_name: None,
            volume_label: None,
            volume_id: None,
            block_size: Some(2048),
            total_size: None,
            confidence: 0.85,
            magic_hex: hex::encode(&buf[UDF_BEA_OFFSET..UDF_BEA_OFFSET + 5]),
        })
    } else {
        None
    }
}

fn try_detect_hfsplus(buf: &[u8], _len: usize) -> Option<FsDetectResult> {
    let sig = &buf[HFSPLUS_MAGIC_OFFSET..HFSPLUS_MAGIC_OFFSET + 2];
    if sig == HFSPLUS_MAGIC || sig == HFSX_MAGIC {
        let block_size = read_be_u32(buf, HFSPLUS_MAGIC_OFFSET + 40);
        let total_blocks = read_be_u32(buf, HFSPLUS_MAGIC_OFFSET + 44);
        Some(FsDetectResult {
            fs_type: FsType::HfsPlus,
            oem_name: None,
            volume_label: None,
            volume_id: None,
            block_size: Some(block_size),
            total_size: Some(total_blocks as u64 * block_size as u64),
            confidence: 0.90,
            magic_hex: hex::encode(sig),
        })
    } else {
        None
    }
}

fn try_detect_swap(buf: &[u8], _len: usize) -> Option<FsDetectResult> {
    let off = SWAP_MAGIC_OFFSET_4K;
    let slice = &buf[off..off + SWAP_MAGIC.len()];
    if slice == SWAP_MAGIC || slice == SWAP_MAGIC_OLD {
        // Swap header: page size encoded at version field, page count at offset 4
        let last_page = read_le_u32(buf, 4);
        Some(FsDetectResult {
            fs_type: FsType::LinuxSwap,
            oem_name: None,
            volume_label: None,
            volume_id: None,
            block_size: Some(4096),
            total_size: Some(last_page as u64 * 4096),
            confidence: 0.90,
            magic_hex: hex::encode(slice),
        })
    } else {
        None
    }
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Helper: create a buffer of given size filled with zeros.
    fn zero_buf(size: usize) -> Vec<u8> {
        vec![0u8; size]
    }

    #[test]
    fn test_fs_type_display() {
        assert_eq!(FsType::Fat32.to_string(), "FAT32");
        assert_eq!(FsType::Ntfs.to_string(), "NTFS");
        assert_eq!(FsType::Ext4.to_string(), "ext4");
        assert_eq!(FsType::Btrfs.to_string(), "Btrfs");
        assert_eq!(FsType::Iso9660.to_string(), "ISO 9660");
        assert_eq!(FsType::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_fs_type_properties() {
        assert!(FsType::Ntfs.is_windows_native());
        assert!(!FsType::Ext4.is_windows_native());
        assert!(FsType::Ext4.is_linux_native());
        assert!(!FsType::Ntfs.is_linux_native());
        assert!(FsType::HfsPlus.is_macos_native());
        assert!(FsType::Fat32.is_fat());
        assert!(!FsType::Ntfs.is_fat());
        assert!(FsType::Ext3.is_ext());
        assert!(!FsType::Xfs.is_ext());
        assert!(FsType::Ntfs.is_writable());
        assert!(!FsType::Iso9660.is_writable());
    }

    #[test]
    fn test_fs_type_max_file_size() {
        assert_eq!(FsType::Fat32.max_file_size(), Some(4 * 1024 * 1024 * 1024 - 1));
        assert!(FsType::Ntfs.max_file_size().is_none());
    }

    #[test]
    fn test_detect_exfat() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 8].copy_from_slice(EXFAT_MAGIC);
        buf[108] = 9; // sector_size_shift = 512
        buf[109] = 3; // cluster_shift = 8 sectors
        // total_sectors = 1000000
        let sectors: u64 = 1_000_000;
        buf[72..80].copy_from_slice(&sectors.to_le_bytes());
        // serial
        buf[100..104].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::ExFat);
        assert_eq!(result.block_size, Some(4096)); // 512 << 3
        assert!(result.confidence > 0.9);
    }

    #[test]
    fn test_detect_ntfs() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 8].copy_from_slice(NTFS_MAGIC);
        // bytes per sector = 512
        buf[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
        // sectors per cluster = 8
        buf[0x0D] = 8;
        // total sectors
        let sectors: u64 = 2_000_000;
        buf[0x28..0x30].copy_from_slice(&sectors.to_le_bytes());
        // serial
        let serial: u64 = 0x0102030405060708;
        buf[0x48..0x50].copy_from_slice(&serial.to_le_bytes());

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Ntfs);
        assert_eq!(result.block_size, Some(4096));
    }

    #[test]
    fn test_detect_refs() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 4].copy_from_slice(REFS_MAGIC);
        buf[FAT_OEM_OFFSET + 4] = 0;
        buf[FAT_OEM_OFFSET + 5] = 0;
        buf[FAT_OEM_OFFSET + 6] = 0;
        buf[FAT_OEM_OFFSET + 7] = 0;

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::ReFs);
    }

    #[test]
    fn test_detect_fat32() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        // OEM name
        buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 8].copy_from_slice(b"MSDOS5.0");
        // bytes per sector = 512
        buf[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
        // sectors per cluster = 8
        buf[0x0D] = 8;
        // reserved sectors = 32
        buf[0x0E..0x10].copy_from_slice(&32u16.to_le_bytes());
        // number of FATs = 2
        buf[0x10] = 2;
        // root entry count = 0 (FAT32)
        buf[0x11..0x13].copy_from_slice(&0u16.to_le_bytes());
        // total sectors 16 = 0
        buf[0x13..0x15].copy_from_slice(&0u16.to_le_bytes());
        // media byte
        buf[0x15] = 0xF8;
        // FAT size 16 = 0
        buf[0x16..0x18].copy_from_slice(&0u16.to_le_bytes());
        // total sectors 32 = 2097152 (1 GB)
        buf[0x20..0x24].copy_from_slice(&2_097_152u32.to_le_bytes());
        // FAT size 32 = 2048
        buf[0x24..0x28].copy_from_slice(&2048u32.to_le_bytes());
        // Volume serial at 0x43
        buf[0x43..0x47].copy_from_slice(&0xCAFEBABEu32.to_le_bytes());
        // Volume label at 0x47
        buf[0x47..0x52].copy_from_slice(b"MYDRIVE    ");
        // FS type at 0x52
        buf[0x52..0x5A].copy_from_slice(b"FAT32   ");

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Fat32);
        assert_eq!(result.volume_label, Some("MYDRIVE".to_string()));
        assert!(result.confidence >= 0.90);
    }

    #[test]
    fn test_detect_fat16() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[FAT_OEM_OFFSET..FAT_OEM_OFFSET + 8].copy_from_slice(b"MSDOS5.0");
        buf[0x0B..0x0D].copy_from_slice(&512u16.to_le_bytes());
        buf[0x0D] = 4; // sectors per cluster
        buf[0x0E..0x10].copy_from_slice(&1u16.to_le_bytes()); // reserved
        buf[0x10] = 2; // FATs
        buf[0x11..0x13].copy_from_slice(&512u16.to_le_bytes()); // root entries
        buf[0x13..0x15].copy_from_slice(&32768u16.to_le_bytes()); // total sectors 16 MB
        buf[0x15] = 0xF8;
        buf[0x16..0x18].copy_from_slice(&32u16.to_le_bytes()); // FAT size
        buf[0x27..0x2B].copy_from_slice(&0x12345678u32.to_le_bytes()); // serial
        buf[0x2B..0x36].copy_from_slice(b"BOOT       ");
        buf[0x36..0x3E].copy_from_slice(b"FAT16   ");

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Fat16);
        assert!(result.confidence >= 0.90);
    }

    #[test]
    fn test_detect_ext4() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        // ext magic at 0x438
        buf[EXT_SUPER_MAGIC_OFFSET..EXT_SUPER_MAGIC_OFFSET + 2]
            .copy_from_slice(&EXT_SUPER_MAGIC.to_le_bytes());
        let sb = 0x400;
        // s_log_block_size = 2 → block_size = 4096
        buf[sb + 0x18..sb + 0x1C].copy_from_slice(&2u32.to_le_bytes());
        // s_blocks_count_lo = 262144
        buf[sb + 0x04..sb + 0x08].copy_from_slice(&262144u32.to_le_bytes());
        // Feature compat: has_journal
        buf[sb + 0x5C..sb + 0x60].copy_from_slice(&0x0004u32.to_le_bytes());
        // Feature incompat: extents (0x40)
        buf[sb + 0x60..sb + 0x64].copy_from_slice(&0x0040u32.to_le_bytes());
        // Volume label
        buf[sb + 0x78..sb + 0x82].copy_from_slice(b"rootfs\0\0\0\0");
        // UUID
        for i in 0..16 {
            buf[sb + 0x68 + i] = (i * 17) as u8;
        }

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Ext4);
        assert_eq!(result.volume_label, Some("rootfs".to_string()));
        assert_eq!(result.block_size, Some(4096));
    }

    #[test]
    fn test_detect_ext2() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[EXT_SUPER_MAGIC_OFFSET..EXT_SUPER_MAGIC_OFFSET + 2]
            .copy_from_slice(&EXT_SUPER_MAGIC.to_le_bytes());
        let sb = 0x400;
        buf[sb + 0x18..sb + 0x1C].copy_from_slice(&0u32.to_le_bytes()); // block size = 1024
        buf[sb + 0x04..sb + 0x08].copy_from_slice(&1024u32.to_le_bytes());
        // No journal, no extents = ext2

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Ext2);
    }

    #[test]
    fn test_detect_xfs() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[0..4].copy_from_slice(XFS_MAGIC);
        buf[4..8].copy_from_slice(&4096u32.to_be_bytes()); // block size
        let blocks: u64 = 500_000;
        buf[8..16].copy_from_slice(&blocks.to_be_bytes());
        for i in 0..16 {
            buf[32 + i] = (i * 11) as u8;
        }
        buf[108..115].copy_from_slice(b"myxfs\0\0");

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Xfs);
        assert_eq!(result.block_size, Some(4096));
        assert_eq!(result.volume_label, Some("myxfs".to_string()));
    }

    #[test]
    fn test_detect_btrfs() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        let sb = 0x10000;
        let total: u64 = 10_737_418_240; // 10 GB
        buf[sb + 0x70..sb + 0x78].copy_from_slice(&total.to_le_bytes());
        buf[sb + 0x44..sb + 0x48].copy_from_slice(&4096u32.to_le_bytes()); // sector_size
        // Write magic AFTER node_size area to avoid overwrite at 0x10040
        buf[BTRFS_MAGIC_OFFSET..BTRFS_MAGIC_OFFSET + 8].copy_from_slice(BTRFS_MAGIC);
        for i in 0..16 {
            buf[sb + 0x20 + i] = (i * 7) as u8;
        }
        buf[sb + 0x12B..sb + 0x12B + 6].copy_from_slice(b"btrfs\0");

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Btrfs);
        assert_eq!(result.total_size, Some(10_737_418_240));
    }

    #[test]
    fn test_detect_iso9660() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[ISO9660_MAGIC_OFFSET..ISO9660_MAGIC_OFFSET + 5].copy_from_slice(ISO9660_MAGIC);
        let vd = 0x8000;
        buf[vd] = 1; // Primary Volume Descriptor
        buf[vd + 8..vd + 20].copy_from_slice(b"LINUX       ");
        buf[vd + 40..vd + 60].copy_from_slice(b"Ubuntu 24.04 LTS    ");
        buf[vd + 128..vd + 130].copy_from_slice(&2048u16.to_le_bytes());
        buf[vd + 80..vd + 84].copy_from_slice(&500_000u32.to_le_bytes());

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Iso9660);
        assert_eq!(
            result.volume_label,
            Some("Ubuntu 24.04 LTS".to_string())
        );
    }

    #[test]
    fn test_detect_udf() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[UDF_BEA_OFFSET..UDF_BEA_OFFSET + 5].copy_from_slice(UDF_BEA_MAGIC);

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Udf);
    }

    #[test]
    fn test_detect_hfsplus() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[HFSPLUS_MAGIC_OFFSET..HFSPLUS_MAGIC_OFFSET + 2].copy_from_slice(HFSPLUS_MAGIC);
        buf[HFSPLUS_MAGIC_OFFSET + 40..HFSPLUS_MAGIC_OFFSET + 44]
            .copy_from_slice(&4096u32.to_be_bytes());
        buf[HFSPLUS_MAGIC_OFFSET + 44..HFSPLUS_MAGIC_OFFSET + 48]
            .copy_from_slice(&1_000_000u32.to_be_bytes());

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::HfsPlus);
    }

    #[test]
    fn test_detect_swap() {
        let mut buf = zero_buf(MIN_DETECT_BYTES);
        buf[SWAP_MAGIC_OFFSET_4K..SWAP_MAGIC_OFFSET_4K + SWAP_MAGIC.len()]
            .copy_from_slice(SWAP_MAGIC);
        buf[4..8].copy_from_slice(&1_048_576u32.to_le_bytes()); // last_page

        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::LinuxSwap);
    }

    #[test]
    fn test_detect_unknown() {
        let buf = zero_buf(MIN_DETECT_BYTES);
        let mut cursor = Cursor::new(&buf);
        let result = detect_filesystem(&mut cursor).unwrap();
        assert_eq!(result.fs_type, FsType::Unknown);
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn test_detect_too_small() {
        let buf = vec![0u8; 100]; // Too small
        let mut cursor = Cursor::new(&buf);
        assert!(detect_filesystem(&mut cursor).is_err());
    }

    #[test]
    fn test_extract_ascii_string() {
        assert_eq!(
            extract_ascii_string(b"HELLO WORLD!", 0, 5),
            Some("HELLO".to_string())
        );
        assert_eq!(extract_ascii_string(b"\0\0\0\0", 0, 4), None);
        assert_eq!(
            extract_ascii_string(b"test   ", 0, 7),
            Some("test".to_string())
        );
    }

    #[test]
    fn test_fs_detect_result_display() {
        let result = FsDetectResult {
            fs_type: FsType::Fat32,
            oem_name: Some("MSDOS5.0".into()),
            volume_label: Some("BOOT".into()),
            volume_id: Some("CAFEBABE".into()),
            block_size: Some(4096),
            total_size: Some(1_073_741_824),
            confidence: 0.95,
            magic_hex: "4d53444f53352e30".into(),
        };
        let s = result.to_string();
        assert!(s.contains("FAT32"));
        assert!(s.contains("BOOT"));
        assert!(s.contains("MSDOS5.0"));
    }

    #[test]
    fn test_read_le_u16_u32() {
        let buf = [0x34, 0x12, 0x78, 0x56];
        assert_eq!(read_le_u16(&buf, 0), 0x1234);
        assert_eq!(read_le_u32(&buf, 0), 0x56781234);
    }

    #[test]
    fn test_read_be_u32() {
        let buf = [0x12, 0x34, 0x56, 0x78];
        assert_eq!(read_be_u32(&buf, 0), 0x12345678);
    }
}
