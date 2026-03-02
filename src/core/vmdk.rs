// VMDK image reader — VMware Virtual Machine Disk format
//
// Supports VMDK sparse extents (hosted sparse / monolithicSparse).
// Parses the sparse extent header (VMDK magic "KDMV" / 0x564d444b at byte 0)
// and the embedded grain directory → grain table → data grain chain.
//
// Reference: VMware Virtual Disk Format 5.0 specification
//            https://www.vmware.com/app/vmdk/?src=vmdk

#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{self, Read, Seek, SeekFrom};

/// VMDK sparse extent magic number: "VMDK" in little-endian → 0x564d444b
/// Stored as the first 4 bytes of a sparse VMDK file (as LE u32: 0x4b444d56 = "KDMV").
const VMDK_MAGIC: u32 = 0x564d444b;

/// Sector size used by VMDK (always 512 bytes).
const SECTOR_SIZE: u64 = 512;

/// VMDK sparse extent header (v1 / v2 / v3).
#[derive(Debug, Clone)]
pub struct VmdkSparseHeader {
    pub magic: u32,
    pub version: u32,
    pub flags: u32,
    /// Total capacity of the virtual disk in sectors.
    pub capacity_sectors: u64,
    /// Number of sectors per grain (data block).
    pub grain_size_sectors: u64,
    /// Offset (in sectors) of the embedded descriptor.
    pub descriptor_offset: u64,
    /// Size of the embedded descriptor in sectors.
    pub descriptor_size: u64,
    /// Number of grain table entries per grain table.
    pub num_gte_per_gt: u32,
    /// Offset (in sectors) of the redundant grain directory.
    pub rgd_offset: u64,
    /// Offset (in sectors) of the primary grain directory.
    pub gd_offset: u64,
    /// Overhead (in sectors) — first usable grain offset.
    pub overhead_sectors: u64,
    /// Whether the file was cleanly closed.
    pub unclean_shutdown: bool,
    /// Compression algorithm (0 = none, 1 = deflate).
    pub compress_algorithm: u16,
}

impl VmdkSparseHeader {
    /// Parse a VMDK sparse extent header from a seekable reader.
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        reader.seek(SeekFrom::Start(0))?;

        let magic = reader.read_u32::<LittleEndian>()?;
        if magic != VMDK_MAGIC {
            bail!(
                "Not a VMDK sparse image: magic {:#010x}, expected {:#010x}",
                magic,
                VMDK_MAGIC
            );
        }

        let version = reader.read_u32::<LittleEndian>()?;
        if version < 1 || version > 3 {
            bail!("Unsupported VMDK version: {}", version);
        }

        let flags = reader.read_u32::<LittleEndian>()?;
        let capacity_sectors = reader.read_u64::<LittleEndian>()?;
        let grain_size_sectors = reader.read_u64::<LittleEndian>()?;
        let descriptor_offset = reader.read_u64::<LittleEndian>()?;
        let descriptor_size = reader.read_u64::<LittleEndian>()?;
        let num_gte_per_gt = reader.read_u32::<LittleEndian>()?;
        let rgd_offset = reader.read_u64::<LittleEndian>()?;
        let gd_offset = reader.read_u64::<LittleEndian>()?;
        let overhead_sectors = reader.read_u64::<LittleEndian>()?;
        let unclean_shutdown = reader.read_u8()? != 0;
        // 4 bytes: single/double end-of-line marker ('\n', ' ', '\r', '\n')
        let mut _eol = [0u8; 4];
        reader.read_exact(&mut _eol)?;
        let compress_algorithm = reader.read_u16::<LittleEndian>()?;
        // 6 bytes padding
        let mut _pad = [0u8; 6];
        reader.read_exact(&mut _pad)?;

        if grain_size_sectors == 0 {
            bail!("Invalid grain size: 0 sectors");
        }

        if compress_algorithm > 1 {
            bail!(
                "Unsupported VMDK compression algorithm: {} (only none/deflate supported)",
                compress_algorithm
            );
        }

