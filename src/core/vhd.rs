// VHD / VHDX image reader — Microsoft Virtual Hard Disk formats
//
// VHD  — Legacy format (.vhd), footer at last 512 bytes, "conectix" magic.
//        Supports Fixed, Dynamic, and Differencing disk types.
// VHDX — Modern format (.vhdx), "vhdxfile" file identifier, 4 KiB headers.
//        Supports larger disks (up to 64 TB), better alignment, and resilience.
//
// This module parses headers and provides streaming Read implementations
// that convert VHD/VHDX cluster chains into raw block data.
//
// Reference: MS-VHDX specification, VHD spec (2006)

#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::io::{self, Read, Seek, SeekFrom};

// ─── VHD (Legacy) ──────────────────────────────────────────────────────────

/// VHD footer magic: "conectix"
const VHD_MAGIC: &[u8; 8] = b"conectix";
/// VHD footer/header size
const VHD_FOOTER_SIZE: u64 = 512;
/// VHD dynamic disk header magic: "cxsparse"
const VHD_DYNAMIC_MAGIC: &[u8; 8] = b"cxsparse";

/// VHD disk types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VhdDiskType {
    None = 0,
    Reserved1 = 1,
    Fixed = 2,
    Dynamic = 3,
    Differencing = 4,
}

impl VhdDiskType {
    fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            2 => Some(Self::Fixed),
            3 => Some(Self::Dynamic),
            4 => Some(Self::Differencing),
            _ => None,
        }
    }
}

impl std::fmt::Display for VhdDiskType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None"),
            Self::Reserved1 => write!(f, "Reserved"),
            Self::Fixed => write!(f, "Fixed"),
            Self::Dynamic => write!(f, "Dynamic"),
            Self::Differencing => write!(f, "Differencing"),
        }
    }
}

/// Parsed VHD footer (last 512 bytes of a .vhd file, or first 512 of fixed).
#[derive(Debug, Clone)]
pub struct VhdFooter {
    pub cookie: [u8; 8],
    pub features: u32,
    pub format_version: u32, // typically 0x00010000
    pub data_offset: u64,    // offset to dynamic header (0xFFFFFFFFFFFFFFFF for fixed)
    pub timestamp: u32,      // seconds since Jan 1, 2000 00:00:00
    pub creator_app: [u8; 4],
    pub creator_version: u32,
    pub creator_host_os: u32,
    pub original_size: u64,
    pub current_size: u64, // virtual disk size in bytes
    pub disk_geometry: VhdGeometry,
    pub disk_type: VhdDiskType,
    pub checksum: u32,
    pub unique_id: [u8; 16],
    pub saved_state: u8,
}

#[derive(Debug, Clone)]
pub struct VhdGeometry {
    pub cylinders: u16,
    pub heads: u8,
    pub sectors_per_track: u8,
}

impl VhdFooter {
    /// Parse a VHD footer from the given byte buffer (must be >= 512 bytes).
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 512 {
            bail!("VHD footer too short: {} bytes", data.len());
        }

        let cookie: [u8; 8] = data[0..8].try_into().unwrap();
        if &cookie != VHD_MAGIC {
            bail!(
                "Not a VHD image: cookie {:?}, expected {:?}",
                &cookie,
                VHD_MAGIC
            );
        }

        let mut r = std::io::Cursor::new(&data[8..]);
        let features = r.read_u32::<BigEndian>()?;
        let format_version = r.read_u32::<BigEndian>()?;
        let data_offset = r.read_u64::<BigEndian>()?;
        let timestamp = r.read_u32::<BigEndian>()?;

        let mut creator_app = [0u8; 4];
        r.read_exact(&mut creator_app)?;
        let creator_version = r.read_u32::<BigEndian>()?;
        let creator_host_os = r.read_u32::<BigEndian>()?;
        let original_size = r.read_u64::<BigEndian>()?;
        let current_size = r.read_u64::<BigEndian>()?;

