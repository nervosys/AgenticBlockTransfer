//! GPT/MBR partition table parsing.
//!
//! Provides read-only introspection of partition tables for the `abt info`
//! command and for enhanced safety reporting (show existing partition layout
//! before overwriting a device).
//!
//! Supports:
//! - MBR (Master Boot Record) — first 512 bytes
//! - GPT (GUID Partition Table) — LBA 1 header + partition entry array

#![allow(dead_code)]

use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Constants ──────────────────────────────────────────────────────────────────

const MBR_SIGNATURE: u16 = 0xAA55;
const MBR_PARTITION_TABLE_OFFSET: usize = 446;
const MBR_PARTITION_ENTRY_SIZE: usize = 16;
const MBR_MAX_PARTITIONS: usize = 4;

const GPT_SIGNATURE: u64 = 0x5452_4150_2049_4645; // "EFI PART"
const GPT_HEADER_SIZE: usize = 92;
const GPT_ENTRY_MIN_SIZE: usize = 128;

const SECTOR_SIZE: u64 = 512;

// ── MBR Types ──────────────────────────────────────────────────────────────────

/// CHS (Cylinder-Head-Sector) address from MBR.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ChsAddress {
    pub head: u8,
    pub sector: u8,
    pub cylinder: u16,
}

/// A single MBR partition entry (16 bytes).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MbrPartition {
    /// Partition index (0-3).
    pub index: usize,
    /// Boot indicator (0x80 = active/bootable).
    pub bootable: bool,
    /// Partition type byte.
    pub partition_type: u8,
    /// Human-readable partition type name.
    pub type_name: String,
    /// Starting CHS address.
    pub start_chs: ChsAddress,
    /// Ending CHS address.
    pub end_chs: ChsAddress,
    /// Starting LBA (logical block address).
    pub start_lba: u32,
    /// Number of sectors in partition.
    pub sector_count: u32,
    /// Size in bytes.
    pub size_bytes: u64,
}

impl fmt::Display for MbrPartition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size = humansize::format_size(self.size_bytes, humansize::BINARY);
        let boot = if self.bootable { "*" } else { " " };
        write!(
            f,
            "  {}{}: type=0x{:02X} ({}) start={} sectors={} size={}",
            boot, self.index, self.partition_type, self.type_name, self.start_lba,
            self.sector_count, size
        )
    }
}

/// Parsed MBR (Master Boot Record).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mbr {
    /// Bootstrap code (first 446 bytes) — omitted from serialization for brevity.
    #[serde(skip)]
    pub bootstrap: Vec<u8>,
    /// MBR signature (should be 0xAA55).
    pub signature: u16,
    /// Whether the signature is valid.
    pub valid: bool,
    /// Whether this MBR is a protective MBR for GPT.
    pub is_protective: bool,
    /// Primary partition entries (up to 4).
    pub partitions: Vec<MbrPartition>,
}

impl fmt::Display for Mbr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.valid {
            return write!(f, "MBR: invalid signature (0x{:04X})", self.signature);
        }
        if self.is_protective {
            writeln!(f, "MBR: Protective (GPT)")?;
        } else {
            writeln!(f, "MBR: Standard")?;
        }
        for p in &self.partitions {
            writeln!(f, "{}", p)?;
        }
        Ok(())
    }
}

// ── GPT Types ──────────────────────────────────────────────────────────────────

/// A single GPT partition entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GptPartition {
    /// Partition index (0-based).
    pub index: usize,
    /// Partition type GUID.
    pub type_guid: Uuid,
    /// Human-readable partition type name.
    pub type_name: String,
    /// Unique partition GUID.
    pub unique_guid: Uuid,
    /// First LBA of the partition.
    pub first_lba: u64,
    /// Last LBA of the partition (inclusive).
    pub last_lba: u64,
    /// Attribute flags.
    pub attributes: u64,
    /// Partition name (UTF-16LE, up to 36 chars).
    pub name: String,
    /// Size in bytes.
    pub size_bytes: u64,
}