        Ok(VmdkSparseHeader {
            magic,
            version,
            flags,
            capacity_sectors,
            grain_size_sectors,
            descriptor_offset,
            descriptor_size,
            num_gte_per_gt,
            rgd_offset,
            gd_offset,
            overhead_sectors,
            unclean_shutdown,
            compress_algorithm,
        })
    }

    /// Virtual disk size in bytes.
    pub fn virtual_size(&self) -> u64 {
        self.capacity_sectors * SECTOR_SIZE
    }

    /// Grain size in bytes.
    pub fn grain_size(&self) -> u64 {
        self.grain_size_sectors * SECTOR_SIZE
    }

    /// Number of grain directory entries.
    pub fn gd_entry_count(&self) -> u64 {
        let grains = (self.capacity_sectors + self.grain_size_sectors - 1) / self.grain_size_sectors;
        (grains + self.num_gte_per_gt as u64 - 1) / self.num_gte_per_gt as u64
    }

    /// Whether deflate compression is used on grains.
    pub fn is_compressed(&self) -> bool {
        self.compress_algorithm == 1
    }

    /// Read the embedded descriptor text (if present).
    pub fn read_descriptor<R: Read + Seek>(&self, reader: &mut R) -> Result<Option<String>> {
        if self.descriptor_offset == 0 || self.descriptor_size == 0 {
            return Ok(None);
        }
        let offset = self.descriptor_offset * SECTOR_SIZE;
        let size = (self.descriptor_size * SECTOR_SIZE) as usize;
        reader.seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; size];
        reader.read_exact(&mut buf)?;
        // Descriptor is NUL-padded ASCII text
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Ok(Some(String::from_utf8_lossy(&buf[..end]).to_string()))
    }

    /// Human-readable summary.
    pub fn summary(&self) -> String {
        let size_mib = self.virtual_size() / (1024 * 1024);
        let grain_kib = self.grain_size() / 1024;
        format!(
            "VMDK v{}, {} MiB virtual, {} KiB grains, {} GD entries{}{}",
            self.version,
            size_mib,
            grain_kib,
            self.gd_entry_count(),
            if self.is_compressed() {
                ", deflate"
            } else {
                ""
            },
            if self.unclean_shutdown {
                " (unclean shutdown)"
            } else {
                ""
            }
        )
    }
}

/// Streaming reader that converts a VMDK sparse extent → raw data.
///
/// Walks grain directory → grain table → data grain chains and emits
/// sequential raw bytes. Implements `Read` for pipeline integration.
pub struct VmdkReader<R: Read + Seek> {
    inner: R,
    header: VmdkSparseHeader,
    /// Grain directory: maps GD index → sector offset of grain table.
    gd: Vec<u32>,
    /// Cache of currently loaded grain table (gd_index, entries).
    gt_cache: Option<(usize, Vec<u32>)>,
    /// Current byte offset in the virtual disk.
    pos: u64,
}

impl<R: Read + Seek> VmdkReader<R> {
    /// Open a VMDK sparse image for reading.
    pub fn open(mut inner: R) -> Result<Self> {
        let header = VmdkSparseHeader::parse(&mut inner)?;

        if header.is_compressed() {
            bail!("Compressed VMDK grains (deflate) are not yet supported in streaming mode");
        }

        // Read grain directory
        let gd_count = header.gd_entry_count() as usize;
        let gd_offset = header.gd_offset * SECTOR_SIZE;
        inner.seek(SeekFrom::Start(gd_offset))?;

        let mut gd = Vec::with_capacity(gd_count);
        for _ in 0..gd_count {
            gd.push(inner.read_u32::<LittleEndian>()?);
        }

        Ok(VmdkReader {
            inner,
            header,
            gd,
            gt_cache: None,
            pos: 0,
        })
    }

    /// Virtual disk size.
    pub fn virtual_size(&self) -> u64 {
        self.header.virtual_size()
    }

    /// Reference to the parsed header.
    pub fn header(&self) -> &VmdkSparseHeader {
        &self.header
    }

    /// Load a grain table from disk (with simple caching).
    fn load_grain_table(&mut self, gd_index: usize) -> Result<&[u32]> {
        if let Some((cached_idx, _)) = &self.gt_cache {
            if *cached_idx == gd_index {
                return Ok(&self.gt_cache.as_ref().unwrap().1);
            }
        }

        let gt_sector = self.gd[gd_index];
        if gt_sector == 0 {
            // Unallocated grain table → entire range reads as zeros
            let count = self.header.num_gte_per_gt as usize;
            self.gt_cache = Some((gd_index, vec![0u32; count]));
            return Ok(&self.gt_cache.as_ref().unwrap().1);
        }

        let gt_offset = gt_sector as u64 * SECTOR_SIZE;
        self.inner.seek(SeekFrom::Start(gt_offset))?;

        let count = self.header.num_gte_per_gt as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(self.inner.read_u32::<LittleEndian>()?);
        }

