// FFU (Full Flash Update) image reader — Microsoft deployment format
//
// FFU is used by Rufus and Windows Device Recovery Tool for flashing
// Windows IoT Core, Windows Phone, and Windows 10/11 recovery images.
//
// Layout (v1/v2):
//   - Security header  (signed hash table)
//   - Image header     (manifest, platform IDs, target size)
//   - Store header(s)  (block data entry maps)
//   - Block data        (payload chunks, each 128 KiB by default)
//
// Reference: FFU Image Format specification (MS internal)
//            Rufus src/format.c / src/parser.c FFU handling
//            wpinternals FFU parser

#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{self, Read, Seek, SeekFrom};

// ─── Constants ─────────────────────────────────────────────────────────────

/// FFU security header signature: "SignedImage "
const FFU_SIGNATURE: &[u8; 12] = b"SignedImage ";

/// Default FFU block/chunk size (128 KiB).
const FFU_DEFAULT_BLOCK_SIZE: u32 = 128 * 1024;

/// Maximum sane manifest length (1 MiB).
const FFU_MAX_MANIFEST_LEN: u64 = 1024 * 1024;

/// Maximum sane number of block data entries (16 M).
const FFU_MAX_BLOCK_ENTRIES: u64 = 16 * 1024 * 1024;

// ─── Security Header ──────────────────────────────────────────────────────

/// Parsed FFU security header (first bytes of the file).
#[derive(Debug, Clone)]
pub struct FfuSecurityHeader {
    /// Total byte size of the security header including padding.
    pub header_size: u32,
    /// Byte offset of the signed hash table.
    pub hash_table_offset: u32,
    /// Byte size of the hash table.
    pub hash_table_size: u32,
    /// Hash algorithm identifier (1 = SHA-256).
    pub hash_algorithm: u32,
    /// Number of hash entries (one per chunk).
    pub chunk_count: u32,
    /// Size of each chunk in bytes (typically 128 KiB).
    pub chunk_size: u32,
    /// Byte offset of the catalog data.
    pub catalog_offset: u32,
    /// Byte size of the catalog data.
    pub catalog_size: u32,
}

impl FfuSecurityHeader {
    /// Parse the FFU security header from position 0 of a seekable reader.
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        reader.seek(SeekFrom::Start(0))?;

        // Validate signature
        let mut sig = [0u8; 12];
        reader.read_exact(&mut sig)?;
        if &sig != FFU_SIGNATURE {
            bail!(
                "Not an FFU image: expected 'SignedImage ' magic, got {:?}",
                String::from_utf8_lossy(&sig)
            );
        }

        // Read sizes & counts
        let header_size = reader.read_u32::<LittleEndian>()?;
        let hash_table_offset = reader.read_u32::<LittleEndian>()?;
        let hash_table_size = reader.read_u32::<LittleEndian>()?;
        let hash_algorithm = reader.read_u32::<LittleEndian>()?;
        let chunk_count = reader.read_u32::<LittleEndian>()?;
        let chunk_size = reader.read_u32::<LittleEndian>()?;
        let catalog_offset = reader.read_u32::<LittleEndian>()?;
        let catalog_size = reader.read_u32::<LittleEndian>()?;

        Ok(Self {
            header_size,
            hash_table_offset,
            hash_table_size,
            hash_algorithm,
            chunk_count,
            chunk_size,
            catalog_offset,
            catalog_size,
        })
    }
}

impl std::fmt::Display for FfuSecurityHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "FFU Security Header:")?;
        writeln!(f, "  Header size:       {} bytes", self.header_size)?;
        writeln!(f, "  Hash algorithm:    {}", match self.hash_algorithm {
            1 => "SHA-256",
            _ => "Unknown",
        })?;
        writeln!(f, "  Chunk count:       {}", self.chunk_count)?;
        writeln!(f, "  Chunk size:        {} KiB", self.chunk_size / 1024)?;
        writeln!(f, "  Hash table:        offset={:#x} size={}", self.hash_table_offset, self.hash_table_size)?;
        write!(f, "  Catalog:           offset={:#x} size={}", self.catalog_offset, self.catalog_size)
    }
}

// ─── Image Header ──────────────────────────────────────────────────────────

/// Parsed FFU image header — immediately after the security header (aligned).
#[derive(Debug, Clone)]
pub struct FfuImageHeader {
    /// Total byte size of the image header.
    pub header_size: u32,
    /// Manifest string (XML or JSON describing the image contents).
    pub manifest: String,
    /// Number of platform IDs in the image.
    pub platform_id_count: u32,
    /// Platform ID strings.
    pub platform_ids: Vec<String>,
}

