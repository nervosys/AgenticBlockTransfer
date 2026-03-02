// ISO 9660 metadata parsing — read-only introspection of ISO images.
//
// Parses the Primary Volume Descriptor (PVD) at sector 16 (offset 0x8000) to
// extract volume label, system/publisher identifiers, volume size, and El Torito
// boot catalog presence.
//
// References:
//   ECMA-119 (ISO 9660:1988)
//   El Torito Bootable CD Specification v1.0

use anyhow::{bail, Result};
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Sector size for ISO 9660 (always 2048 bytes).
const SECTOR_SIZE: u64 = 2048;

/// Offset of the first volume descriptor (sector 16).
const FIRST_VD_OFFSET: u64 = 16 * SECTOR_SIZE;

/// Volume descriptor types.
const VD_TYPE_BOOT_RECORD: u8 = 0;
const VD_TYPE_PRIMARY: u8 = 1;
const VD_TYPE_SUPPLEMENTARY: u8 = 2;
const VD_TYPE_TERMINATOR: u8 = 255;

/// Standard identifier for ISO 9660 volume descriptors.
const ISO_STANDARD_ID: &[u8; 5] = b"CD001";

/// El Torito boot system identifier.
const EL_TORITO_SPEC: &[u8] = b"EL TORITO SPECIFICATION";

/// Parsed ISO 9660 volume information.
#[derive(Debug, Clone)]
pub struct Iso9660Info {
    /// Volume identifier (label), trimmed.
    pub volume_id: String,
    /// System identifier, trimmed.
    pub system_id: String,
    /// Publisher identifier, trimmed.
    pub publisher_id: String,
    /// Data preparer identifier, trimmed.
    pub preparer_id: String,
    /// Application identifier, trimmed.
    pub application_id: String,
    /// Volume set identifier, trimmed.
    pub volume_set_id: String,
    /// Volume creation date (ISO 8601 string from PVD).
    pub creation_date: String,
    /// Volume modification date.
    pub modification_date: String,
    /// Logical block size (usually 2048).
    pub logical_block_size: u16,
    /// Total volume size in bytes.
    pub volume_size_bytes: u64,
    /// Number of logical blocks in the volume.
    pub volume_block_count: u32,
    /// Whether an El Torito boot record was found.
    pub is_bootable: bool,
    /// El Torito boot catalog sector (if bootable).
    pub boot_catalog_sector: Option<u32>,
    /// Whether a Joliet (supplementary) volume descriptor was found.
    pub has_joliet: bool,
}

impl fmt::Display for Iso9660Info {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "ISO 9660 Volume Information:")?;
        writeln!(f, "  Volume ID:        {}", self.volume_id)?;
        if !self.system_id.is_empty() {
            writeln!(f, "  System ID:        {}", self.system_id)?;
        }
        if !self.publisher_id.is_empty() {
            writeln!(f, "  Publisher:         {}", self.publisher_id)?;
        }
        if !self.preparer_id.is_empty() {
            writeln!(f, "  Data Preparer:     {}", self.preparer_id)?;
        }
        if !self.application_id.is_empty() {
            writeln!(f, "  Application:       {}", self.application_id)?;
        }
        writeln!(f, "  Block Size:        {} bytes", self.logical_block_size)?;
        writeln!(
            f,
            "  Volume Size:       {} ({} blocks)",
            humansize::format_size(self.volume_size_bytes, humansize::BINARY),
            self.volume_block_count
        )?;
        writeln!(f, "  Bootable:          {}", if self.is_bootable { "Yes (El Torito)" } else { "No" })?;
        if let Some(sector) = self.boot_catalog_sector {
            writeln!(f, "  Boot Catalog:      sector {}", sector)?;
        }
        writeln!(f, "  Joliet Extensions: {}", if self.has_joliet { "Yes" } else { "No" })?;
        if !self.creation_date.is_empty() {
            writeln!(f, "  Created:           {}", self.creation_date)?;
        }
        if !self.modification_date.is_empty() {
            writeln!(f, "  Modified:          {}", self.modification_date)?;
        }
        Ok(())
    }
}

/// Read ISO 9660 metadata from an ISO image file.
///
/// Reads the volume descriptor set starting at sector 16. Parses the Primary
/// Volume Descriptor for metadata and scans for El Torito boot records and
/// Joliet supplementary descriptors.
pub fn read_iso9660_info(path: &Path) -> Result<Iso9660Info> {
    let mut file = std::fs::File::open(path)?;
    let file_size = file.metadata()?.len();

    if file_size < FIRST_VD_OFFSET + SECTOR_SIZE {
        bail!("File too small to contain ISO 9660 volume descriptors");
    }

    parse_iso9660(&mut file)
}

