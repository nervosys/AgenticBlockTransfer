// ISOHybrid detection — detect MBR/GPT boot records embedded in ISO images
//
// An ISOHybrid ISO 9660 image contains an embedded MBR (and optionally GPT)
// so it can be booted from both optical media (CD/DVD via El Torito) and
// from USB drives (via the embedded MBR/GPT). This is critical for determining
// the correct write mode:
//   - ISOHybrid ISO → write as raw dd (block-level) to USB
//   - Standard ISO  → extract and copy files to FAT32 partition
//
// Detection checks:
//   1. MBR boot signature (0x55AA) at bytes 510-511
//   2. ISOLINUX/isohybrid MBR signature at byte offset 0x1B0 (magic: 0xFB 0xC0 0x78 0x70)
//   3. Non-zero partition entries in the MBR at offsets 446-509
//   4. GPT protective MBR (partition type 0xEE) indicating embedded GPT
//   5. El Torito boot record in ISO 9660 volume descriptors
//   6. Grub2 isohybrid indicators
//
// Reference: syslinux isohybrid source, Rufus iso.c, xorriso documentation

#![allow(dead_code)]

use anyhow::{Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

// ─── Constants ─────────────────────────────────────────────────────────────

/// MBR boot signature: 0x55 0xAA at offset 510-511.
const MBR_BOOT_SIGNATURE: [u8; 2] = [0x55, 0xAA];

/// ISOLINUX isohybrid MBR magic at offset 0x1B0 (0x70 0x78 0xC0 0xFB in LE).
const ISOHYBRID_MBR_MAGIC: u32 = 0xFBC07870;

/// Alternate ISOLINUX isohybrid indicator: look for isohybrid signature.
const ISOHYBRID_V2_OFFSET: u64 = 0x1B0;

/// GPT protective MBR partition type.
const GPT_PROTECTIVE_TYPE: u8 = 0xEE;

/// ISO 9660 sector size.
const ISO_SECTOR: u64 = 2048;

/// GPT header signature: "EFI PART" at LBA 1 offset 0.
const GPT_SIGNATURE: &[u8; 8] = b"EFI PART";

/// Grub2 hybrid MBR tag bytes (alternate isohybrid).
const GRUB2_HYBRID_TAG: &[u8; 4] = b"GRUB";

// ─── MBR Partition Entry ───────────────────────────────────────────────────

/// Parsed MBR partition entry (16 bytes each, 4 entries at MBR offset 446).
#[derive(Debug, Clone, Copy, Default)]
pub struct MbrPartitionEntry {
    /// Boot indicator (0x80 = active/bootable, 0x00 = inactive).
    pub boot_indicator: u8,
    /// Partition type byte.
    pub partition_type: u8,
    /// Starting CHS address (packed, usually not useful for LBA).
    pub start_chs: [u8; 3],
    /// Ending CHS address (packed).
    pub end_chs: [u8; 3],
    /// Starting LBA sector.
    pub start_lba: u32,
    /// Number of sectors.
    pub sector_count: u32,
}

impl MbrPartitionEntry {
    /// Parse an MBR partition entry from 16 bytes.
    pub fn from_bytes(data: &[u8; 16]) -> Self {
        Self {
            boot_indicator: data[0],
            partition_type: data[4],
            start_chs: [data[1], data[2], data[3]],
            end_chs: [data[5], data[6], data[7]],
            start_lba: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            sector_count: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        }
    }

    /// Whether this entry is empty/unused.
    pub fn is_empty(&self) -> bool {
        self.partition_type == 0 && self.start_lba == 0 && self.sector_count == 0
    }

    /// Whether this entry is a GPT protective partition.
    pub fn is_gpt_protective(&self) -> bool {
        self.partition_type == GPT_PROTECTIVE_TYPE
    }

    /// Whether this entry is marked bootable.
    pub fn is_bootable(&self) -> bool {
        self.boot_indicator == 0x80
    }

    /// Size in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.sector_count as u64 * 512
    }
}

impl std::fmt::Display for MbrPartitionEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_empty() {
            write!(f, "(empty)")
        } else {
            write!(
                f,
                "type={:#04x}{} start_lba={} sectors={} ({} MiB)",
                self.partition_type,
                if self.is_bootable() { " [bootable]" } else { "" },
                self.start_lba,
                self.sector_count,
                self.size_bytes() / (1024 * 1024),
            )
        }
    }
}

// ─── ISOHybrid Type ────────────────────────────────────────────────────────