impl FfuImageHeader {
    /// Parse the image header from the reader at the given offset.
    pub fn parse<R: Read + Seek>(reader: &mut R, offset: u64) -> Result<Self> {
        reader.seek(SeekFrom::Start(offset))?;

        let header_size = reader.read_u32::<LittleEndian>()?;
        let manifest_len = reader.read_u32::<LittleEndian>()? as u64;

        if manifest_len > FFU_MAX_MANIFEST_LEN {
            bail!("FFU manifest length {} exceeds maximum {}", manifest_len, FFU_MAX_MANIFEST_LEN);
        }

        let mut manifest_bytes = vec![0u8; manifest_len as usize];
        reader.read_exact(&mut manifest_bytes)?;

        // Manifest may be UTF-16LE or UTF-8; try UTF-8 first
        let manifest = String::from_utf8(manifest_bytes.clone())
            .unwrap_or_else(|_| {
                // Try UTF-16LE
                let words: Vec<u16> = manifest_bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16_lossy(&words)
            });

        let platform_id_count = reader.read_u32::<LittleEndian>()?;
        let mut platform_ids = Vec::with_capacity(platform_id_count as usize);
        for _ in 0..platform_id_count.min(64) {
            let id_len = reader.read_u32::<LittleEndian>()? as usize;
            let mut id_buf = vec![0u8; id_len.min(256)];
            reader.read_exact(&mut id_buf)?;
            // Trim null bytes
            let id = String::from_utf8_lossy(&id_buf)
                .trim_end_matches('\0')
                .to_string();
            platform_ids.push(id);
        }

        Ok(Self {
            header_size,
            manifest,
            platform_id_count,
            platform_ids,
        })
    }
}

impl std::fmt::Display for FfuImageHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "FFU Image Header:")?;
        writeln!(f, "  Header size:       {} bytes", self.header_size)?;
        writeln!(f, "  Manifest length:   {} chars", self.manifest.len())?;
        writeln!(f, "  Platform IDs:      {}", self.platform_id_count)?;
        for (i, id) in self.platform_ids.iter().enumerate() {
            writeln!(f, "    [{}] {}", i, id)?;
        }
        Ok(())
    }
}

// ─── Store Header ──────────────────────────────────────────────────────────

/// Describes one block-data-entry mapping in a store.
#[derive(Debug, Clone, Copy)]
pub struct BlockDataEntry {
    /// Number of disk locations this block maps to.
    pub location_count: u32,
    /// Starting disk byte offset for the first location.
    pub disk_offset: u64,
    /// Number of contiguous blocks at this location.
    pub block_count: u32,
}

/// Parsed FFU store header — one per target partition/store.
#[derive(Debug, Clone)]
pub struct FfuStoreHeader {
    /// Total byte size of the store header.
    pub header_size: u32,
    /// Update type (1 = Full, 2 = Partial).
    pub update_type: u32,
    /// Major file-format version.
    pub major_version: u16,
    /// Minor file-format version.
    pub minor_version: u16,
    /// Full-flash major version.
    pub full_flash_major: u16,
    /// Full-flash minor version.
    pub full_flash_minor: u16,
    /// Number of block data entries.
    pub block_data_entry_count: u32,
    /// Size of each payload block in bytes.
    pub block_size: u32,
    /// Number of bytes per sector on the target device.
    pub bytes_per_sector: u32,
    /// Total number of sectors on the target device.
    pub sector_count: u64,
    /// Parsed block data entries (disk layout map).
    pub entries: Vec<BlockDataEntry>,
}

impl FfuStoreHeader {
    /// Parse a store header from the reader at the current position.
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let header_size = reader.read_u32::<LittleEndian>()?;
        let update_type = reader.read_u32::<LittleEndian>()?;
        let major_version = reader.read_u16::<LittleEndian>()?;
        let minor_version = reader.read_u16::<LittleEndian>()?;
        let full_flash_major = reader.read_u16::<LittleEndian>()?;
        let full_flash_minor = reader.read_u16::<LittleEndian>()?;

        // Skip platform ID and device path entries (variable length)
        // Read the block data entry count
        let _padding = reader.read_u32::<LittleEndian>()?; // reserved
        let block_data_entry_count = reader.read_u32::<LittleEndian>()?;
        let block_size = reader.read_u32::<LittleEndian>()?;
        let bytes_per_sector = reader.read_u32::<LittleEndian>()?;