/// Parse ISO 9660 volume descriptors from a seekable reader.
pub fn parse_iso9660<R: Read + Seek>(reader: &mut R) -> Result<Iso9660Info> {
    let mut info = Iso9660Info {
        volume_id: String::new(),
        system_id: String::new(),
        publisher_id: String::new(),
        preparer_id: String::new(),
        application_id: String::new(),
        volume_set_id: String::new(),
        creation_date: String::new(),
        modification_date: String::new(),
        logical_block_size: 0,
        volume_size_bytes: 0,
        volume_block_count: 0,
        is_bootable: false,
        boot_catalog_sector: None,
        has_joliet: false,
    };

    let mut sector = 16u64;
    let mut found_pvd = false;
    let mut vd_buf = [0u8; SECTOR_SIZE as usize];

    loop {
        let offset = sector * SECTOR_SIZE;
        reader.seek(SeekFrom::Start(offset))?;

        if reader.read(&mut vd_buf)? < SECTOR_SIZE as usize {
            break;
        }

        // Validate standard identifier
        if &vd_buf[1..6] != ISO_STANDARD_ID {
            bail!(
                "Invalid ISO 9660 standard identifier at sector {} (expected CD001)",
                sector
            );
        }

        let vd_type = vd_buf[0];
        let vd_version = vd_buf[6];

        match vd_type {
            VD_TYPE_TERMINATOR => break,

            VD_TYPE_PRIMARY => {
                if vd_version != 1 {
                    log::warn!("Unexpected PVD version {} (expected 1)", vd_version);
                }
                parse_pvd(&vd_buf, &mut info);
                found_pvd = true;
            }

            VD_TYPE_BOOT_RECORD => {
                parse_boot_record(&vd_buf, &mut info);
            }

            VD_TYPE_SUPPLEMENTARY => {
                // Check for Joliet: escape sequences in bytes 88-120
                let escape = &vd_buf[88..120];
                if escape.windows(3).any(|w| {
                    w == [0x25, 0x2F, 0x40]  // %/@  — UCS-2 Level 1
                    || w == [0x25, 0x2F, 0x43]  // %/C  — UCS-2 Level 2
                    || w == [0x25, 0x2F, 0x45]  // %/E  — UCS-2 Level 3
                }) {
                    info.has_joliet = true;
                }
            }

            _ => {
                // Unknown VD type — skip
                log::debug!("Skipping unknown volume descriptor type {} at sector {}", vd_type, sector);
            }
        }

        sector += 1;

        // Safety limit: don't scan more than 32 sectors of VDs
        if sector > 48 {
            break;
        }
    }

    if !found_pvd {
        bail!("No Primary Volume Descriptor found — not a valid ISO 9660 image");
    }

    Ok(info)
}

/// Parse the Primary Volume Descriptor (PVD) fields.
fn parse_pvd(buf: &[u8; 2048], info: &mut Iso9660Info) {
    // System Identifier: bytes 8–39 (32 bytes, a-characters)
    info.system_id = read_strA(buf, 8, 32);

    // Volume Identifier: bytes 40–71 (32 bytes, d-characters)
    info.volume_id = read_strA(buf, 40, 32);

    // Volume Set Identifier: bytes 190–317 (128 bytes)
    info.volume_set_id = read_strA(buf, 190, 128);

    // Publisher Identifier: bytes 318–445 (128 bytes)
    info.publisher_id = read_strA(buf, 318, 128);

    // Data Preparer Identifier: bytes 446–573 (128 bytes)
    info.preparer_id = read_strA(buf, 446, 128);

    // Application Identifier: bytes 574–701 (128 bytes)
    info.application_id = read_strA(buf, 574, 128);

    // Volume Space Size: bytes 80–87 (both-endian 32-bit)
    // The little-endian value is at offset 80, big-endian at 84
    info.volume_block_count = u32::from_le_bytes([buf[80], buf[81], buf[82], buf[83]]);

    // Logical Block Size: bytes 128–131 (both-endian 16-bit)
    info.logical_block_size = u16::from_le_bytes([buf[128], buf[129]]);

    info.volume_size_bytes = info.volume_block_count as u64 * info.logical_block_size as u64;

    // Volume Creation Date and Time: bytes 813–829 (17 bytes, dec-datetime)
    info.creation_date = read_dec_datetime(buf, 813);

    // Volume Modification Date and Time: bytes 830–846
    info.modification_date = read_dec_datetime(buf, 830);
}