/// Classification of the ISO's boot hybridization method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsoHybridType {
    /// Not an ISOHybrid — standard ISO 9660.
    None,
    /// ISOLINUX / syslinux isohybrid MBR (most Linux ISOs).
    Isolinux,
    /// GRUB2 isohybrid (some Fedora, openSUSE).
    Grub2,
    /// Generic MBR with partition entries (detected heuristically).
    GenericMbr,
    /// GPT protective MBR (UEFI-bootable hybrid).
    GptHybrid,
}

impl std::fmt::Display for IsoHybridType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None (standard ISO 9660)"),
            Self::Isolinux => write!(f, "ISOLINUX isohybrid"),
            Self::Grub2 => write!(f, "GRUB2 isohybrid"),
            Self::GenericMbr => write!(f, "Generic MBR hybrid"),
            Self::GptHybrid => write!(f, "GPT protective hybrid"),
        }
    }
}

// ─── Detection Result ──────────────────────────────────────────────────────

/// Recommended write mode based on ISOHybrid detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Write as raw dd (block-level) — image is bootable via MBR/GPT.
    RawDd,
    /// Extract and copy files to a FAT32/NTFS partition.
    FileCopy,
    /// Both modes are viable — let the user choose.
    Either,
}

impl std::fmt::Display for WriteMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RawDd => write!(f, "Raw DD (block-level write)"),
            Self::FileCopy => write!(f, "File copy (extract to FAT32/NTFS)"),
            Self::Either => write!(f, "Either (user choice: DD or file copy)"),
        }
    }
}

/// Complete ISOHybrid detection results.
#[derive(Debug, Clone)]
pub struct IsoHybridInfo {
    /// Whether the image is an ISOHybrid.
    pub is_hybrid: bool,
    /// Type of hybridization detected.
    pub hybrid_type: IsoHybridType,
    /// Recommended write mode.
    pub recommended_mode: WriteMode,
    /// Whether a valid MBR boot signature was found.
    pub has_mbr_signature: bool,
    /// Whether ISOLINUX isohybrid magic was found.
    pub has_isolinux_magic: bool,
    /// Whether GPT protective MBR was found.
    pub has_gpt_protective: bool,
    /// Whether GPT header signature was found (at LBA 1).
    pub has_gpt_header: bool,
    /// Parsed MBR partition entries (up to 4).
    pub partitions: Vec<MbrPartitionEntry>,
    /// Non-empty partition count.
    pub active_partition_count: usize,
    /// Whether El Torito boot record was found in ISO volume descriptors.
    pub has_el_torito: bool,
    /// Whether the image appears to be a Windows ISO (uses file copy mode).
    pub is_windows_iso: bool,
    /// File size in bytes (if known).
    pub file_size: Option<u64>,
}

impl IsoHybridInfo {
    /// Quick summary for display.
    pub fn summary(&self) -> String {
        if self.is_hybrid {
            format!(
                "ISOHybrid detected: {} — recommended: {}",
                self.hybrid_type, self.recommended_mode
            )
        } else {
            "Standard ISO 9660 (not an ISOHybrid)".into()
        }
    }
}

impl std::fmt::Display for IsoHybridInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "ISOHybrid Analysis:")?;
        writeln!(f, "  Is hybrid:            {}", self.is_hybrid)?;
        writeln!(f, "  Hybrid type:          {}", self.hybrid_type)?;
        writeln!(f, "  Recommended mode:     {}", self.recommended_mode)?;
        writeln!(f, "  MBR boot signature:   {}", self.has_mbr_signature)?;
        writeln!(f, "  ISOLINUX magic:       {}", self.has_isolinux_magic)?;
        writeln!(f, "  GPT protective MBR:   {}", self.has_gpt_protective)?;
        writeln!(f, "  GPT header:           {}", self.has_gpt_header)?;
        writeln!(f, "  El Torito boot:       {}", self.has_el_torito)?;
        writeln!(f, "  Windows ISO:          {}", self.is_windows_iso)?;
        writeln!(f, "  Active partitions:    {}", self.active_partition_count)?;
        for (i, p) in self.partitions.iter().enumerate() {
            if !p.is_empty() {
                writeln!(f, "    Partition {}: {}", i + 1, p)?;
            }
        }
        if let Some(size) = self.file_size {
            writeln!(f, "  File size:            {} MiB", size / (1024 * 1024))?;
        }
        Ok(())
    }
}

// ─── Detection Functions ───────────────────────────────────────────────────