        // Sector count is at least 4 bytes (may be 8 in v2)
        let sector_count = if major_version >= 2 {
            reader.read_u64::<LittleEndian>()?
        } else {
            reader.read_u32::<LittleEndian>()? as u64
        };

        // Parse block data entries
        let entry_count = (block_data_entry_count as u64).min(FFU_MAX_BLOCK_ENTRIES);
        let mut entries = Vec::with_capacity(entry_count as usize);
        for _ in 0..entry_count {
            let location_count = reader.read_u32::<LittleEndian>()?;
            let disk_offset = if location_count > 0 {
                // Each location: u32 disk_access_method + u64 block_index
                let _access_method = reader.read_u32::<LittleEndian>()?;
                let block_index = reader.read_u64::<LittleEndian>()?;
                block_index * block_size as u64
            } else {
                0
            };
            // Skip remaining locations if > 1
            for _ in 1..location_count {
                let _ = reader.read_u32::<LittleEndian>()?;
                let _ = reader.read_u64::<LittleEndian>()?;
            }
            let block_count = reader.read_u32::<LittleEndian>()?;
            entries.push(BlockDataEntry {
                location_count,
                disk_offset,
                block_count,
            });
        }

        Ok(Self {
            header_size,
            update_type,
            major_version,
            minor_version,
            full_flash_major,
            full_flash_minor,
            block_data_entry_count,
            block_size,
            bytes_per_sector,
            sector_count,
            entries,
        })
    }

    /// Calculate total image payload size in bytes.
    pub fn payload_size(&self) -> u64 {
        self.entries
            .iter()
            .map(|e| e.block_count as u64 * self.block_size as u64)
            .sum()
    }

    /// Calculate target disk size in bytes.
    pub fn disk_size(&self) -> u64 {
        self.sector_count * self.bytes_per_sector as u64
    }
}

impl std::fmt::Display for FfuStoreHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "FFU Store Header:")?;
        writeln!(f, "  Version:           {}.{}", self.major_version, self.minor_version)?;
        writeln!(f, "  Full-flash ver:    {}.{}", self.full_flash_major, self.full_flash_minor)?;
        writeln!(f, "  Update type:       {}", match self.update_type {
            1 => "Full",
            2 => "Partial",
            _ => "Unknown",
        })?;
        writeln!(f, "  Block size:        {} KiB", self.block_size / 1024)?;
        writeln!(f, "  Sector size:       {} bytes", self.bytes_per_sector)?;
        writeln!(f, "  Sector count:      {}", self.sector_count)?;
        writeln!(f, "  Disk size:         {} MiB", self.disk_size() / (1024 * 1024))?;
        writeln!(f, "  Block entries:     {}", self.block_data_entry_count)?;
        write!(f, "  Payload size:      {} MiB", self.payload_size() / (1024 * 1024))
    }
}

// ─── Composite Info ────────────────────────────────────────────────────────

/// Complete parsed metadata from an FFU image file.
#[derive(Debug, Clone)]
pub struct FfuInfo {
    pub security: FfuSecurityHeader,
    pub image: FfuImageHeader,
    pub stores: Vec<FfuStoreHeader>,
}

impl FfuInfo {
    /// Parse a complete FFU image from a seekable reader.
    /// Reads security header → image header → store headers.
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        let security = FfuSecurityHeader::parse(reader)
            .context("Failed to parse FFU security header")?;

        // Image header starts after security header (aligned to chunk boundary)
        let chunk_size = if security.chunk_size > 0 {
            security.chunk_size as u64
        } else {
            FFU_DEFAULT_BLOCK_SIZE as u64
        };

        let sec_end = security.header_size as u64;
        let image_offset = align_up(sec_end, chunk_size);

        let image = FfuImageHeader::parse(reader, image_offset)
            .context("Failed to parse FFU image header")?;

        // Store header starts after image header (aligned)
        let img_end = image_offset + image.header_size as u64;
        let store_offset = align_up(img_end, chunk_size);
        reader.seek(SeekFrom::Start(store_offset))?;

        // Parse one store header (multi-store images are rare)
        let store = FfuStoreHeader::parse(reader)
            .context("Failed to parse FFU store header")?;