/// Parse an El Torito Boot Record Volume Descriptor.
fn parse_boot_record(buf: &[u8; 2048], info: &mut Iso9660Info) {
    // Boot System Identifier: bytes 7–38 (32 bytes)
    let boot_sys_id = &buf[7..39];

    // Check for El Torito specification identifier
    if boot_sys_id.starts_with(EL_TORITO_SPEC) {
        info.is_bootable = true;

        // Boot Catalog pointer: bytes 71–74 (little-endian 32-bit sector number)
        let catalog_sector = u32::from_le_bytes([buf[71], buf[72], buf[73], buf[74]]);
        if catalog_sector > 0 {
            info.boot_catalog_sector = Some(catalog_sector);
        }
    }
}

/// Read a fixed-length ISO 9660 a-character / d-character string field, trimming
/// trailing spaces and NULs.
#[allow(non_snake_case)]
fn read_strA(buf: &[u8], offset: usize, len: usize) -> String {
    let slice = &buf[offset..offset + len];
    String::from_utf8_lossy(slice)
        .trim_end_matches(|c: char| c == ' ' || c == '\0')
        .to_string()
}

/// Read a 17-byte "dec-datetime" field from a PVD and produce an ISO 8601 string.
/// Format: YYYYMMDDHHMMSScc±hhmm (digits as ASCII characters).
fn read_dec_datetime(buf: &[u8], offset: usize) -> String {
    let field = &buf[offset..offset + 17];

    // Check if the field is all zeros or spaces (not set)
    if field.iter().all(|&b| b == 0 || b == b'0' || b == b' ') {
        return String::new();
    }

    let year = std::str::from_utf8(&field[0..4]).unwrap_or("????");
    let month = std::str::from_utf8(&field[4..6]).unwrap_or("??");
    let day = std::str::from_utf8(&field[6..8]).unwrap_or("??");
    let hour = std::str::from_utf8(&field[8..10]).unwrap_or("??");
    let minute = std::str::from_utf8(&field[10..12]).unwrap_or("??");
    let second = std::str::from_utf8(&field[12..14]).unwrap_or("??");
    let hundredths = std::str::from_utf8(&field[14..16]).unwrap_or("00");

    // Timezone offset in 15-minute intervals from GMT (signed byte at field[16])
    let tz_offset = field[16] as i8;
    let tz_hours = tz_offset / 4;
    let tz_mins = (tz_offset % 4) * 15;

    format!(
        "{}-{}-{}T{}:{}:{}.{}{}{}:{:02}",
        year,
        month,
        day,
        hour,
        minute,
        second,
        hundredths,
        if tz_hours >= 0 { "+" } else { "" },
        tz_hours,
        tz_mins.unsigned_abs()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Build a minimal ISO 9660 image in memory with PVD and terminator.
    fn build_minimal_iso() -> Vec<u8> {
        // Need at least 18 sectors (16 system area + sector 16 PVD + sector 17 terminator)
        let mut data = vec![0u8; 18 * 2048];

        // Sector 16: Primary Volume Descriptor
        let pvd = &mut data[16 * 2048..17 * 2048];
        pvd[0] = VD_TYPE_PRIMARY;
        pvd[1..6].copy_from_slice(ISO_STANDARD_ID);
        pvd[6] = 1; // version

        // Volume ID at offset 40, 32 bytes
        let vol_id = b"TEST_VOLUME";
        pvd[40..40 + vol_id.len()].copy_from_slice(vol_id);
        // Pad rest with spaces
        for b in &mut pvd[40 + vol_id.len()..72] {
            *b = b' ';
        }

        // System ID at offset 8
        let sys_id = b"LINUX";
        pvd[8..8 + sys_id.len()].copy_from_slice(sys_id);
        for b in &mut pvd[8 + sys_id.len()..40] {
            *b = b' ';
        }

        // Volume Space Size at offset 80 (both-endian)
        let block_count: u32 = 1000;
        pvd[80..84].copy_from_slice(&block_count.to_le_bytes());
        pvd[84..88].copy_from_slice(&block_count.to_be_bytes());

        // Logical Block Size at offset 128 (both-endian)
        let block_size: u16 = 2048;
        pvd[128..130].copy_from_slice(&block_size.to_le_bytes());
        pvd[130..132].copy_from_slice(&block_size.to_be_bytes());

        // Creation date at offset 813 (17 bytes): "2024010112000000\0"
        let date = b"2024010112000000";
        pvd[813..813 + 16].copy_from_slice(date);
        pvd[829] = 0; // UTC

        // Sector 17: Volume Descriptor Set Terminator
        let term = &mut data[17 * 2048..18 * 2048];
        term[0] = VD_TYPE_TERMINATOR;
        term[1..6].copy_from_slice(ISO_STANDARD_ID);
        term[6] = 1;

        data
    }

    #[test]
    fn parse_minimal_iso() {
        let data = build_minimal_iso();
        let mut cursor = Cursor::new(&data);
        let info = parse_iso9660(&mut cursor).unwrap();

        assert_eq!(info.volume_id, "TEST_VOLUME");
        assert_eq!(info.system_id, "LINUX");
        assert_eq!(info.logical_block_size, 2048);
        assert_eq!(info.volume_block_count, 1000);
        assert_eq!(info.volume_size_bytes, 1000 * 2048);
        assert!(!info.is_bootable);
        assert!(!info.has_joliet);
    }

    #[test]
    fn parse_bootable_iso() {
        let data = build_minimal_iso();

        // Insert a boot record at sector 16, shift PVD to 17, terminator to 18
        let mut bigger = vec![0u8; 19 * 2048];
        // System area (sectors 0–15)
        bigger[..16 * 2048].copy_from_slice(&data[..16 * 2048]);

        // Sector 16: El Torito Boot Record
        let boot = &mut bigger[16 * 2048..17 * 2048];
        boot[0] = VD_TYPE_BOOT_RECORD;
        boot[1..6].copy_from_slice(ISO_STANDARD_ID);
        boot[6] = 1;
        boot[7..7 + EL_TORITO_SPEC.len()].copy_from_slice(EL_TORITO_SPEC);
        // Boot catalog at sector 20
        boot[71..75].copy_from_slice(&20u32.to_le_bytes());

        // Sector 17: PVD (copy from sector 16 of original)
        bigger[17 * 2048..18 * 2048].copy_from_slice(&data[16 * 2048..17 * 2048]);

        // Sector 18: Terminator
        let term = &mut bigger[18 * 2048..19 * 2048];
        term[0] = VD_TYPE_TERMINATOR;
        term[1..6].copy_from_slice(ISO_STANDARD_ID);
        term[6] = 1;

        let mut cursor = Cursor::new(&bigger);
        let info = parse_iso9660(&mut cursor).unwrap();

        assert!(info.is_bootable);
        assert_eq!(info.boot_catalog_sector, Some(20));
        assert_eq!(info.volume_id, "TEST_VOLUME");
    }

    #[test]
    fn parse_joliet_iso() {
        let data = build_minimal_iso();

        // Expand to 19 sectors: boot area, PVD(16), Supplementary(17), Terminator(18)
        let mut bigger = vec![0u8; 19 * 2048];
        bigger[..17 * 2048].copy_from_slice(&data[..17 * 2048]);

        // Sector 17: Supplementary Volume Descriptor with Joliet escape
        let svd = &mut bigger[17 * 2048..18 * 2048];
        svd[0] = VD_TYPE_SUPPLEMENTARY;
        svd[1..6].copy_from_slice(ISO_STANDARD_ID);
        svd[6] = 1;
        // Joliet UCS-2 Level 3 escape sequence at offset 88
        svd[88] = 0x25;
        svd[89] = 0x2F;
        svd[90] = 0x45;

        // Sector 18: Terminator
        let term = &mut bigger[18 * 2048..19 * 2048];
        term[0] = VD_TYPE_TERMINATOR;
        term[1..6].copy_from_slice(ISO_STANDARD_ID);
        term[6] = 1;

        let mut cursor = Cursor::new(&bigger);
        let info = parse_iso9660(&mut cursor).unwrap();

        assert!(info.has_joliet);
        assert_eq!(info.volume_id, "TEST_VOLUME");
    }

    #[test]
    fn reject_too_small_file() {
        let data = vec![0u8; 1024]; // Way too small
        let mut cursor = Cursor::new(&data);
        assert!(parse_iso9660(&mut cursor).is_err());
    }

    #[test]
    fn reject_bad_standard_id() {
        let mut data = vec![0u8; 18 * 2048];
        // Sector 16 with wrong standard ID
        data[16 * 2048] = VD_TYPE_PRIMARY;
        data[16 * 2048 + 1..16 * 2048 + 6].copy_from_slice(b"WRONG");

        let mut cursor = Cursor::new(&data);
        assert!(parse_iso9660(&mut cursor).is_err());
    }

    #[test]
    fn dec_datetime_parsing() {
        let mut buf = [0u8; 2048];
        let date = b"2023120315304200";
        buf[813..813 + 16].copy_from_slice(date);
        buf[829] = 8; // +2 hours (8 * 15min = 120min)

        let result = read_dec_datetime(&buf, 813);
        assert!(result.contains("2023"));
        assert!(result.contains("12"));
        assert!(result.contains("03"));
    }

    #[test]
    fn empty_date_returns_empty() {
        let buf = [0u8; 2048];
        let result = read_dec_datetime(&buf, 813);
        assert!(result.is_empty());
    }
}
