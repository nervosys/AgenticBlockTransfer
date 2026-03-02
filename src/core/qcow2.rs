// QCOW2 image reader — Copy-On-Write v2/v3 disk image format
//
// Parses QCOW2 headers and provides a streaming Read implementation that
// transparently converts QCOW2 cluster chains into raw block data.
// Supports version 2 and 3 images, uncompressed clusters, and zero clusters.
//
// Reference: https://github.com/qemu/qemu/blob/master/docs/interop/qcow2.txt

#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{self, Read, Seek, SeekFrom};

/// QCOW2 magic number: "QFI\xfb"
const QCOW2_MAGIC: u32 = 0x514649fb;

/// L2 entry flags
const L2_ENTRY_COMPRESSED: u64 = 1 << 62;
const L2_ENTRY_ALL_ZEROS: u64 = 1 << 0; // v3 zero flag
const L2_OFFSET_MASK: u64 = 0x00FF_FFFF_FFFF_FE00; // bits 9..55

/// QCOW2 file header (versions 2 and 3).
#[derive(Debug, Clone)]
pub struct Qcow2Header {
    pub magic: u32,
    pub version: u32,
    pub backing_file_offset: u64,
    pub backing_file_size: u32,
    pub cluster_bits: u32,
    pub size: u64, // virtual disk size in bytes
    pub crypt_method: u32,
    pub l1_size: u32,
    pub l1_table_offset: u64,
    pub refcount_table_offset: u64,
    pub refcount_table_clusters: u32,
    pub nb_snapshots: u32,
    pub snapshots_offset: u64,
    // v3 fields
    pub incompatible_features: u64,
    pub compatible_features: u64,
    pub autoclear_features: u64,
    pub refcount_order: u32, // default 4 → 16-bit refcounts
    pub header_length: u32,
}

impl Qcow2Header {
    /// Parse a QCOW2 header from a seekable reader.
    pub fn parse<R: Read + Seek>(reader: &mut R) -> Result<Self> {
        reader.seek(SeekFrom::Start(0))?;

        let magic = reader.read_u32::<BigEndian>()?;
        if magic != QCOW2_MAGIC {
            bail!(
                "Not a QCOW2 image: magic {:#010x}, expected {:#010x}",
                magic,
                QCOW2_MAGIC
            );
        }

        let version = reader.read_u32::<BigEndian>()?;
        if version != 2 && version != 3 {
            bail!("Unsupported QCOW2 version: {}", version);
        }

        let backing_file_offset = reader.read_u64::<BigEndian>()?;
        let backing_file_size = reader.read_u32::<BigEndian>()?;
        let cluster_bits = reader.read_u32::<BigEndian>()?;
        let size = reader.read_u64::<BigEndian>()?;
        let crypt_method = reader.read_u32::<BigEndian>()?;
        let l1_size = reader.read_u32::<BigEndian>()?;
        let l1_table_offset = reader.read_u64::<BigEndian>()?;
        let refcount_table_offset = reader.read_u64::<BigEndian>()?;
        let refcount_table_clusters = reader.read_u32::<BigEndian>()?;
        let nb_snapshots = reader.read_u32::<BigEndian>()?;
        let snapshots_offset = reader.read_u64::<BigEndian>()?;

        // v3 extended fields (default to 0 for v2)
        let (incompatible_features, compatible_features, autoclear_features, refcount_order, header_length) =
            if version >= 3 {
                let inc = reader.read_u64::<BigEndian>()?;
                let com = reader.read_u64::<BigEndian>()?;
                let aut = reader.read_u64::<BigEndian>()?;
                let rco = reader.read_u32::<BigEndian>()?;
                let hlen = reader.read_u32::<BigEndian>()?;
                (inc, com, aut, rco, hlen)
            } else {
                (0, 0, 0, 4, 72) // v2 header is 72 bytes
            };

        // Validate cluster_bits (typically 16 for 64 KiB clusters, range 9..21)
        if !(9..=21).contains(&cluster_bits) {
            bail!(
                "Invalid cluster_bits {} (must be 9..21)",
                cluster_bits
            );
        }

        if crypt_method != 0 {
            bail!("Encrypted QCOW2 images are not supported");
        }

        Ok(Qcow2Header {
            magic,
            version,
            backing_file_offset,
            backing_file_size,
            cluster_bits,
            size,
            crypt_method,
            l1_size,
            l1_table_offset,
            refcount_table_offset,
            refcount_table_clusters,
            nb_snapshots,
            snapshots_offset,
            incompatible_features,
            compatible_features,
            autoclear_features,
            refcount_order,
            header_length,
        })
    }

