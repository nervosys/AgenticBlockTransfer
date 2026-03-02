// WIM reader — Windows Imaging Format metadata parser
//
// Parses the WIM header structure: magic ("MSWIM\0\0\0"), version, flags,
// image count, resource offsets, and boot index.
//
// WIM files are *archive* formats (like tar) containing one or more OS images
// with file-level deduplication. Unlike QCOW2/VHD/VMDK, WIM is NOT a block-level
// format — it cannot be written sector-by-sector to produce a bootable disk.
// However, abt needs to:
//   1. Detect WIM files and show metadata (`abt info`)
//   2. In the future, support extraction-based writing (like Rufus does with
//      install.wim → FAT32 USB)
//
// This module provides header parsing and metadata display. Block-level streaming
// is not applicable; instead, the write engine should use extraction mode.
//
// Reference: MS-WIM specification
//            https://docs.microsoft.com/en-us/previous-versions/windows/it-pro/windows-7-vista/dd861280

#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::fmt;
use std::io::{Read, Seek, SeekFrom};

/// WIM magic: "MSWIM\0\0\0" (8 bytes).
const WIM_MAGIC: [u8; 8] = *b"MSWIM\0\0\0";

/// WIM header size.
const WIM_HEADER_SIZE: u32 = 208;

/// WIM header flags.
#[derive(Debug, Clone, Copy)]
pub struct WimFlags(pub u32);

impl WimFlags {
    pub const RESERVED: u32 = 0x0000_0001;
    pub const COMPRESS_XPRESS: u32 = 0x0002_0000;
    pub const COMPRESS_LZX: u32 = 0x0004_0000;
    pub const COMPRESS_LZMS: u32 = 0x0008_0000;

    pub fn is_compressed(self) -> bool {
        self.0
            & (Self::COMPRESS_XPRESS | Self::COMPRESS_LZX | Self::COMPRESS_LZMS)
            != 0
    }

    pub fn compression_name(self) -> &'static str {
        if self.0 & Self::COMPRESS_LZMS != 0 {
            "LZMS"
        } else if self.0 & Self::COMPRESS_LZX != 0 {
            "LZX"
        } else if self.0 & Self::COMPRESS_XPRESS != 0 {
            "XPRESS"
        } else {
            "none"
        }
    }
}

/// A WIM resource descriptor (offset table / boot metadata / integrity table).
#[derive(Debug, Clone, Default)]
pub struct WimResourceDescriptor {
    /// Combined size+flags field (lower 56 bits = compressed size, upper 8 bits = flags).
    pub size_and_flags: u64,
    /// Offset within the WIM file.
    pub offset: u64,
    /// Original (uncompressed) size.
    pub original_size: u64,
}

impl WimResourceDescriptor {
    pub fn compressed_size(&self) -> u64 {
        self.size_and_flags & 0x00FF_FFFF_FFFF_FFFF
    }

    pub fn flags(&self) -> u8 {
        ((self.size_and_flags >> 56) & 0xFF) as u8
    }

    pub fn is_present(&self) -> bool {
        self.offset != 0 || self.original_size != 0
    }

    fn parse<R: Read>(reader: &mut R) -> Result<Self> {
        let size_and_flags = reader.read_u64::<LittleEndian>()?;
        let offset = reader.read_u64::<LittleEndian>()?;
        let original_size = reader.read_u64::<LittleEndian>()?;
        Ok(Self {
            size_and_flags,
            offset,
            original_size,
        })
    }
}

/// Parsed WIM file header.
#[derive(Debug, Clone)]
pub struct WimHeader {
    /// WIM format version (major, minor).
    pub version_major: u16,
    pub version_minor: u16,
    /// Header flags (compression, etc.).
    pub flags: WimFlags,
    /// Size of each compressed chunk (default 32768 for LZX).
    pub chunk_size: u32,
    /// GUID of the WIM file (16 bytes).
    pub guid: [u8; 16],
    /// Part number (for split WIMs).
    pub part_number: u16,
    /// Total number of parts.
    pub total_parts: u16,
    /// Number of images in the WIM.
    pub image_count: u32,
    /// Offset table (resource lookup).
    pub offset_table: WimResourceDescriptor,
    /// XML data describing images.
    pub xml_data: WimResourceDescriptor,
    /// Boot metadata resource.
    pub boot_metadata: WimResourceDescriptor,
    /// Boot index (0 = not bootable).
    pub boot_index: u32,
    /// Integrity table.
    pub integrity_table: WimResourceDescriptor,
}

impl WimHeader {
    /// Parse a WIM header from a seekable reader.
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        reader.seek(SeekFrom::Start(0))?;