        let cylinders = r.read_u16::<BigEndian>()?;
        let heads = r.read_u8()?;
        let sectors_per_track = r.read_u8()?;

        let disk_type_raw = r.read_u32::<BigEndian>()?;
        let disk_type = VhdDiskType::from_u32(disk_type_raw)
            .unwrap_or(VhdDiskType::None);

        let checksum = r.read_u32::<BigEndian>()?;

        let mut unique_id = [0u8; 16];
        r.read_exact(&mut unique_id)?;
        let saved_state = r.read_u8()?;

        Ok(VhdFooter {
            cookie,
            features,
            format_version,
            data_offset,
            timestamp,
            creator_app,
            creator_version,
            creator_host_os,
            original_size,
            current_size,
            disk_geometry: VhdGeometry {
                cylinders,
                heads,
                sectors_per_track,
            },
            disk_type,
            checksum,
            unique_id,
            saved_state,
        })
    }

    /// Human-readable summary.
    pub fn summary(&self) -> String {
        let size_mib = self.current_size / (1024 * 1024);
        let app = String::from_utf8_lossy(&self.creator_app);
        format!(
            "VHD {}, {} MiB, created by '{}', C/H/S={}/{}/{}",
            self.disk_type,
            size_mib,
            app.trim(),
            self.disk_geometry.cylinders,
            self.disk_geometry.heads,
            self.disk_geometry.sectors_per_track,
        )
    }
}

/// Dynamic VHD disk header (follows the footer copy at `data_offset`).
#[derive(Debug, Clone)]
pub struct VhdDynamicHeader {
    pub cookie: [u8; 8],
    pub data_offset: u64, // unused, 0xFFFFFFFFFFFFFFFF
    pub table_offset: u64,
    pub header_version: u32,
    pub max_table_entries: u32,
    pub block_size: u32, // typically 2 MiB (0x200000)
}

impl VhdDynamicHeader {
    fn parse<R: Read>(reader: &mut R) -> Result<Self> {
        let mut cookie = [0u8; 8];
        reader.read_exact(&mut cookie)?;
        if &cookie != VHD_DYNAMIC_MAGIC {
            bail!("Invalid VHD dynamic header: cookie {:?}", &cookie);
        }

        let data_offset = reader.read_u64::<BigEndian>()?;
        let table_offset = reader.read_u64::<BigEndian>()?;
        let header_version = reader.read_u32::<BigEndian>()?;
        let max_table_entries = reader.read_u32::<BigEndian>()?;
        let block_size = reader.read_u32::<BigEndian>()?;

        Ok(VhdDynamicHeader {
            cookie,
            data_offset,
            table_offset,
            header_version,
            max_table_entries,
            block_size,
        })
    }
}

/// Streaming reader for VHD (Fixed) images.
/// Fixed VHDs store raw data followed by a 512-byte footer.
pub struct VhdFixedReader<R: Read + Seek> {
    inner: R,
    size: u64,
    pos: u64,
}

impl<R: Read + Seek> VhdFixedReader<R> {
    fn new(mut inner: R, footer: &VhdFooter) -> Result<Self> {
        inner.seek(SeekFrom::Start(0))?;
        Ok(VhdFixedReader {
            inner,
            size: footer.current_size,
            pos: 0,
        })
    }
}

impl<R: Read + Seek> Read for VhdFixedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.size {
            return Ok(0);
        }
        let remaining = (self.size - self.pos) as usize;
        let to_read = buf.len().min(remaining);
        let n = self.inner.read(&mut buf[..to_read])?;
        self.pos += n as u64;
        Ok(n)
    }
}