    /// Cluster size in bytes (2^cluster_bits).
    pub fn cluster_size(&self) -> u64 {
        1u64 << self.cluster_bits
    }

    /// Number of L2 entries per cluster (cluster_size / 8).
    pub fn l2_entries_per_cluster(&self) -> u64 {
        self.cluster_size() / 8
    }

    /// Whether this image has a backing file (overlay/snapshot).
    pub fn has_backing_file(&self) -> bool {
        self.backing_file_offset != 0 && self.backing_file_size > 0
    }

    /// Human-readable summary.
    pub fn summary(&self) -> String {
        let size_mib = self.size / (1024 * 1024);
        format!(
            "QCOW2 v{}, {} MiB virtual, {} KiB clusters, {} L1 entries{}",
            self.version,
            size_mib,
            self.cluster_size() / 1024,
            self.l1_size,
            if self.has_backing_file() {
                " (has backing file)"
            } else {
                ""
            }
        )
    }
}

/// Streaming reader that converts QCOW2 → raw data.
///
/// Implements `Read` so it can be directly plugged into the write pipeline.
/// Walks L1 → L2 → data cluster chains and emits raw bytes sequentially.
pub struct Qcow2Reader<R: Read + Seek> {
    inner: R,
    header: Qcow2Header,
    l1_table: Vec<u64>,
    /// Cache of the currently loaded L2 table (l2_index, entries)
    l2_cache: Option<(u64, Vec<u64>)>,
    /// Current byte offset in the virtual disk.
    pos: u64,
    /// Pre-allocated zero buffer (one cluster).
    zero_buf: Vec<u8>,
}

impl<R: Read + Seek> Qcow2Reader<R> {
    /// Open a QCOW2 image for reading.
    pub fn open(mut inner: R) -> Result<Self> {
        let header = Qcow2Header::parse(&mut inner)?;

        if header.has_backing_file() {
            bail!("QCOW2 images with backing files (overlays) are not yet supported");
        }

        // Read L1 table
        inner.seek(SeekFrom::Start(header.l1_table_offset))?;
        let mut l1_table = Vec::with_capacity(header.l1_size as usize);
        for _ in 0..header.l1_size {
            l1_table.push(inner.read_u64::<BigEndian>()?);
        }

        let cluster_size = header.cluster_size() as usize;
        let zero_buf = vec![0u8; cluster_size];

        Ok(Qcow2Reader {
            inner,
            header,
            l1_table,
            l2_cache: None,
            pos: 0,
            zero_buf,
        })
    }

    /// Virtual disk size.
    pub fn virtual_size(&self) -> u64 {
        self.header.size
    }

    /// Reference to the parsed header.
    pub fn header(&self) -> &Qcow2Header {
        &self.header
    }

    /// Load an L2 table from disk (with simple caching).
    fn load_l2_table(&mut self, l1_index: u64) -> Result<&[u64]> {
        // Check cache
        if let Some((cached_idx, _)) = &self.l2_cache {
            if *cached_idx == l1_index {
                return Ok(&self.l2_cache.as_ref().unwrap().1);
            }
        }

        let l1_entry = self.l1_table[l1_index as usize];
        let l2_offset = l1_entry & L2_OFFSET_MASK;

        if l2_offset == 0 {
            // Unallocated L1 entry → entire L2 range is zeros
            let count = self.header.l2_entries_per_cluster() as usize;
            self.l2_cache = Some((l1_index, vec![0u64; count]));
            return Ok(&self.l2_cache.as_ref().unwrap().1);
        }

        self.inner.seek(SeekFrom::Start(l2_offset))?;
        let count = self.header.l2_entries_per_cluster() as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(self.inner.read_u64::<BigEndian>()?);
        }
        self.l2_cache = Some((l1_index, entries));
        Ok(&self.l2_cache.as_ref().unwrap().1)
    }

    /// Read data from a single cluster at the given virtual offset, filling `buf`.
    /// Returns the number of bytes read.
    fn read_cluster_data(&mut self, virtual_offset: u64, buf: &mut [u8]) -> Result<usize> {
        let cluster_size = self.header.cluster_size();
        let l2_entries = self.header.l2_entries_per_cluster();

        // Compute indices
        let l1_index = virtual_offset / (cluster_size * l2_entries);
        let l2_index = (virtual_offset / cluster_size) % l2_entries;
        let offset_in_cluster = virtual_offset % cluster_size;

        let bytes_remaining_in_cluster = (cluster_size - offset_in_cluster) as usize;
        let to_read = buf.len().min(bytes_remaining_in_cluster);

        // Load L2 table
        let l2_table = self.load_l2_table(l1_index)?;
        let l2_entry = l2_table[l2_index as usize];

        if l2_entry & L2_ENTRY_COMPRESSED != 0 {
            bail!("Compressed QCOW2 clusters are not yet supported");
        }

        let data_offset = l2_entry & L2_OFFSET_MASK;

        if data_offset == 0 {
            // Unallocated cluster → read as zeros
            buf[..to_read].fill(0);
        } else {
            // Read from the physical offset
            self.inner
                .seek(SeekFrom::Start(data_offset + offset_in_cluster))?;
            self.inner.read_exact(&mut buf[..to_read])?;
        }

        Ok(to_read)
    }
}