/// Detect ISOHybrid properties of an ISO image.
///
/// Reads the first 2 sectors (1 KiB) for MBR analysis plus
/// ISO 9660 volume descriptors at sector 16+ for El Torito.
pub fn detect<R: Read + Seek>(reader: &mut R) -> Result<IsoHybridInfo> {
    // Read first 512 bytes (MBR)
    reader.seek(SeekFrom::Start(0))?;
    let mut mbr = [0u8; 512];
    reader.read_exact(&mut mbr)
        .context("Failed to read MBR from ISO image")?;

    // 1. Check MBR boot signature at 510-511
    let has_mbr_signature = mbr[510] == MBR_BOOT_SIGNATURE[0] && mbr[511] == MBR_BOOT_SIGNATURE[1];

    // 2. Check ISOLINUX isohybrid magic at 0x1B0
    let has_isolinux_magic = if mbr.len() > 0x1B3 {
        let magic = u32::from_le_bytes([mbr[0x1B0], mbr[0x1B1], mbr[0x1B2], mbr[0x1B3]]);
        magic == ISOHYBRID_MBR_MAGIC
    } else {
        false
    };

    // 3. Parse 4 MBR partition entries at offset 446
    let mut partitions = Vec::with_capacity(4);
    let mut has_gpt_protective = false;
    let mut active_partition_count = 0;

    for i in 0..4 {
        let base = 446 + i * 16;
        let mut entry_data = [0u8; 16];
        entry_data.copy_from_slice(&mbr[base..base + 16]);
        let entry = MbrPartitionEntry::from_bytes(&entry_data);

        if entry.is_gpt_protective() {
            has_gpt_protective = true;
        }
        if !entry.is_empty() {
            active_partition_count += 1;
        }
        partitions.push(entry);
    }

    // 4. Check for GPT header at LBA 1 (byte 512)
    let has_gpt_header = {
        let mut gpt_sig = [0u8; 8];
        match reader.seek(SeekFrom::Start(512)) {
            Ok(_) => match reader.read_exact(&mut gpt_sig) {
                Ok(()) => &gpt_sig == GPT_SIGNATURE,
                Err(_) => false,
            },
            Err(_) => false,
        }
    };

    // 5. Check for GRUB2 hybrid indicator
    let has_grub2 = {
        // GRUB2 embeds "GRUB" near the end of the MBR code area
        let mbr_code = &mbr[0..440];
        mbr_code.windows(4).any(|w| w == GRUB2_HYBRID_TAG)
    };

    // 6. Check for El Torito boot record at ISO sector 17 (0x11)
    let has_el_torito = check_el_torito(reader).unwrap_or(false);

    // 7. Check for Windows ISO indicators
    let is_windows_iso = check_windows_iso(reader).unwrap_or(false);

    // 8. Get file size
    let file_size = reader.seek(SeekFrom::End(0)).ok();

    // Determine hybrid type
    let hybrid_type = if has_isolinux_magic {
        IsoHybridType::Isolinux
    } else if has_grub2 && (has_mbr_signature || has_gpt_protective) {
        IsoHybridType::Grub2
    } else if has_gpt_protective && has_gpt_header {
        IsoHybridType::GptHybrid
    } else if has_mbr_signature && active_partition_count > 0 && !is_windows_iso {
        IsoHybridType::GenericMbr
    } else {
        IsoHybridType::None
    };

    let is_hybrid = hybrid_type != IsoHybridType::None;

    // Determine recommended write mode
    let recommended_mode = if is_windows_iso {
        WriteMode::Either
    } else if is_hybrid {
        WriteMode::RawDd
    } else if has_el_torito {
        WriteMode::FileCopy
    } else {
        WriteMode::FileCopy
    };

    Ok(IsoHybridInfo {
        is_hybrid,
        hybrid_type,
        recommended_mode,
        has_mbr_signature,
        has_isolinux_magic,
        has_gpt_protective,
        has_gpt_header,
        partitions,
        active_partition_count,
        has_el_torito,
        is_windows_iso,
        file_size,
    })
}

/// Check for El Torito boot record volume descriptor at ISO sector 17.
fn check_el_torito<R: Read + Seek>(reader: &mut R) -> Result<bool> {
    // El Torito Boot Record Volume Descriptor is at sector 17 (0x11)
    // Type byte = 0, identifier = "CD001", version = 1
    // Boot system identifier starts with "EL TORITO SPECIFICATION"
    reader.seek(SeekFrom::Start(17 * ISO_SECTOR))?;
    let mut vd = [0u8; 40];
    match reader.read_exact(&mut vd) {
        Ok(()) => {
            // Type = 0 (boot record), "CD001" at offset 1
            if vd[0] == 0 && &vd[1..6] == b"CD001" {
                // Check for "EL TORITO" in boot system identifier (offset 7)
                let sys_id = &vd[7..39];
                Ok(sys_id.windows(9).any(|w| w == b"EL TORITO"))
            } else {
                Ok(false)
            }
        }
        Err(_) => Ok(false),
    }
}