impl fmt::Display for GptPartition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let size = humansize::format_size(self.size_bytes, humansize::BINARY);
        write!(
            f,
            "  {}: \"{}\" type={} ({}) LBA={}-{} size={}",
            self.index, self.name, self.type_guid, self.type_name, self.first_lba,
            self.last_lba, size
        )
    }
}

/// Parsed GPT (GUID Partition Table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpt {
    /// GPT header revision.
    pub revision: u32,
    /// Disk GUID.
    pub disk_guid: Uuid,
    /// First usable LBA.
    pub first_usable_lba: u64,
    /// Last usable LBA.
    pub last_usable_lba: u64,
    /// Number of partition entries defined in header.
    pub max_partition_entries: u32,
    /// Size of each partition entry.
    pub partition_entry_size: u32,
    /// Parsed partition entries (only non-empty ones).
    pub partitions: Vec<GptPartition>,
}

impl fmt::Display for Gpt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "GPT: disk_guid={} usable_lba={}-{} entries={}",
            self.disk_guid, self.first_usable_lba, self.last_usable_lba,
            self.partitions.len()
        )?;
        for p in &self.partitions {
            writeln!(f, "{}", p)?;
        }
        Ok(())
    }
}

/// Complete partition table information for a device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartitionInfo {
    /// Device path this was read from.
    pub device: String,
    /// MBR (always present on MBR-formatted and GPT-formatted disks).
    pub mbr: Option<Mbr>,
    /// GPT (present only on GPT-formatted disks).
    pub gpt: Option<Gpt>,
    /// Summary scheme description.
    pub scheme: PartitionScheme,
}

/// The partition scheme detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartitionScheme {
    /// No valid partition table found.
    None,
    /// Standard MBR partitioning.
    Mbr,
    /// GUID Partition Table (with protective MBR).
    Gpt,
}

impl fmt::Display for PartitionScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Mbr => write!(f, "MBR"),
            Self::Gpt => write!(f, "GPT"),
        }
    }
}

impl fmt::Display for PartitionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Device: {} ({})", self.device, self.scheme)?;
        if let Some(ref mbr) = self.mbr {
            write!(f, "{}", mbr)?;
        }
        if let Some(ref gpt) = self.gpt {
            write!(f, "{}", gpt)?;
        }
        Ok(())
    }
}

// ── Parsing ────────────────────────────────────────────────────────────────────

/// Read and parse partition table information from a device or image file.
pub fn read_partition_info(path: &Path) -> Result<PartitionInfo> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open {} for partition reading", path.display()))?;

    let device = path.to_string_lossy().to_string();

    // Read the first sector (512 bytes) — MBR
    let mut mbr_buf = [0u8; 512];
    file.read_exact(&mut mbr_buf)
        .with_context(|| "Failed to read MBR sector")?;

    let mbr = parse_mbr(&mbr_buf);

    if !mbr.valid {
        return Ok(PartitionInfo {
            device,
            mbr: None,
            gpt: None,
            scheme: PartitionScheme::None,
        });
    }

    // If the MBR is protective (type 0xEE), try reading GPT at LBA 1
    if mbr.is_protective {
        match parse_gpt(&mut file) {
            Ok(gpt) => Ok(PartitionInfo {
                device,
                mbr: Some(mbr),
                gpt: Some(gpt),
                scheme: PartitionScheme::Gpt,
            }),
            Err(e) => {
                log::warn!("Protective MBR found but GPT parsing failed: {}", e);
                Ok(PartitionInfo {
                    device,
                    mbr: Some(mbr),
                    gpt: None,
                    scheme: PartitionScheme::Mbr,
                })
            }
        }
    } else {
        Ok(PartitionInfo {
            device,
            mbr: Some(mbr),
            gpt: None,
            scheme: PartitionScheme::Mbr,
        })
    }
}