impl<R: Read + Seek> Read for Qcow2Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.header.size {
            return Ok(0); // EOF
        }

        let remaining = (self.header.size - self.pos) as usize;
        let to_read = buf.len().min(remaining);

        if to_read == 0 {
            return Ok(0);
        }

        let n = self
            .read_cluster_data(self.pos, &mut buf[..to_read])
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        self.pos += n as u64;
        Ok(n)
    }
}

/// Parse a QCOW2 image and return header information.
pub fn parse_qcow2(path: &std::path::Path) -> Result<Qcow2Header> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open QCOW2 image: {}", path.display()))?;
    Qcow2Header::parse(&mut file)
}

/// Open a QCOW2 image and return a reader that emits raw data.
pub fn open_qcow2(path: &std::path::Path) -> Result<Qcow2Reader<std::fs::File>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open QCOW2 image: {}", path.display()))?;
    Qcow2Reader::open(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use byteorder::{BigEndian, WriteBytesExt};

    /// Build a minimal valid QCOW2 v2 image in memory.
    /// Creates a 1 MiB virtual disk with one allocated cluster containing test data.
    fn build_test_qcow2(data: &[u8]) -> Vec<u8> {
        let cluster_bits: u32 = 16; // 64 KiB clusters
        let cluster_size: u64 = 1 << cluster_bits;
        let virtual_size: u64 = 1024 * 1024; // 1 MiB
        let _l2_entries = cluster_size / 8;

        // Layout:
        //   Cluster 0: Header (72 bytes)
        //   Cluster 1: L1 table (1 entry)
        //   Cluster 2: L2 table (l2_entries entries)
        //   Cluster 3: Data cluster
        //   Cluster 4: Refcount table
        //   Cluster 5: Refcount block

        let l1_table_offset = cluster_size;       // cluster 1
        let l2_table_offset = cluster_size * 2;    // cluster 2
        let data_offset = cluster_size * 3;        // cluster 3
        let refcount_table_offset = cluster_size * 4; // cluster 4

        let l1_size: u32 = 1; // one L1 entry covers l2_entries clusters

        let total_size = cluster_size * 6;
        let mut img = vec![0u8; total_size as usize];

        // Write header at offset 0
        let mut cursor = Cursor::new(&mut img[..]);
        cursor.write_u32::<BigEndian>(QCOW2_MAGIC).unwrap();
        cursor.write_u32::<BigEndian>(2).unwrap(); // version
        cursor.write_u64::<BigEndian>(0).unwrap(); // backing_file_offset
        cursor.write_u32::<BigEndian>(0).unwrap(); // backing_file_size
        cursor.write_u32::<BigEndian>(cluster_bits).unwrap();
        cursor.write_u64::<BigEndian>(virtual_size).unwrap();
        cursor.write_u32::<BigEndian>(0).unwrap(); // crypt_method
        cursor.write_u32::<BigEndian>(l1_size).unwrap();
        cursor.write_u64::<BigEndian>(l1_table_offset).unwrap();
        cursor.write_u64::<BigEndian>(refcount_table_offset).unwrap();
        cursor.write_u32::<BigEndian>(1).unwrap(); // refcount_table_clusters
        cursor.write_u32::<BigEndian>(0).unwrap(); // nb_snapshots
        cursor.write_u64::<BigEndian>(0).unwrap(); // snapshots_offset

        // Write L1 table at cluster 1 — one entry pointing to L2 at cluster 2
        let l1_offset = l1_table_offset as usize;
        let mut c = Cursor::new(&mut img[l1_offset..]);
        c.write_u64::<BigEndian>(l2_table_offset).unwrap();

        // Write L2 table at cluster 2 — first entry points to data at cluster 3
        let l2_off = l2_table_offset as usize;
        let mut c = Cursor::new(&mut img[l2_off..]);
        c.write_u64::<BigEndian>(data_offset).unwrap();
        // Remaining L2 entries are 0 (unallocated → zeros)

        // Write data at cluster 3
        let data_off = data_offset as usize;
        let len = data.len().min(cluster_size as usize);
        img[data_off..data_off + len].copy_from_slice(&data[..len]);

        img
    }

    #[test]
    fn test_parse_header() {
        let img = build_test_qcow2(&[0xAA; 512]);
        let mut cursor = Cursor::new(&img);
        let header = Qcow2Header::parse(&mut cursor).unwrap();

        assert_eq!(header.magic, QCOW2_MAGIC);
        assert_eq!(header.version, 2);
        assert_eq!(header.cluster_bits, 16);
        assert_eq!(header.cluster_size(), 65536);
        assert_eq!(header.size, 1024 * 1024);
        assert_eq!(header.l1_size, 1);
        assert!(!header.has_backing_file());
    }

    #[test]
    fn test_reader_first_sector() {
        let pattern: Vec<u8> = (0..512).map(|i| (i % 256) as u8).collect();
        let img = build_test_qcow2(&pattern);

        let cursor = Cursor::new(&img);
        let mut reader = Qcow2Reader::open(cursor).unwrap();
        assert_eq!(reader.virtual_size(), 1024 * 1024);

        let mut buf = vec![0u8; 512];
        reader.read_exact(&mut buf).unwrap();
        assert_eq!(buf, pattern);
    }

    #[test]
    fn test_reader_unallocated_cluster() {
        // Read from the second cluster which is unallocated (L2[1] = 0)
        let img = build_test_qcow2(&[0xFF; 64]);
        let cursor = Cursor::new(&img);
        let mut reader = Qcow2Reader::open(cursor).unwrap();

        // Skip first cluster (64 KiB)
        let cluster_size = 65536;
        let mut discard = vec![0u8; cluster_size];
        reader.read_exact(&mut discard).unwrap();

        // Second cluster should be all zeros (unallocated)
        let mut buf = vec![0u8; 512];
        reader.read_exact(&mut buf).unwrap();
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_reader_eof() {
        let img = build_test_qcow2(&[0xBB; 16]);
        let cursor = Cursor::new(&img);
        let mut reader = Qcow2Reader::open(cursor).unwrap();

        // Read entire virtual disk
        let mut all = Vec::new();
        reader.read_to_end(&mut all).unwrap();
        assert_eq!(all.len(), 1024 * 1024);
    }

    #[test]
    fn test_invalid_magic() {
        let mut img = build_test_qcow2(&[]);
        img[0..4].copy_from_slice(&[0, 0, 0, 0]);
        let mut cursor = Cursor::new(&img);
        assert!(Qcow2Header::parse(&mut cursor).is_err());
    }

    #[test]
    fn test_unsupported_version() {
        let mut img = build_test_qcow2(&[]);
        // Set version to 99
        let mut c = Cursor::new(&mut img[4..8]);
        c.write_u32::<BigEndian>(99).unwrap();
        let mut cursor = Cursor::new(&img);
        assert!(Qcow2Header::parse(&mut cursor).is_err());
    }

    #[test]
    fn test_summary_format() {
        let img = build_test_qcow2(&[]);
        let mut cursor = Cursor::new(&img);
        let header = Qcow2Header::parse(&mut cursor).unwrap();
        let summary = header.summary();
        assert!(summary.contains("QCOW2"));
        assert!(summary.contains("v2"));
        assert!(summary.contains("1 MiB"));
    }

    #[test]
    fn test_data_integrity_across_reads() {
        let pattern: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let img = build_test_qcow2(&pattern);
        let cursor = Cursor::new(&img);
        let mut reader = Qcow2Reader::open(cursor).unwrap();

        // Read in small chunks
        let mut result = Vec::new();
        let mut buf = [0u8; 137]; // odd size to test boundary handling
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