/// Streaming reader for VHD (Dynamic) images.
/// Walks the Block Allocation Table (BAT) to resolve block → file offset mapping.
/// Each block has a sector bitmap followed by data sectors.
pub struct VhdDynamicReader<R: Read + Seek> {
    inner: R,
    footer: VhdFooter,
    dynamic_header: VhdDynamicHeader,
    bat: Vec<u32>,
    /// Sectors per block (block_size / 512)
    sectors_per_block: u32,
    /// Bitmap size in bytes (ceil(sectors_per_block / 8)), rounded up to sector boundary
    bitmap_size: u32,
    pos: u64,
}

impl<R: Read + Seek> VhdDynamicReader<R> {
    fn new(mut inner: R, footer: VhdFooter) -> Result<Self> {
        // Read dynamic header
        inner.seek(SeekFrom::Start(footer.data_offset))?;
        let dynamic_header = VhdDynamicHeader::parse(&mut inner)?;

        // Read BAT
        inner.seek(SeekFrom::Start(dynamic_header.table_offset))?;
        let mut bat = Vec::with_capacity(dynamic_header.max_table_entries as usize);
        for _ in 0..dynamic_header.max_table_entries {
            bat.push(inner.read_u32::<BigEndian>()?);
        }

        let sectors_per_block = dynamic_header.block_size / 512;
        // Bitmap: one bit per sector, rounded up to next 512-byte boundary
        let bitmap_bits = sectors_per_block;
        let bitmap_bytes = (bitmap_bits + 7) / 8;
        let bitmap_size = ((bitmap_bytes + 511) / 512) * 512;

        Ok(VhdDynamicReader {
            inner,
            footer,
            dynamic_header,
            bat,
            sectors_per_block,
            bitmap_size,
            pos: 0,
        })
    }

    fn read_at_offset(&mut self, virtual_offset: u64, buf: &mut [u8]) -> Result<usize> {
        let block_size = self.dynamic_header.block_size as u64;
        let block_index = (virtual_offset / block_size) as usize;
        let offset_in_block = virtual_offset % block_size;

        let remaining_in_block = (block_size - offset_in_block) as usize;
        let remaining_in_disk = (self.footer.current_size - virtual_offset) as usize;
        let to_read = buf.len().min(remaining_in_block).min(remaining_in_disk);

        if block_index >= self.bat.len() {
            buf[..to_read].fill(0);
            return Ok(to_read);
        }

        let bat_entry = self.bat[block_index];
        if bat_entry == 0xFFFFFFFF {
            // Sparse / unallocated block → zeros
            buf[..to_read].fill(0);
            return Ok(to_read);
        }

        // BAT entry is the sector number (multiply by 512 for byte offset)
        let block_file_offset = bat_entry as u64 * 512;
        // Skip the bitmap to get to data
        let data_offset = block_file_offset + self.bitmap_size as u64 + offset_in_block;

        self.inner.seek(SeekFrom::Start(data_offset))?;
        self.inner.read_exact(&mut buf[..to_read])?;
        Ok(to_read)
    }
}

impl<R: Read + Seek> Read for VhdDynamicReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.footer.current_size {
            return Ok(0);
        }
        let n = self
            .read_at_offset(self.pos, buf)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        self.pos += n as u64;
        Ok(n)
    }
}

// ─── VHDX (Modern) ────────────────────────────────────────────────────────

/// VHDX file identifier magic: "vhdxfile"
const VHDX_FILE_MAGIC: &[u8; 8] = b"vhdxfile";
/// VHDX header signature: "head"
const VHDX_HEADER_SIG: u32 = 0x64616568; // "head" in LE

/// Parsed VHDX file identifier (first 64 KiB).
#[derive(Debug, Clone)]
pub struct VhdxFileIdentifier {
    pub signature: [u8; 8],
    pub creator: String,
}

/// Parsed VHDX header (one of two copies at 64 KiB and 128 KiB).
#[derive(Debug, Clone)]
pub struct VhdxHeader {
    pub signature: u32,
    pub checksum: u32,
    pub sequence_number: u64,
    pub log_guid: [u8; 16],
    pub file_write_guid: [u8; 16],
    pub data_write_guid: [u8; 16],
    pub version: u32,
    pub log_length: u32,
    pub log_offset: u64,
}