/// Parse the MBR from a 512-byte buffer.
pub fn parse_mbr(buf: &[u8; 512]) -> Mbr {
    let signature = u16::from_le_bytes([buf[510], buf[511]]);
    let valid = signature == MBR_SIGNATURE;

    if !valid {
        return Mbr {
            bootstrap: buf[..MBR_PARTITION_TABLE_OFFSET].to_vec(),
            signature,
            valid: false,
            is_protective: false,
            partitions: vec![],
        };
    }

    let mut partitions = Vec::new();
    let mut is_protective = false;

    for i in 0..MBR_MAX_PARTITIONS {
        let offset = MBR_PARTITION_TABLE_OFFSET + i * MBR_PARTITION_ENTRY_SIZE;
        let entry = &buf[offset..offset + MBR_PARTITION_ENTRY_SIZE];

        let partition_type = entry[4];

        // Skip empty entries
        if partition_type == 0x00 {
            continue;
        }

        // Check for GPT protective MBR marker
        if partition_type == 0xEE {
            is_protective = true;
        }

        let bootable = entry[0] == 0x80;

        let start_chs = ChsAddress {
            head: entry[1],
            sector: entry[2] & 0x3F,
            cylinder: ((entry[2] as u16 & 0xC0) << 2) | entry[3] as u16,
        };

        let end_chs = ChsAddress {
            head: entry[5],
            sector: entry[6] & 0x3F,
            cylinder: ((entry[6] as u16 & 0xC0) << 2) | entry[7] as u16,
        };

        let start_lba = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
        let sector_count = u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]);

        partitions.push(MbrPartition {
            index: i,
            bootable,
            partition_type,
            type_name: mbr_type_name(partition_type),
            start_chs,
            end_chs,
            start_lba,
            sector_count,
            size_bytes: sector_count as u64 * SECTOR_SIZE,
        });
    }

    Mbr {
        bootstrap: buf[..MBR_PARTITION_TABLE_OFFSET].to_vec(),
        signature,
        valid: true,
        is_protective,
        partitions,
    }
}

/// Parse the GPT header and partition entries from LBA 1 onward.
fn parse_gpt(file: &mut std::fs::File) -> Result<Gpt> {
    // Seek to LBA 1 (byte 512)
    file.seek(SeekFrom::Start(SECTOR_SIZE))?;

    let mut header_buf = [0u8; GPT_HEADER_SIZE];
    file.read_exact(&mut header_buf)
        .with_context(|| "Failed to read GPT header")?;

    // Verify signature: bytes 0-7 = "EFI PART"
    let sig = u64::from_le_bytes(header_buf[0..8].try_into().unwrap());
    if sig != GPT_SIGNATURE {
        anyhow::bail!(
            "Invalid GPT signature: expected 0x{:016X}, got 0x{:016X}",
            GPT_SIGNATURE,
            sig
        );
    }

    let revision = u32::from_le_bytes(header_buf[8..12].try_into().unwrap());
    let first_usable_lba = u64::from_le_bytes(header_buf[40..48].try_into().unwrap());
    let last_usable_lba = u64::from_le_bytes(header_buf[48..56].try_into().unwrap());
    let disk_guid = parse_mixed_endian_guid(&header_buf[56..72]);
    let partition_entry_lba = u64::from_le_bytes(header_buf[72..80].try_into().unwrap());
    let max_partition_entries = u32::from_le_bytes(header_buf[80..84].try_into().unwrap());
    let partition_entry_size = u32::from_le_bytes(header_buf[84..88].try_into().unwrap());

    if (partition_entry_size as usize) < GPT_ENTRY_MIN_SIZE {
        anyhow::bail!(
            "GPT partition entry size too small: {} (expected >= {})",
            partition_entry_size,
            GPT_ENTRY_MIN_SIZE
        );
    }

    // Cap entries to a reasonable limit to prevent memory issues
    let max_entries = max_partition_entries.min(256) as usize;
    let entry_size = partition_entry_size as usize;

    // Seek to partition entry array
    file.seek(SeekFrom::Start(partition_entry_lba * SECTOR_SIZE))?;

    let mut partitions = Vec::new();
    let mut entry_buf = vec![0u8; entry_size];

    for i in 0..max_entries {
        file.read_exact(&mut entry_buf)
            .with_context(|| format!("Failed to read GPT partition entry {}", i))?;

        // Check if entry is empty (all-zero type GUID)
        let type_guid = parse_mixed_endian_guid(&entry_buf[0..16]);
        if type_guid == Uuid::nil() {
            continue;
        }

        let unique_guid = parse_mixed_endian_guid(&entry_buf[16..32]);
        let first_lba = u64::from_le_bytes(entry_buf[32..40].try_into().unwrap());
        let last_lba = u64::from_le_bytes(entry_buf[40..48].try_into().unwrap());
        let attributes = u64::from_le_bytes(entry_buf[48..56].try_into().unwrap());

        // Parse UTF-16LE name (bytes 56-128, up to 36 UTF-16 code units)
        let name_bytes = &entry_buf[56..entry_size.min(128)];
        let name = parse_utf16le_name(name_bytes);

        let size_bytes = (last_lba - first_lba + 1) * SECTOR_SIZE;

        partitions.push(GptPartition {
            index: i,
            type_guid,
            type_name: gpt_type_name(&type_guid),
            unique_guid,
            first_lba,
            last_lba,
            attributes,
            name,
            size_bytes,
        });
    }

    Ok(Gpt {
        revision,
        disk_guid,
        first_usable_lba,
        last_usable_lba,
        max_partition_entries,
        partition_entry_size,
        partitions,
    })
}