        Ok(Self {
            security,
            image,
            stores: vec![store],
        })
    }

    /// Total target disk size from the first store.
    pub fn disk_size(&self) -> u64 {
        self.stores.first().map_or(0, |s| s.disk_size())
    }

    /// Total payload size from all stores.
    pub fn total_payload_size(&self) -> u64 {
        self.stores.iter().map(|s| s.payload_size()).sum()
    }
}

impl std::fmt::Display for FfuInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.security)?;
        writeln!(f)?;
        writeln!(f, "{}", self.image)?;
        for (i, store) in self.stores.iter().enumerate() {
            writeln!(f)?;
            writeln!(f, "Store {}:", i)?;
            write!(f, "{}", store)?;
        }
        Ok(())
    }
}

// ─── FFU Reader ────────────────────────────────────────────────────────────

/// Streaming reader that converts FFU block-data-entry payloads into a
/// linear raw disk image stream.
///
/// Blocks are read sequentially from the FFU payload section and written
/// to their mapped disk offsets. Gaps between mapped regions are filled
/// with zeroes.
pub struct FfuReader<R: Read + Seek> {
    inner: R,
    info: FfuInfo,
    /// Current linear byte position in the virtual disk.
    disk_pos: u64,
    /// Current entry index being read.
    entry_idx: usize,
    /// Byte offset within the current entry's payload.
    entry_byte_offset: u64,
    /// File offset where payload data begins.
    payload_file_offset: u64,
    /// Cumulative payload offset (sequential through entries).
    payload_cursor: u64,
    /// Virtual disk size.
    disk_size: u64,
}

impl<R: Read + Seek> FfuReader<R> {
    /// Create a new FFU reader from a parsed FFU info and seekable reader.
    pub fn new(mut inner: R, info: FfuInfo) -> Result<Self> {
        let disk_size = info.disk_size();

        // Compute payload file offset: after all headers, aligned to chunk boundary
        let chunk_size = if info.security.chunk_size > 0 {
            info.security.chunk_size as u64
        } else {
            FFU_DEFAULT_BLOCK_SIZE as u64
        };

        let sec_end = info.security.header_size as u64;
        let img_offset = align_up(sec_end, chunk_size);
        let img_end = img_offset + info.image.header_size as u64;
        let store_offset = align_up(img_end, chunk_size);

        // Store header size
        let store_header_size = info.stores.first()
            .map_or(0, |s| s.header_size as u64);
        let payload_file_offset = align_up(store_offset + store_header_size, chunk_size);

        inner.seek(SeekFrom::Start(payload_file_offset))?;

        Ok(Self {
            inner,
            info,
            disk_pos: 0,
            entry_idx: 0,
            entry_byte_offset: 0,
            payload_file_offset,
            payload_cursor: 0,
            disk_size,
        })
    }

    /// Get the virtual disk size in bytes.
    pub fn disk_size(&self) -> u64 {
        self.disk_size
    }

    /// Get a reference to the parsed FFU info.
    pub fn info(&self) -> &FfuInfo {
        &self.info
    }
}

impl<R: Read + Seek> Read for FfuReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.disk_pos >= self.disk_size {
            return Ok(0);
        }

        let stores = &self.info.stores;
        if stores.is_empty() {
            return Ok(0);
        }

        let store = &stores[0];
        let block_size = store.block_size as u64;

        // Find which entry covers the current disk position
        let mut file_payload_offset = 0u64;
        for (i, entry) in store.entries.iter().enumerate() {
            let entry_disk_start = entry.disk_offset;
            let entry_disk_len = entry.block_count as u64 * block_size;
            let entry_disk_end = entry_disk_start + entry_disk_len;

            if self.disk_pos >= entry_disk_start && self.disk_pos < entry_disk_end {
                // Within this entry — read from payload
                let offset_in_entry = self.disk_pos - entry_disk_start;
                let remaining_in_entry = entry_disk_len - offset_in_entry;
                let remaining_in_disk = self.disk_size - self.disk_pos;
                let to_read = buf.len().min(remaining_in_entry as usize).min(remaining_in_disk as usize);

                let file_offset = self.payload_file_offset + file_payload_offset + offset_in_entry;
                self.inner.seek(SeekFrom::Start(file_offset))
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

                let n = self.inner.read(&mut buf[..to_read])?;
                self.disk_pos += n as u64;
                return Ok(n);
            }

            file_payload_offset += entry_disk_len;

            // Check if disk_pos is in a gap before the next entry
            if i + 1 < store.entries.len() {
                let next_start = store.entries[i + 1].disk_offset;
                if self.disk_pos >= entry_disk_end && self.disk_pos < next_start {
                    // In a gap — fill with zeros
                    let gap_remaining = next_start - self.disk_pos;
                    let remaining_in_disk = self.disk_size - self.disk_pos;
                    let to_fill = buf.len().min(gap_remaining as usize).min(remaining_in_disk as usize);
                    buf[..to_fill].fill(0);
                    self.disk_pos += to_fill as u64;
                    return Ok(to_fill);
                }
            }
        }

        // Past all entries — fill with zeros to disk end
        let remaining = (self.disk_size - self.disk_pos).min(buf.len() as u64) as usize;
        if remaining > 0 {
            buf[..remaining].fill(0);
            self.disk_pos += remaining as u64;
            Ok(remaining)
        } else {
            Ok(0)
        }
    }
}