/// Minimal VHDX metadata needed for reading.
#[derive(Debug, Clone)]
pub struct VhdxInfo {
    pub file_id: VhdxFileIdentifier,
    pub header: VhdxHeader,
    pub virtual_size: u64,
    pub block_size: u32,
    pub logical_sector_size: u32,
}

impl VhdxInfo {
    /// Human-readable summary.
    pub fn summary(&self) -> String {
        let size_mib = self.virtual_size / (1024 * 1024);
        format!(
            "VHDX v{}, {} MiB, {} KiB blocks, {} byte sectors, creator: {}",
            self.header.version,
            size_mib,
            self.block_size / 1024,
            self.logical_sector_size,
            self.file_id.creator,
        )
    }
}

/// Parse a VHDX file identifier and first header.
pub fn parse_vhdx<R: Read + Seek>(reader: &mut R) -> Result<VhdxInfo> {
    reader.seek(SeekFrom::Start(0))?;

    // File identifier — first 64 KiB
    let mut sig = [0u8; 8];
    reader.read_exact(&mut sig)?;
    if &sig != VHDX_FILE_MAGIC {
        bail!("Not a VHDX image: signature {:?}", &sig);
    }

    // Creator (UTF-16LE, up to 256 chars = 512 bytes at offset 8)
    let mut creator_bytes = [0u8; 512];
    reader.read_exact(&mut creator_bytes)?;
    let creator: String = creator_bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|&c| c != 0)
        .map(|c| char::from(c as u8))
        .collect();

    let file_id = VhdxFileIdentifier {
        signature: sig,
        creator,
    };

    // Header 1 at offset 64 KiB (0x10000)
    reader.seek(SeekFrom::Start(0x10000))?;
    let h_sig = reader.read_u32::<LittleEndian>()?;
    let h_checksum = reader.read_u32::<LittleEndian>()?;
    let h_sequence = reader.read_u64::<LittleEndian>()?;

    // File write GUID (16 bytes)
    let mut file_write_guid = [0u8; 16];
    reader.read_exact(&mut file_write_guid)?;
    let mut data_write_guid = [0u8; 16];
    reader.read_exact(&mut data_write_guid)?;
    let mut log_guid = [0u8; 16];
    reader.read_exact(&mut log_guid)?;

    // Log GUID, version, log length, log offset
    let _log_version = reader.read_u16::<LittleEndian>()?;
    let version = reader.read_u16::<LittleEndian>()? as u32;
    let log_length = reader.read_u32::<LittleEndian>()?;
    let log_offset = reader.read_u64::<LittleEndian>()?;

    let header = VhdxHeader {
        signature: h_sig,
        checksum: h_checksum,
        sequence_number: h_sequence,
        log_guid,
        file_write_guid,
        data_write_guid,
        version,
        log_length,
        log_offset,
    };

    // For now, use defaults for metadata fields that require full region table parsing.
    // A production implementation would parse the region table and metadata regions.
    let virtual_size = 0; // would come from metadata
    let block_size = 32 * 1024 * 1024; // typical 32 MiB
    let logical_sector_size = 512;

    Ok(VhdxInfo {
        file_id,
        header,
        virtual_size,
        block_size,
        logical_sector_size,
    })
}

// ─── Public API ────────────────────────────────────────────────────────────