/// Parse a GUID from GPT's mixed-endian format.
/// GPT stores GUIDs in "mixed endian": the first 3 components are little-endian,
/// the remaining 2 are big-endian.
fn parse_mixed_endian_guid(bytes: &[u8]) -> Uuid {
    if bytes.len() < 16 {
        return Uuid::nil();
    }
    let d1 = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let d2 = u16::from_le_bytes([bytes[4], bytes[5]]);
    let d3 = u16::from_le_bytes([bytes[6], bytes[7]]);
    let d4: [u8; 8] = bytes[8..16].try_into().unwrap_or([0u8; 8]);
    Uuid::from_fields(d1, d2, d3, &d4)
}

/// Parse a UTF-16LE null-terminated name string.
fn parse_utf16le_name(bytes: &[u8]) -> String {
    let u16_units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
        .take_while(|&c| c != 0)
        .collect();
    String::from_utf16_lossy(&u16_units)
}

// ── Type name lookups ──────────────────────────────────────────────────────────

/// Get a human-readable name for an MBR partition type byte.
fn mbr_type_name(type_byte: u8) -> String {
    match type_byte {
        0x00 => "Empty".to_string(),
        0x01 => "FAT12".to_string(),
        0x04 => "FAT16 <32M".to_string(),
        0x05 => "Extended".to_string(),
        0x06 => "FAT16".to_string(),
        0x07 => "HPFS/NTFS/exFAT".to_string(),
        0x0B => "FAT32 (CHS)".to_string(),
        0x0C => "FAT32 (LBA)".to_string(),
        0x0E => "FAT16 (LBA)".to_string(),
        0x0F => "Extended (LBA)".to_string(),
        0x11 => "Hidden FAT12".to_string(),
        0x14 => "Hidden FAT16 <32M".to_string(),
        0x16 => "Hidden FAT16".to_string(),
        0x17 => "Hidden HPFS/NTFS".to_string(),
        0x1B => "Hidden FAT32".to_string(),
        0x1C => "Hidden FAT32 (LBA)".to_string(),
        0x1E => "Hidden FAT16 (LBA)".to_string(),
        0x27 => "Windows Recovery".to_string(),
        0x42 => "Dynamic Disk".to_string(),
        0x82 => "Linux swap".to_string(),
        0x83 => "Linux".to_string(),
        0x85 => "Linux extended".to_string(),
        0x8E => "Linux LVM".to_string(),
        0xA5 => "FreeBSD".to_string(),
        0xA6 => "OpenBSD".to_string(),
        0xA8 => "macOS".to_string(),
        0xA9 => "NetBSD".to_string(),
        0xAB => "macOS Boot".to_string(),
        0xAF => "HFS/HFS+".to_string(),
        0xBE => "Solaris boot".to_string(),
        0xBF => "Solaris".to_string(),
        0xEE => "GPT Protective".to_string(),
        0xEF => "EFI System".to_string(),
        0xFD => "Linux RAID".to_string(),
        _ => format!("Unknown (0x{:02X})", type_byte),
    }
}