        // Magic
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        if magic != WIM_MAGIC {
            bail!(
                "Not a WIM file: magic {:?}, expected {:?}",
                &magic[..6],
                &WIM_MAGIC[..6]
            );
        }

        // Header size
        let header_size = reader.read_u32::<LittleEndian>()?;
        if header_size < WIM_HEADER_SIZE {
            bail!("WIM header too small: {} bytes", header_size);
        }

        // Version
        let version_minor = reader.read_u16::<LittleEndian>()?;
        let version_major = reader.read_u16::<LittleEndian>()?;

        // Flags
        let flags = WimFlags(reader.read_u32::<LittleEndian>()?);

        // Chunk size
        let chunk_size = reader.read_u32::<LittleEndian>()?;

        // GUID
        let mut guid = [0u8; 16];
        reader.read_exact(&mut guid)?;

        // Part info
        let part_number = reader.read_u16::<LittleEndian>()?;
        let total_parts = reader.read_u16::<LittleEndian>()?;

        // Image count
        let image_count = reader.read_u32::<LittleEndian>()?;

        // Resource descriptors
        let offset_table = WimResourceDescriptor::parse(reader)?;
        let xml_data = WimResourceDescriptor::parse(reader)?;
        let boot_metadata = WimResourceDescriptor::parse(reader)?;

        // Boot index
        let boot_index = reader.read_u32::<LittleEndian>()?;

        // Integrity table
        let integrity_table = WimResourceDescriptor::parse(reader)?;

        // Skip unused bytes (76 bytes of padding)
        // reader position is now at offset 208

        Ok(WimHeader {
            version_major,
            version_minor,
            flags,
            chunk_size,
            guid,
            part_number,
            total_parts,
            image_count,
            offset_table,
            xml_data,
            boot_metadata,
            boot_index,
            integrity_table,
        })
    }

    /// Whether this WIM is bootable.
    pub fn is_bootable(&self) -> bool {
        self.boot_index > 0
    }

    /// Whether this is a split WIM.
    pub fn is_split(&self) -> bool {
        self.total_parts > 1
    }

    /// Format the GUID as a standard UUID-like string.
    pub fn guid_string(&self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.guid[0], self.guid[1], self.guid[2], self.guid[3],
            self.guid[4], self.guid[5],
            self.guid[6], self.guid[7],
            self.guid[8], self.guid[9],
            self.guid[10], self.guid[11], self.guid[12], self.guid[13], self.guid[14], self.guid[15]
        )
    }

    /// Read the embedded XML metadata (if present).
    pub fn read_xml<R: Read + Seek>(&self, reader: &mut R) -> Result<Option<String>> {
        if !self.xml_data.is_present() {
            return Ok(None);
        }
        reader.seek(SeekFrom::Start(self.xml_data.offset))?;
        let size = self.xml_data.original_size as usize;
        let mut buf = vec![0u8; size.min(64 * 1024)]; // cap at 64 KiB
        let n = reader.read(&mut buf)?;
        buf.truncate(n);

        // WIM XML is typically UTF-16LE
        if buf.len() >= 2 && buf[0] == 0xFF && buf[1] == 0xFE {
            // UTF-16LE BOM
            let u16_data: Vec<u16> = buf[2..]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            Ok(Some(String::from_utf16_lossy(&u16_data)))
        } else {
            Ok(Some(String::from_utf8_lossy(&buf).to_string()))
        }
    }

    /// Human-readable summary.
    pub fn summary(&self) -> String {
        format!(
            "WIM v{}.{}, {} image(s), compression: {}, chunk: {} KiB{}{}",
            self.version_major,
            self.version_minor,
            self.image_count,
            self.flags.compression_name(),
            self.chunk_size / 1024,
            if self.is_bootable() {
                format!(", boot index #{}", self.boot_index)
            } else {
                String::new()
            },
            if self.is_split() {
                format!(", part {}/{}", self.part_number, self.total_parts)
            } else {
                String::new()
            }
        )
    }
}

impl fmt::Display for WimHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.summary())
    }
}