/// Heuristic check for Windows installation ISO:
/// Look for characteristic files/strings in the ISO volume descriptor.
fn check_windows_iso<R: Read + Seek>(reader: &mut R) -> Result<bool> {
    // Check Primary Volume Descriptor at sector 16
    reader.seek(SeekFrom::Start(16 * ISO_SECTOR))?;
    let mut pvd = [0u8; 256];
    match reader.read_exact(&mut pvd) {
        Ok(()) => {
            // Volume identifier is at offset 40-71 (32 bytes, padded with spaces)
            let vol_id = String::from_utf8_lossy(&pvd[40..72]);
            let vol_id_upper = vol_id.trim().to_uppercase();

            // Check for common Windows ISO volume IDs
            let is_win = vol_id_upper.contains("CCCOMA_X64")
                || vol_id_upper.contains("CCCOMA_X86")
                || vol_id_upper.contains("CPBA_A64")
                || vol_id_upper.contains("CPBA_X64")
                || vol_id_upper.starts_with("WIN")
                || vol_id_upper.starts_with("WINDOWS")
                || vol_id_upper.contains("GRMCULFR")
                || vol_id_upper.contains("GSP1RMCUL")
                || vol_id_upper.contains("J_CCSA_X64");

            // Also check application identifier at offset 574
            if is_win {
                return Ok(true);
            }

            // Check publisher at offset 318-446 for "MICROSOFT CORPORATION"
            if pvd.len() >= 446 {
                let publisher = String::from_utf8_lossy(&pvd[318..446.min(pvd.len())]);
                if publisher.to_uppercase().contains("MICROSOFT") {
                    return Ok(true);
                }
            }

            Ok(false)
        }
        Err(_) => Ok(false),
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_standard_iso() -> Vec<u8> {
        // Create minimal ISO: at least 18 sectors (for El Torito check at sector 17)
        let mut data = vec![0u8; 18 * ISO_SECTOR as usize];

        // Primary Volume Descriptor at sector 16:
        // type=1, "CD001", version=1
        let pvd_offset = 16 * ISO_SECTOR as usize;
        data[pvd_offset] = 1; // type
        data[pvd_offset + 1..pvd_offset + 6].copy_from_slice(b"CD001");
        data[pvd_offset + 6] = 1; // version
        // Volume ID at offset 40
        data[pvd_offset + 40..pvd_offset + 50].copy_from_slice(b"TEST_LINUX");

        data
    }

    fn make_isohybrid_iso() -> Vec<u8> {
        let mut data = make_standard_iso();

        // Add MBR boot signature
        data[510] = 0x55;
        data[511] = 0xAA;

        // Add ISOLINUX isohybrid magic at 0x1B0
        let magic = ISOHYBRID_MBR_MAGIC.to_le_bytes();
        data[0x1B0..0x1B4].copy_from_slice(&magic);

        // Add a partition entry at offset 446
        data[446] = 0x80; // bootable
        data[450] = 0x17; // partition type (hidden NTFS — common for isohybrid)
        data[454..458].copy_from_slice(&1u32.to_le_bytes()); // start LBA
        data[458..462].copy_from_slice(&2048u32.to_le_bytes()); // sectors

        // Add El Torito at sector 17
        let el_offset = 17 * ISO_SECTOR as usize;
        data[el_offset] = 0; // type = boot
        data[el_offset + 1..el_offset + 6].copy_from_slice(b"CD001");
        data[el_offset + 7..el_offset + 16].copy_from_slice(b"EL TORITO");

        data
    }

    fn make_gpt_hybrid_iso() -> Vec<u8> {
        let mut data = make_standard_iso();

        // MBR boot signature
        data[510] = 0x55;
        data[511] = 0xAA;

        // GPT protective MBR partition at entry 0
        data[446] = 0x00; // not bootable
        data[450] = GPT_PROTECTIVE_TYPE; // 0xEE
        data[454..458].copy_from_slice(&1u32.to_le_bytes()); // start LBA 1
        data[458..462].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes()); // max sectors

        // GPT header at LBA 1 (byte 512)
        data[512..520].copy_from_slice(GPT_SIGNATURE);

        data
    }

    #[test]
    fn test_standard_iso_not_hybrid() {
        let data = make_standard_iso();
        let mut cursor = Cursor::new(&data);
        let info = detect(&mut cursor).unwrap();
        assert!(!info.is_hybrid);
        assert_eq!(info.hybrid_type, IsoHybridType::None);
        assert_eq!(info.recommended_mode, WriteMode::FileCopy);
    }

    #[test]
    fn test_isolinux_isohybrid_detection() {
        let data = make_isohybrid_iso();
        let mut cursor = Cursor::new(&data);
        let info = detect(&mut cursor).unwrap();
        assert!(info.is_hybrid);
        assert_eq!(info.hybrid_type, IsoHybridType::Isolinux);
        assert!(info.has_mbr_signature);
        assert!(info.has_isolinux_magic);
        assert!(info.has_el_torito);
        assert_eq!(info.recommended_mode, WriteMode::RawDd);
        assert_eq!(info.active_partition_count, 1);
    }

    #[test]
    fn test_gpt_hybrid_detection() {
        let data = make_gpt_hybrid_iso();
        let mut cursor = Cursor::new(&data);
        let info = detect(&mut cursor).unwrap();
        assert!(info.is_hybrid);
        assert_eq!(info.hybrid_type, IsoHybridType::GptHybrid);
        assert!(info.has_gpt_protective);
        assert!(info.has_gpt_header);
        assert_eq!(info.recommended_mode, WriteMode::RawDd);
    }

    #[test]
    fn test_windows_iso_detection() {
        let mut data = make_standard_iso();
        // Set volume ID to a Windows identifier
        let pvd_offset = 16 * ISO_SECTOR as usize;
        data[pvd_offset + 40..pvd_offset + 51].copy_from_slice(b"CCCOMA_X64\0");
        let mut cursor = Cursor::new(&data);
        let info = detect(&mut cursor).unwrap();
        assert!(info.is_windows_iso);
        assert_eq!(info.recommended_mode, WriteMode::Either);
    }

    #[test]
    fn test_mbr_partition_entry_parse() {
        let mut entry_data = [0u8; 16];
        entry_data[0] = 0x80; // bootable
        entry_data[4] = 0x0C; // FAT32 LBA
        entry_data[8..12].copy_from_slice(&100u32.to_le_bytes());
        entry_data[12..16].copy_from_slice(&2048u32.to_le_bytes());
        let entry = MbrPartitionEntry::from_bytes(&entry_data);
        assert!(entry.is_bootable());
        assert!(!entry.is_empty());
        assert_eq!(entry.partition_type, 0x0C);
        assert_eq!(entry.start_lba, 100);
        assert_eq!(entry.sector_count, 2048);
        assert_eq!(entry.size_bytes(), 2048 * 512);
    }

    #[test]
    fn test_empty_partition_entry() {
        let entry = MbrPartitionEntry::default();
        assert!(entry.is_empty());
        assert!(!entry.is_bootable());
        assert!(!entry.is_gpt_protective());
    }

    #[test]
    fn test_display_formatting() {
        let data = make_isohybrid_iso();
        let mut cursor = Cursor::new(&data);
        let info = detect(&mut cursor).unwrap();
        let display = format!("{}", info);
        assert!(display.contains("ISOHybrid Analysis"));
        assert!(display.contains("ISOLINUX"));
        assert!(display.contains("true"));
    }

    #[test]
    fn test_isohybrid_type_display() {
        assert!(format!("{}", IsoHybridType::Isolinux).contains("ISOLINUX"));
        assert!(format!("{}", IsoHybridType::Grub2).contains("GRUB2"));
        assert!(format!("{}", IsoHybridType::None).contains("standard"));
    }

    #[test]
    fn test_write_mode_display() {
        assert!(format!("{}", WriteMode::RawDd).contains("Raw DD"));
        assert!(format!("{}", WriteMode::FileCopy).contains("File copy"));
        assert!(format!("{}", WriteMode::Either).contains("Either"));
    }

    #[test]
    fn test_summary() {
        let data = make_isohybrid_iso();
        let mut cursor = Cursor::new(&data);
        let info = detect(&mut cursor).unwrap();
        let summary = info.summary();
        assert!(summary.contains("ISOHybrid detected"));
        assert!(summary.contains("ISOLINUX"));

        let standard = make_standard_iso();
        let mut cursor2 = Cursor::new(&standard);
        let info2 = detect(&mut cursor2).unwrap();
        assert!(info2.summary().contains("not an ISOHybrid"));
    }

    #[test]
    fn test_too_small_file() {
        // File smaller than 512 bytes — should fail gracefully
        let data = vec![0u8; 100];
        let mut cursor = Cursor::new(&data);
        assert!(detect(&mut cursor).is_err());
    }
}