        self.gt_cache = Some((gd_index, entries));
        Ok(&self.gt_cache.as_ref().unwrap().1)
    }

    /// Read data from a single grain at the given virtual offset.
    fn read_grain_data(&mut self, virtual_offset: u64, buf: &mut [u8]) -> Result<usize> {
        let grain_size = self.header.grain_size();
        let gte_per_gt = self.header.num_gte_per_gt as u64;

        // Global grain index
        let grain_index = virtual_offset / grain_size;
        // Which grain directory entry
        let gd_index = (grain_index / gte_per_gt) as usize;
        // Which grain table entry within this GT
        let gt_index = (grain_index % gte_per_gt) as usize;
        // Offset within the grain
        let offset_in_grain = virtual_offset % grain_size;

        let bytes_remaining = (grain_size - offset_in_grain) as usize;
        let to_read = buf.len().min(bytes_remaining);

        let gt = self.load_grain_table(gd_index)?;
        let grain_sector = gt[gt_index];

        if grain_sector == 0 {
            // Unallocated grain → zeros
            buf[..to_read].fill(0);
        } else {
            let data_offset = grain_sector as u64 * SECTOR_SIZE + offset_in_grain;
            self.inner.seek(SeekFrom::Start(data_offset))?;
            self.inner.read_exact(&mut buf[..to_read])?;
        }

        Ok(to_read)
    }
}

impl<R: Read + Seek> Read for VmdkReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let capacity = self.header.virtual_size();
        if self.pos >= capacity {
            return Ok(0); // EOF
        }

        let remaining = (capacity - self.pos) as usize;
        let to_read = buf.len().min(remaining);

        if to_read == 0 {
            return Ok(0);
        }

        let n = self
            .read_grain_data(self.pos, &mut buf[..to_read])
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        self.pos += n as u64;
        Ok(n)
    }
}

/// Parse a VMDK sparse image and return header information.
pub fn parse_vmdk(path: &std::path::Path) -> Result<VmdkSparseHeader> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open VMDK image: {}", path.display()))?;
    VmdkSparseHeader::parse(&mut file)
}