/// Get a human-readable name for a GPT partition type GUID.
fn gpt_type_name(guid: &Uuid) -> String {
    // Well-known GPT partition type GUIDs (lowercase for comparison)
    let s = guid.to_string().to_lowercase();
    match s.as_str() {
        "c12a7328-f81f-11d2-ba4b-00a0c93ec93b" => "EFI System".to_string(),
        "21686148-6449-6e6f-744e-656564454649" => "BIOS Boot".to_string(),
        "e3c9e316-0b5c-4db8-817d-f92df00215ae" => "Microsoft Reserved".to_string(),
        "ebd0a0a2-b9e5-4433-87c0-68b6b72699c7" => "Microsoft Basic Data".to_string(),
        "5808c8aa-7e8f-42e0-85d2-e1e90434cfb3" => "Microsoft LDM metadata".to_string(),
        "af9b60a0-1431-4f62-bc68-3311714a69ad" => "Microsoft LDM data".to_string(),
        "de94bba4-06d1-4d40-a16a-bfd50179d6ac" => "Windows Recovery".to_string(),
        "0fc63daf-8483-4772-8e79-3d69d8477de4" => "Linux filesystem".to_string(),
        "0657fd6d-a4ab-43c4-84e5-0933c84b4f4f" => "Linux swap".to_string(),
        "e6d6d379-f507-44c2-a23c-238f2a3df928" => "Linux LVM".to_string(),
        "a19d880f-05fc-4d3b-a006-743f0f84911e" => "Linux RAID".to_string(),
        "933ac7e1-2eb4-4f13-b844-0e14e2aef915" => "Linux home".to_string(),
        "48465300-0000-11aa-aa11-00306543ecac" => "Apple HFS/HFS+".to_string(),
        "7c3457ef-0000-11aa-aa11-00306543ecac" => "Apple APFS".to_string(),
        "55465300-0000-11aa-aa11-00306543ecac" => "Apple UFS".to_string(),
        "516e7cb4-6ecf-11d6-8ff8-00022d09712b" => "FreeBSD data".to_string(),
        "83bd6b9d-7f41-11dc-be0b-001560b84f0f" => "FreeBSD boot".to_string(),
        "516e7cb5-6ecf-11d6-8ff8-00022d09712b" => "FreeBSD swap".to_string(),
        "516e7cb6-6ecf-11d6-8ff8-00022d09712b" => "FreeBSD UFS".to_string(),
        "516e7cb8-6ecf-11d6-8ff8-00022d09712b" => "FreeBSD ZFS".to_string(),
        _ => format!("Unknown ({})", guid),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;

    fn make_mbr_with_type(type_byte: u8) -> [u8; 512] {
        let mut buf = [0u8; 512];
        // Set signature
        buf[510] = 0x55;
        buf[511] = 0xAA;
        // Set first partition entry: type, start_lba=2048, sector_count=4096
        let offset = MBR_PARTITION_TABLE_OFFSET;
        buf[offset + 4] = type_byte;
        buf[offset + 8..offset + 12].copy_from_slice(&2048u32.to_le_bytes());
        buf[offset + 12..offset + 16].copy_from_slice(&4096u32.to_le_bytes());
        buf
    }

    #[test]
    fn parse_empty_mbr() {
        let buf = [0u8; 512];
        let mbr = parse_mbr(&buf);
        assert!(!mbr.valid);
        assert_eq!(mbr.partitions.len(), 0);
    }

    #[test]
    fn parse_valid_mbr_fat32() {
        let buf = make_mbr_with_type(0x0C);
        let mbr = parse_mbr(&buf);
        assert!(mbr.valid);
        assert!(!mbr.is_protective);
        assert_eq!(mbr.partitions.len(), 1);
        assert_eq!(mbr.partitions[0].partition_type, 0x0C);
        assert_eq!(mbr.partitions[0].type_name, "FAT32 (LBA)");
        assert_eq!(mbr.partitions[0].start_lba, 2048);
        assert_eq!(mbr.partitions[0].sector_count, 4096);
        assert_eq!(mbr.partitions[0].size_bytes, 4096 * 512);
    }

    #[test]
    fn parse_protective_mbr() {
        let buf = make_mbr_with_type(0xEE);
        let mbr = parse_mbr(&buf);
        assert!(mbr.valid);
        assert!(mbr.is_protective);
        assert_eq!(mbr.partitions[0].type_name, "GPT Protective");
    }

    #[test]
    fn parse_bootable_partition() {
        let mut buf = make_mbr_with_type(0x83);
        buf[MBR_PARTITION_TABLE_OFFSET] = 0x80; // bootable flag
        let mbr = parse_mbr(&buf);
        assert!(mbr.partitions[0].bootable);
    }

    #[test]
    fn mbr_type_names() {
        assert_eq!(mbr_type_name(0x07), "HPFS/NTFS/exFAT");
        assert_eq!(mbr_type_name(0x83), "Linux");
        assert_eq!(mbr_type_name(0x82), "Linux swap");
        assert_eq!(mbr_type_name(0xEE), "GPT Protective");
        assert_eq!(mbr_type_name(0xEF), "EFI System");
        assert!(mbr_type_name(0xFF).starts_with("Unknown"));
    }

    #[test]
    fn parse_mixed_endian_guid_known() {
        // EFI System Partition GUID: C12A7328-F81F-11D2-BA4B-00A0C93EC93B
        let bytes: [u8; 16] = [
            0x28, 0x73, 0x2A, 0xC1, // d1 LE
            0x1F, 0xF8, // d2 LE
            0xD2, 0x11, // d3 LE
            0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B, // d4 BE
        ];
        let guid = parse_mixed_endian_guid(&bytes);
        assert_eq!(
            guid.to_string().to_lowercase(),
            "c12a7328-f81f-11d2-ba4b-00a0c93ec93b"
        );
    }

    #[test]
    fn utf16le_name_parsing() {
        // "EFI" in UTF-16LE followed by null terminator
        let bytes = [
            b'E', 0, b'F', 0, b'I', 0, 0, 0,
        ];
        assert_eq!(parse_utf16le_name(&bytes), "EFI");
    }

    #[test]
    fn utf16le_empty_name() {
        let bytes = [0u8; 8];
        assert_eq!(parse_utf16le_name(&bytes), "");
    }

    #[test]
    fn partition_scheme_display() {
        assert_eq!(format!("{}", PartitionScheme::None), "none");
        assert_eq!(format!("{}", PartitionScheme::Mbr), "MBR");
        assert_eq!(format!("{}", PartitionScheme::Gpt), "GPT");
    }

    #[test]
    fn mbr_display_format() {
        let buf = make_mbr_with_type(0x83);
        let mbr = parse_mbr(&buf);
        let s = format!("{}", mbr);
        assert!(s.contains("MBR: Standard"));
        assert!(s.contains("Linux"));
    }

    #[test]
    fn read_partition_info_from_raw_file() {
        // Create a temp file with a valid MBR
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let buf = make_mbr_with_type(0x0C);
        std::io::Write::write_all(&mut f, &buf).unwrap();
        // Pad to at least 2 sectors for read_partition_info
        std::io::Write::write_all(&mut f, &[0u8; 512]).unwrap();
        f.flush().unwrap();

        let info = read_partition_info(f.path()).unwrap();
        assert_eq!(info.scheme, PartitionScheme::Mbr);
        assert!(info.mbr.is_some());
        assert!(info.gpt.is_none());
        let mbr = info.mbr.unwrap();
        assert_eq!(mbr.partitions.len(), 1);
        assert_eq!(mbr.partitions[0].partition_type, 0x0C);
    }
}