/// Parse a WIM file and return header information.
pub fn parse_wim(path: &std::path::Path) -> Result<WimHeader> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open WIM file: {}", path.display()))?;
    WimHeader::parse(&mut file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::{LittleEndian, WriteBytesExt};
    use std::io::Cursor;

    /// Build a minimal valid WIM header in memory.
    fn build_test_wim_header(image_count: u32, boot_index: u32, flags: u32) -> Vec<u8> {
        let mut buf = Vec::new();

        // Magic
        buf.extend_from_slice(&WIM_MAGIC);
        // Header size
        buf.write_u32::<LittleEndian>(WIM_HEADER_SIZE).unwrap();
        // Version (minor, major)
        buf.write_u16::<LittleEndian>(14).unwrap(); // minor
        buf.write_u16::<LittleEndian>(1).unwrap(); // major = 1
        // Flags
        buf.write_u32::<LittleEndian>(flags).unwrap();
        // Chunk size
        buf.write_u32::<LittleEndian>(32768).unwrap();
        // GUID (16 bytes)
        buf.extend_from_slice(&[
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10,
        ]);
        // Part number
        buf.write_u16::<LittleEndian>(1).unwrap();
        // Total parts
        buf.write_u16::<LittleEndian>(1).unwrap();
        // Image count
        buf.write_u32::<LittleEndian>(image_count).unwrap();
        // Offset table (24 bytes)
        buf.write_u64::<LittleEndian>(0).unwrap(); // size_and_flags
        buf.write_u64::<LittleEndian>(0).unwrap(); // offset
        buf.write_u64::<LittleEndian>(0).unwrap(); // original_size
        // XML data (24 bytes)
        buf.write_u64::<LittleEndian>(0).unwrap();
        buf.write_u64::<LittleEndian>(0).unwrap();
        buf.write_u64::<LittleEndian>(0).unwrap();
        // Boot metadata (24 bytes)
        buf.write_u64::<LittleEndian>(0).unwrap();
        buf.write_u64::<LittleEndian>(0).unwrap();
        buf.write_u64::<LittleEndian>(0).unwrap();
        // Boot index
        buf.write_u32::<LittleEndian>(boot_index).unwrap();
        // Integrity table (24 bytes)
        buf.write_u64::<LittleEndian>(0).unwrap();
        buf.write_u64::<LittleEndian>(0).unwrap();
        buf.write_u64::<LittleEndian>(0).unwrap();
        // Pad to 208 bytes
        while buf.len() < 208 {
            buf.push(0);
        }
        buf
    }

    #[test]
    fn test_parse_wim_header_basic() {
        let data = build_test_wim_header(2, 0, 0);
        let mut cursor = Cursor::new(data);
        let header = WimHeader::parse(&mut cursor).unwrap();
        assert_eq!(header.version_major, 1);
        assert_eq!(header.version_minor, 14);
        assert_eq!(header.image_count, 2);
        assert!(!header.is_bootable());
        assert!(!header.is_split());
    }

    #[test]
    fn test_parse_wim_bootable() {
        let data = build_test_wim_header(3, 1, 0);
        let mut cursor = Cursor::new(data);
        let header = WimHeader::parse(&mut cursor).unwrap();
        assert_eq!(header.image_count, 3);
        assert!(header.is_bootable());
        assert_eq!(header.boot_index, 1);
    }

    #[test]
    fn test_parse_wim_lzx_compression() {
        let data = build_test_wim_header(1, 0, WimFlags::COMPRESS_LZX);
        let mut cursor = Cursor::new(data);
        let header = WimHeader::parse(&mut cursor).unwrap();
        assert!(header.flags.is_compressed());
        assert_eq!(header.flags.compression_name(), "LZX");
    }

    #[test]
    fn test_parse_wim_xpress_compression() {
        let data = build_test_wim_header(1, 0, WimFlags::COMPRESS_XPRESS);
        let mut cursor = Cursor::new(data);
        let header = WimHeader::parse(&mut cursor).unwrap();
        assert!(header.flags.is_compressed());
        assert_eq!(header.flags.compression_name(), "XPRESS");
    }

    #[test]
    fn test_wim_bad_magic() {
        let mut data = build_test_wim_header(1, 0, 0);
        data[0] = 0xFF; // corrupt magic
        let mut cursor = Cursor::new(data);
        assert!(WimHeader::parse(&mut cursor).is_err());
    }

    #[test]
    fn test_wim_summary() {
        let data = build_test_wim_header(2, 1, WimFlags::COMPRESS_LZX);
        let mut cursor = Cursor::new(data);
        let header = WimHeader::parse(&mut cursor).unwrap();
        let summary = header.summary();
        assert!(summary.contains("WIM v1.14"));
        assert!(summary.contains("2 image(s)"));
        assert!(summary.contains("LZX"));
        assert!(summary.contains("boot index #1"));
    }

    #[test]
    fn test_wim_guid_string() {
        let data = build_test_wim_header(1, 0, 0);
        let mut cursor = Cursor::new(data);
        let header = WimHeader::parse(&mut cursor).unwrap();
        let guid = header.guid_string();
        assert_eq!(guid, "01020304-0506-0708-090a-0b0c0d0e0f10");
    }

    #[test]
    fn test_wim_no_xml() {
        let data = build_test_wim_header(1, 0, 0);
        let mut cursor = Cursor::new(data);
        let header = WimHeader::parse(&mut cursor).unwrap();
        let xml = header.read_xml(&mut cursor).unwrap();
        assert!(xml.is_none());
    }
}