/// Open a VMDK sparse image and return a reader that emits raw data.
pub fn open_vmdk(path: &std::path::Path) -> Result<VmdkReader<std::fs::File>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open VMDK image: {}", path.display()))?;
    VmdkReader::open(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::{LittleEndian, WriteBytesExt};
    use std::io::Cursor;

    /// Build a minimal valid VMDK sparse header in memory.
    fn build_test_header(
        capacity_sectors: u64,
        grain_size_sectors: u64,
        num_gte_per_gt: u32,
    ) -> Vec<u8> {
        let mut buf = Vec::new();
        // magic
        buf.write_u32::<LittleEndian>(VMDK_MAGIC).unwrap();
        // version
        buf.write_u32::<LittleEndian>(1).unwrap();
        // flags
        buf.write_u32::<LittleEndian>(0).unwrap();
        // capacity_sectors
        buf.write_u64::<LittleEndian>(capacity_sectors).unwrap();
        // grain_size_sectors
        buf.write_u64::<LittleEndian>(grain_size_sectors).unwrap();
        // descriptor_offset
        buf.write_u64::<LittleEndian>(0).unwrap();
        // descriptor_size
        buf.write_u64::<LittleEndian>(0).unwrap();
        // num_gte_per_gt
        buf.write_u32::<LittleEndian>(num_gte_per_gt).unwrap();
        // rgd_offset
        buf.write_u64::<LittleEndian>(0).unwrap();
        // gd_offset (we'll place the GD right after the header — sector 1)
        buf.write_u64::<LittleEndian>(1).unwrap();
        // overhead_sectors
        buf.write_u64::<LittleEndian>(2).unwrap();
        // unclean_shutdown
        buf.write_u8(0).unwrap();
        // 4-byte EOL markers
        buf.extend_from_slice(&[b'\n', b' ', b'\r', b'\n']);
        // compress_algorithm
        buf.write_u16::<LittleEndian>(0).unwrap();
        // 6 bytes padding
        buf.extend_from_slice(&[0u8; 6]);
        buf
    }

    #[test]
    fn test_parse_vmdk_header() {
        let data = build_test_header(2048, 128, 512);
        let mut cursor = Cursor::new(data);
        let header = VmdkSparseHeader::parse(&mut cursor).unwrap();
        assert_eq!(header.version, 1);
        assert_eq!(header.capacity_sectors, 2048);
        assert_eq!(header.grain_size_sectors, 128);
        assert_eq!(header.virtual_size(), 2048 * 512);
        assert_eq!(header.grain_size(), 128 * 512);
    }

    #[test]
    fn test_vmdk_summary() {
        let data = build_test_header(2 * 1024 * 1024, 128, 512);
        let mut cursor = Cursor::new(data);
        let header = VmdkSparseHeader::parse(&mut cursor).unwrap();
        let summary = header.summary();
        assert!(summary.contains("VMDK v1"));
        assert!(summary.contains("MiB virtual"));
    }

    #[test]
    fn test_bad_magic() {
        let mut data = build_test_header(2048, 128, 512);
        data[0] = 0xFF; // corrupt magic
        let mut cursor = Cursor::new(data);
        assert!(VmdkSparseHeader::parse(&mut cursor).is_err());
    }

    #[test]
    fn test_zero_grain_size() {
        let data = build_test_header(2048, 0, 512);
        let mut cursor = Cursor::new(data);
        assert!(VmdkSparseHeader::parse(&mut cursor).is_err());
    }

    #[test]
    fn test_gd_entry_count() {
        // 2048 sectors / 128 sectors per grain = 16 grains
        // 16 grains / 512 grain table entries per GT = 1 GD entry
        let data = build_test_header(2048, 128, 512);
        let mut cursor = Cursor::new(data);
        let header = VmdkSparseHeader::parse(&mut cursor).unwrap();
        assert_eq!(header.gd_entry_count(), 1);
    }

    #[test]
    fn test_vmdk_reader_unallocated() {
        // Build a VMDK with 1 grain (128 sectors = 64 KiB), 1 GD entry, GD at sector 1
        let mut data = build_test_header(128, 128, 512);
        // Pad to sector 1 (512 bytes)
        data.resize(512, 0);
        // Write grain directory entry: 0 → unallocated
        data.write_u32::<LittleEndian>(0).unwrap();
        // Pad rest
        data.resize(2048, 0);

        let mut cursor = Cursor::new(data);
        let mut reader = VmdkReader::open(&mut cursor).unwrap();
        assert_eq!(reader.virtual_size(), 128 * 512);

        // Read should return zeros
        let mut buf = vec![0xFFu8; 512];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 512);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_vmdk_reader_allocated_grain() {
        // 1 grain (128 sectors = 64 KiB), GD at sector 1, GT at sector 2, data at sector 4
        let grain_size_sectors = 128u64;
        let capacity_sectors = 128u64;
        let mut data = build_test_header(capacity_sectors, grain_size_sectors, 512);

        // Pad to sector 1 (GD offset)
        data.resize(512, 0);
        // GD entry: grain table at sector 2
        data.write_u32::<LittleEndian>(2).unwrap();
        // Pad to sector 2 (GT offset)
        data.resize(1024, 0);
        // GT entry 0: grain at sector 4
        data.write_u32::<LittleEndian>(4).unwrap();
        // Pad to sector 4 (data grain start)
        data.resize(2048, 0);
        // Write known data pattern
        let pattern: Vec<u8> = (0..512u16).map(|i| (i & 0xFF) as u8).collect();
        data.extend_from_slice(&pattern);
        // Pad remainder
        data.resize(2048 + 128 * 512, 0);

        let mut cursor = Cursor::new(data);
        let mut reader = VmdkReader::open(&mut cursor).unwrap();

        let mut buf = vec![0u8; 512];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 512);
        assert_eq!(buf, pattern);
    }

    #[test]
    fn test_vmdk_reader_eof() {
        let grain_size_sectors = 128u64;
        let capacity_sectors = 128u64;
        let mut data = build_test_header(capacity_sectors, grain_size_sectors, 512);
        data.resize(512, 0);
        // GD entry: unallocated
        data.write_u32::<LittleEndian>(0).unwrap();
        data.resize(2048, 0);

        let mut cursor = Cursor::new(data);
        let mut reader = VmdkReader::open(&mut cursor).unwrap();

        // Read entire virtual disk
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), (capacity_sectors * 512) as usize);

        // Next read should return 0 (EOF)
        let mut extra = [0u8; 64];
        assert_eq!(reader.read(&mut extra).unwrap(), 0);
    }
}