/// Parse a VHD image and return footer information.
pub fn parse_vhd(path: &std::path::Path) -> Result<VhdFooter> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open VHD image: {}", path.display()))?;

    // Footer is at the last 512 bytes
    let file_size = file.seek(SeekFrom::End(0))?;
    if file_size < VHD_FOOTER_SIZE {
        bail!("File too small to be a VHD image: {} bytes", file_size);
    }
    file.seek(SeekFrom::Start(file_size - VHD_FOOTER_SIZE))?;

    let mut footer_data = [0u8; 512];
    file.read_exact(&mut footer_data)?;

    // Try footer at end first; if that fails, try header copy at offset 0
    match VhdFooter::parse(&footer_data) {
        Ok(f) => Ok(f),
        Err(_) => {
            file.seek(SeekFrom::Start(0))?;
            file.read_exact(&mut footer_data)?;
            VhdFooter::parse(&footer_data)
                .context("Failed to parse VHD footer at both end and beginning of file")
        }
    }
}

/// Open a VHD image and return a reader that emits raw data.
pub fn open_vhd(path: &std::path::Path) -> Result<Box<dyn Read + Send>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open VHD image: {}", path.display()))?;

    let file_size = file.seek(SeekFrom::End(0))?;
    if file_size < VHD_FOOTER_SIZE {
        bail!("File too small to be a VHD image: {} bytes", file_size);
    }

    // Read footer from end
    file.seek(SeekFrom::Start(file_size - VHD_FOOTER_SIZE))?;
    let mut footer_data = [0u8; 512];
    file.read_exact(&mut footer_data)?;
    let footer = VhdFooter::parse(&footer_data)?;

    match footer.disk_type {
        VhdDiskType::Fixed => {
            let reader = VhdFixedReader::new(file, &footer)?;
            Ok(Box::new(reader))
        }
        VhdDiskType::Dynamic => {
            let reader = VhdDynamicReader::new(file, footer)?;
            Ok(Box::new(reader))
        }
        VhdDiskType::Differencing => {
            bail!("Differencing VHD images are not yet supported")
        }
        other => bail!("Unsupported VHD disk type: {:?}", other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::WriteBytesExt;
    use std::io::{Cursor, Write};

    /// Build a minimal valid Fixed VHD image in memory.
    /// Data is placed at the start, footer at the end.
    fn build_test_fixed_vhd(data: &[u8], virtual_size: u64) -> Vec<u8> {
        let mut img = vec![0u8; virtual_size as usize + 512];

        // Write data at the beginning
        let len = data.len().min(virtual_size as usize);
        img[..len].copy_from_slice(&data[..len]);

        // Build footer at the end
        let footer_start = virtual_size as usize;
        let footer = &mut img[footer_start..footer_start + 512];

        // Cookie
        footer[0..8].copy_from_slice(VHD_MAGIC);
        let mut c = Cursor::new(&mut footer[8..]);
        c.write_u32::<BigEndian>(0x00000002).unwrap(); // features: reserved
        c.write_u32::<BigEndian>(0x00010000).unwrap(); // format_version
        c.write_u64::<BigEndian>(0xFFFFFFFFFFFFFFFF).unwrap(); // data_offset (none for fixed)
        c.write_u32::<BigEndian>(0).unwrap(); // timestamp
        c.write_all(b"abt ").unwrap(); // creator_app
        c.write_u32::<BigEndian>(0x00010000).unwrap(); // creator_version
        c.write_u32::<BigEndian>(0x5769326B).unwrap(); // creator_host_os (Wi2k = Windows)
        c.write_u64::<BigEndian>(virtual_size).unwrap(); // original_size
        c.write_u64::<BigEndian>(virtual_size).unwrap(); // current_size
        // geometry: 10 cylinders, 16 heads, 63 sectors
        c.write_u16::<BigEndian>(10).unwrap();
        c.write_u8(16).unwrap();
        c.write_u8(63).unwrap();
        c.write_u32::<BigEndian>(VhdDiskType::Fixed as u32).unwrap(); // disk_type
        c.write_u32::<BigEndian>(0).unwrap(); // checksum (simplified)
        c.write_all(&[0u8; 16]).unwrap(); // unique_id
        c.write_u8(0).unwrap(); // saved_state

        img
    }

    #[test]
    fn test_parse_fixed_footer() {
        let img = build_test_fixed_vhd(&[0xAA; 512], 65536);
        let footer_data = &img[65536..65536 + 512];
        let footer = VhdFooter::parse(footer_data).unwrap();

        assert_eq!(&footer.cookie, VHD_MAGIC);
        assert_eq!(footer.disk_type, VhdDiskType::Fixed);
        assert_eq!(footer.current_size, 65536);
        assert_eq!(footer.disk_geometry.cylinders, 10);
        assert_eq!(footer.disk_geometry.heads, 16);
    }

    #[test]
    fn test_fixed_reader() {
        let pattern: Vec<u8> = (0..512).map(|i| (i % 256) as u8).collect();
        let img = build_test_fixed_vhd(&pattern, 65536);

        let footer_data = &img[65536..65536 + 512];
        let footer = VhdFooter::parse(footer_data).unwrap();

        let cursor = Cursor::new(&img);
        let mut reader = VhdFixedReader::new(cursor, &footer).unwrap();

        let mut buf = vec![0u8; 512];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, pattern);
    }

    #[test]
    fn test_fixed_reader_eof() {
        let img = build_test_fixed_vhd(&[0xBB; 64], 4096);
        let footer_data = &img[4096..4096 + 512];
        let footer = VhdFooter::parse(footer_data).unwrap();

        let cursor = Cursor::new(&img);
        let mut reader = VhdFixedReader::new(cursor, &footer).unwrap();

        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), 4096);
        assert_eq!(all[0..64], vec![0xBB; 64][..]);
    }

    #[test]
    fn test_invalid_footer_magic() {
        let data = [0u8; 512];
        assert!(VhdFooter::parse(&data).is_err());
    }

    #[test]
    fn test_footer_summary() {
        let img = build_test_fixed_vhd(&[], 1024 * 1024);
        let footer_data = &img[1024 * 1024..1024 * 1024 + 512];
        let footer = VhdFooter::parse(footer_data).unwrap();
        let summary = footer.summary();
        assert!(summary.contains("VHD"));
        assert!(summary.contains("Fixed"));
        assert!(summary.contains("1 MiB"));
    }

    #[test]
    fn test_vhdx_file_identifier() {
        let mut img = vec![0u8; 0x20000]; // 128 KiB minimum
        img[0..8].copy_from_slice(VHDX_FILE_MAGIC);

        // Write creator as UTF-16LE: "abt"
        let creator = "abt";
        for (i, ch) in creator.chars().enumerate() {
            img[8 + i * 2] = ch as u8;
            img[8 + i * 2 + 1] = 0;
        }

        // Write header signature at 0x10000
        let mut c = Cursor::new(&mut img[0x10000..]);
        c.write_u32::<LittleEndian>(VHDX_HEADER_SIG).unwrap();

        let mut cursor = Cursor::new(&img);
        let info = parse_vhdx(&mut cursor).unwrap();
        assert_eq!(&info.file_id.signature, VHDX_FILE_MAGIC);
        assert_eq!(info.file_id.creator, "abt");
    }

    #[test]
    fn test_invalid_vhdx_magic() {
        let img = vec![0u8; 0x20000];
        let mut cursor = Cursor::new(&img);
        assert!(parse_vhdx(&mut cursor).is_err());
    }

    #[test]
    fn test_data_integrity_fixed_vhd() {
        let pattern: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let img = build_test_fixed_vhd(&pattern, 8192);
        let footer_data = &img[8192..8192 + 512];
        let footer = VhdFooter::parse(footer_data).unwrap();

        let cursor = Cursor::new(&img);
        let mut reader = VhdFixedReader::new(cursor, &footer).unwrap();

        let mut result = Vec::new();
        let mut buf = [0u8; 137]; // odd chunk size
        loop {
            let n = reader.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            result.extend_from_slice(&buf[..n]);
            if result.len() >= 4096 {
                break;
            }
        }
        assert_eq!(&result[..4096], &pattern[..]);
    }
}