/// Align a value up to the next multiple of alignment.
fn align_up(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        return value;
    }
    let remainder = value % alignment;
    if remainder == 0 {
        value
    } else {
        value + alignment - remainder
    }
}

/// Detect whether a file is an FFU image by checking for the magic signature.
pub fn is_ffu<R: Read + Seek>(reader: &mut R) -> Result<bool> {
    reader.seek(SeekFrom::Start(0))?;
    let mut sig = [0u8; 12];
    match reader.read_exact(&mut sig) {
        Ok(()) => Ok(&sig == FFU_SIGNATURE),
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e.into()),
    }
}

/// Detect FFU and parse its info in one call.
pub fn parse_ffu<R: Read + Seek>(reader: &mut R) -> Result<FfuInfo> {
    FfuInfo::parse(reader)
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn make_ffu_header() -> Vec<u8> {
        let mut buf = Vec::new();

        // Security header: signature + 8 u32 fields
        buf.extend_from_slice(FFU_SIGNATURE); // 12 bytes
        buf.extend_from_slice(&256u32.to_le_bytes()); // header_size
        buf.extend_from_slice(&64u32.to_le_bytes());  // hash_table_offset
        buf.extend_from_slice(&32u32.to_le_bytes());  // hash_table_size
        buf.extend_from_slice(&1u32.to_le_bytes());   // hash_algorithm (SHA-256)
        buf.extend_from_slice(&4u32.to_le_bytes());   // chunk_count
        buf.extend_from_slice(&(128 * 1024u32).to_le_bytes()); // chunk_size (128 KiB)
        buf.extend_from_slice(&128u32.to_le_bytes()); // catalog_offset
        buf.extend_from_slice(&64u32.to_le_bytes());  // catalog_size
        // Pad to 256 bytes (header_size)
        buf.resize(256, 0);

        // Align to chunk boundary (128 KiB)
        let chunk = 128 * 1024;
        buf.resize(chunk, 0);

        // Image header: header_size + manifest_len + manifest + platform_count
        let manifest = b"<FullFlash><Version>2.0</Version></FullFlash>";
        let img_start = buf.len();
        buf.extend_from_slice(&128u32.to_le_bytes()); // header_size
        buf.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
        buf.extend_from_slice(manifest);
        buf.extend_from_slice(&1u32.to_le_bytes()); // platform_id_count
        let plat = b"Microsoft.Platform.1234\0";
        buf.extend_from_slice(&(plat.len() as u32).to_le_bytes());
        buf.extend_from_slice(plat);
        // Pad image header to header_size (128 bytes from img_start)
        let padded = img_start + 128;
        if buf.len() < padded {
            buf.resize(padded, 0);
        }

        // Align to next chunk boundary
        let aligned = align_up(buf.len() as u64, chunk as u64) as usize;
        buf.resize(aligned, 0);

        // Store header: minimal fields
        let store_start = buf.len();
        buf.extend_from_slice(&64u32.to_le_bytes());  // header_size
        buf.extend_from_slice(&1u32.to_le_bytes());   // update_type (Full)
        buf.extend_from_slice(&2u16.to_le_bytes());   // major_version
        buf.extend_from_slice(&0u16.to_le_bytes());   // minor_version
        buf.extend_from_slice(&1u16.to_le_bytes());   // full_flash_major
        buf.extend_from_slice(&0u16.to_le_bytes());   // full_flash_minor
        buf.extend_from_slice(&0u32.to_le_bytes());   // reserved
        buf.extend_from_slice(&1u32.to_le_bytes());   // block_data_entry_count
        buf.extend_from_slice(&(128 * 1024u32).to_le_bytes()); // block_size
        buf.extend_from_slice(&512u32.to_le_bytes()); // bytes_per_sector
        buf.extend_from_slice(&2048u64.to_le_bytes()); // sector_count → 1 MiB disk
        // Block data entry: location_count=1, access_method=0, block_index=0, block_count=1
        buf.extend_from_slice(&1u32.to_le_bytes());   // location_count
        buf.extend_from_slice(&0u32.to_le_bytes());   // access_method
        buf.extend_from_slice(&0u64.to_le_bytes());   // block_index
        buf.extend_from_slice(&1u32.to_le_bytes());   // block_count
        // Pad store header
        let padded_store = store_start + 64;
        if buf.len() < padded_store {
            buf.resize(padded_store, 0);
        }

        // Align to next chunk
        let aligned = align_up(buf.len() as u64, chunk as u64) as usize;
        buf.resize(aligned, 0);

        // Payload: one block of 128 KiB with pattern
        let payload_start = buf.len();
        for i in 0..(128 * 1024) {
            buf.push((i % 256) as u8);
        }

        buf
    }

    #[test]
    fn test_ffu_signature_detection() {
        let data = make_ffu_header();
        let mut cursor = Cursor::new(&data);
        assert!(is_ffu(&mut cursor).unwrap());

        let not_ffu = vec![0u8; 64];
        let mut cursor2 = Cursor::new(&not_ffu);
        assert!(!is_ffu(&mut cursor2).unwrap());
    }

    #[test]
    fn test_ffu_security_header_parse() {
        let data = make_ffu_header();
        let mut cursor = Cursor::new(&data);
        let sec = FfuSecurityHeader::parse(&mut cursor).unwrap();
        assert_eq!(sec.header_size, 256);
        assert_eq!(sec.hash_algorithm, 1);
        assert_eq!(sec.chunk_count, 4);
        assert_eq!(sec.chunk_size, 128 * 1024);
    }

    #[test]
    fn test_ffu_info_parse() {
        let data = make_ffu_header();
        let mut cursor = Cursor::new(&data);
        let info = FfuInfo::parse(&mut cursor).unwrap();

        assert_eq!(info.security.chunk_size, 128 * 1024);
        assert!(info.image.manifest.contains("FullFlash"));
        assert_eq!(info.image.platform_id_count, 1);
        assert!(!info.stores.is_empty());
        assert_eq!(info.stores[0].major_version, 2);
        assert_eq!(info.stores[0].block_data_entry_count, 1);
    }

    #[test]
    fn test_ffu_display() {
        let data = make_ffu_header();
        let mut cursor = Cursor::new(&data);
        let info = FfuInfo::parse(&mut cursor).unwrap();
        let display = format!("{}", info);
        assert!(display.contains("FFU Security Header"));
        assert!(display.contains("FFU Image Header"));
        assert!(display.contains("FFU Store Header"));
    }

    #[test]
    fn test_ffu_reader_basic() {
        let data = make_ffu_header();
        let mut cursor = Cursor::new(data.clone());
        let info = FfuInfo::parse(&mut cursor).unwrap();
        let disk_size = info.disk_size();
        assert!(disk_size > 0, "FFU disk size should be > 0");

        let reader = FfuReader::new(Cursor::new(data), info).unwrap();
        assert_eq!(reader.disk_size(), disk_size);
    }

    #[test]
    fn test_align_up() {
        assert_eq!(align_up(0, 512), 0);
        assert_eq!(align_up(1, 512), 512);
        assert_eq!(align_up(512, 512), 512);
        assert_eq!(align_up(513, 512), 1024);
        assert_eq!(align_up(100, 0), 100);
    }

    #[test]
    fn test_ffu_not_ffu_image() {
        let data = b"This is definitely not an FFU image file header!";
        let mut cursor = Cursor::new(data.as_ref());
        assert!(FfuSecurityHeader::parse(&mut cursor).is_err());
    }

    #[test]
    fn test_ffu_empty_input() {
        let data: Vec<u8> = vec![];
        let mut cursor = Cursor::new(data);
        assert!(!is_ffu(&mut cursor).unwrap());
    }

    #[test]
    fn test_ffu_store_payload_size() {
        let data = make_ffu_header();
        let mut cursor = Cursor::new(&data);
        let info = FfuInfo::parse(&mut cursor).unwrap();
        let payload = info.total_payload_size();
        // 1 entry × 1 block × 128 KiB = 128 KiB
        assert_eq!(payload, 128 * 1024);
    }
}
