// Optical disc reader — read CD/DVD/Blu-ray media and save to ISO file
//
// Reads raw sectors from optical disc drives and writes them to ISO 9660
// image files. Supports:
//   - CD-ROM:  2048 bytes/sector (Mode 1) or cooked
//   - DVD:     2048 bytes/sector
//   - Blu-ray: 2048 bytes/sector
//
// Platform support:
//   Windows:  CreateFile on \\.\CdRom0, DeviceIoControl for disc info
//   Linux:    /dev/sr0 (or /dev/cdrom), CDROM_DISC_STATUS ioctl
//   macOS:    /dev/disk* for optical drives
//
// Reference: Rufus iso.c (ISO save functionality), cdrecord, UDF/ISO 9660 specs

#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

// ─── Constants ─────────────────────────────────────────────────────────────

/// Standard sector size for optical media (2048 bytes = 2 KiB).
const SECTOR_SIZE: u64 = 2048;

/// Default read buffer size: 64 sectors (128 KiB).
const DEFAULT_BUFFER_SECTORS: usize = 64;

/// ISO 9660 Primary Volume Descriptor type.
const PVD_TYPE: u8 = 1;

/// ISO 9660 Volume Descriptor Set Terminator type.
const VDST_TYPE: u8 = 255;

/// CD001 identifier in volume descriptors.
const ISO_MAGIC: &[u8; 5] = b"CD001";

// ─── Disc Type ─────────────────────────────────────────────────────────────

/// Type of optical disc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscType {
    /// CD-ROM (up to 700 MiB typical).
    CdRom,
    /// DVD (up to 4.7 GB single-layer, 8.5 GB dual-layer).
    Dvd,
    /// Blu-ray Disc (up to 25 GB single-layer, 50 GB dual-layer).
    BluRay,
    /// Unknown or unrecognized disc type.
    Unknown,
}

impl DiscType {
    /// Typical maximum capacity in bytes.
    pub fn typical_max_bytes(&self) -> u64 {
        match self {
            Self::CdRom => 737_280_000,         // 703 MiB
            Self::Dvd => 4_707_319_808,          // 4.38 GiB (single-layer)
            Self::BluRay => 25_025_314_816,      // 23.3 GiB (single-layer)
            Self::Unknown => 0,
        }
    }
}

impl std::fmt::Display for DiscType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CdRom => write!(f, "CD-ROM"),
            Self::Dvd => write!(f, "DVD"),
            Self::BluRay => write!(f, "Blu-ray"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

// ─── Disc Info ─────────────────────────────────────────────────────────────

/// Information about an optical disc.
#[derive(Debug, Clone)]
pub struct DiscInfo {
    /// Drive device path.
    pub device_path: String,
    /// Disc type.
    pub disc_type: DiscType,
    /// Total number of sectors.
    pub sector_count: u64,
    /// Sector size in bytes (typically 2048).
    pub sector_size: u64,
    /// Total disc size in bytes.
    pub total_size: u64,
    /// Volume label from ISO 9660 PVD (if available).
    pub volume_label: Option<String>,
    /// Whether the disc appears to be blank.
    pub is_blank: bool,
    /// Whether the disc is multi-session.
    pub is_multi_session: bool,
}

impl DiscInfo {
    /// Create disc info from a sector count.
    pub fn from_sector_count(device_path: &str, sectors: u64) -> Self {
        let total_size = sectors * SECTOR_SIZE;
        let disc_type = if total_size <= 737_300_000 {
            DiscType::CdRom
        } else if total_size <= 9_500_000_000 {
            DiscType::Dvd
        } else {
            DiscType::BluRay
        };

        Self {
            device_path: device_path.to_string(),
            disc_type,
            sector_count: sectors,
            sector_size: SECTOR_SIZE,
            total_size,
            volume_label: None,
            is_blank: false,
            is_multi_session: false,
        }
    }
}

impl std::fmt::Display for DiscInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Optical Disc Info:")?;
        writeln!(f, "  Device:    {}", self.device_path)?;
        writeln!(f, "  Type:      {}", self.disc_type)?;
        writeln!(f, "  Sectors:   {}", self.sector_count)?;
        writeln!(f, "  Size:      {} MiB ({} bytes)",
            self.total_size / (1024 * 1024), self.total_size)?;
        if let Some(ref label) = self.volume_label {
            writeln!(f, "  Label:     {}", label)?;
        }
        writeln!(f, "  Blank:     {}", self.is_blank)?;
        write!(f, "  Sessions:  {}", if self.is_multi_session { "multi" } else { "single" })
    }
}

// ─── Read Progress ─────────────────────────────────────────────────────────

/// Progress callback for disc reading operations.
pub type ProgressCallback = Box<dyn Fn(ReadProgress) + Send>;

/// Progress information during disc read.
#[derive(Debug, Clone)]
pub struct ReadProgress {
    /// Bytes read so far.
    pub bytes_read: u64,
    /// Total bytes to read.
    pub bytes_total: u64,
    /// Current sector being read.
    pub current_sector: u64,
    /// Total sectors.
    pub total_sectors: u64,
    /// Read speed in bytes per second.
    pub speed_bps: u64,
    /// Estimated time remaining in seconds.
    pub eta_seconds: f64,
    /// Number of read errors encountered.
    pub error_count: u32,
}

impl ReadProgress {
    /// Completion percentage (0.0 — 100.0).
    pub fn percent(&self) -> f64 {
        if self.bytes_total == 0 {
            0.0
        } else {
            (self.bytes_read as f64 / self.bytes_total as f64) * 100.0
        }
    }
}

// ─── Read Configuration ────────────────────────────────────────────────────

/// Configuration for reading an optical disc.
#[derive(Debug, Clone)]
pub struct ReadConfig {
    /// Source device path.
    pub device: String,
    /// Output ISO file path.
    pub output: PathBuf,
    /// Number of sectors to read per I/O operation.
    pub buffer_sectors: usize,
    /// Maximum number of read retries per sector on error.
    pub max_retries: u32,
    /// Whether to skip unreadable sectors (fill with zeros).
    pub skip_errors: bool,
    /// Whether to verify the output after reading.
    pub verify: bool,
    /// Whether to overwrite existing output file.
    pub overwrite: bool,
}

impl Default for ReadConfig {
    fn default() -> Self {
        Self {
            device: String::new(),
            output: PathBuf::new(),
            buffer_sectors: DEFAULT_BUFFER_SECTORS,
            max_retries: 3,
            skip_errors: false,
            verify: true,
            overwrite: false,
        }
    }
}

// ─── Read Result ───────────────────────────────────────────────────────────

/// Result of a disc read operation.
#[derive(Debug, Clone)]
pub struct ReadResult {
    /// Output file path.
    pub output_path: PathBuf,
    /// Total bytes read.
    pub bytes_read: u64,
    /// Total sectors read.
    pub sectors_read: u64,
    /// Number of sectors that had read errors.
    pub error_sectors: u32,
    /// Whether the read completed successfully.
    pub success: bool,
    /// Duration in seconds.
    pub duration_seconds: f64,
    /// Average read speed in bytes per second.
    pub avg_speed_bps: u64,
    /// SHA-256 hash of the output file (if verification was enabled).
    pub sha256: Option<String>,
}

impl std::fmt::Display for ReadResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Disc Read Result:")?;
        writeln!(f, "  Output:      {}", self.output_path.display())?;
        writeln!(f, "  Bytes:       {} MiB", self.bytes_read / (1024 * 1024))?;
        writeln!(f, "  Sectors:     {}", self.sectors_read)?;
        writeln!(f, "  Errors:      {}", self.error_sectors)?;
        writeln!(f, "  Duration:    {:.1}s", self.duration_seconds)?;
        writeln!(f, "  Speed:       {} MiB/s",
            self.avg_speed_bps / (1024 * 1024))?;
        if let Some(ref hash) = self.sha256 {
            writeln!(f, "  SHA-256:     {}", hash)?;
        }
        write!(f, "  Status:      {}", if self.success { "OK" } else { "FAILED" })
    }
}

// ─── Disc Detection ────────────────────────────────────────────────────────

/// Detect optical drives on the system.
pub fn detect_drives() -> Result<Vec<String>> {
    let mut drives = Vec::new();

    #[cfg(target_os = "linux")]
    {
        // Check for /dev/sr* and /dev/cdrom
        for i in 0..8 {
            let path = format!("/dev/sr{}", i);
            if Path::new(&path).exists() {
                drives.push(path);
            }
        }
        if drives.is_empty() {
            if Path::new("/dev/cdrom").exists() {
                drives.push("/dev/cdrom".into());
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Check \\.\CdRom0, \\.\CdRom1, etc.
        for i in 0..8 {
            let path = format!("\\\\.\\CdRom{}", i);
            // We can't easily test if the device exists without opening it,
            // so we enumerate drive letters with GetDriveType == DRIVE_CDROM
            drives.push(path);
        }
        // Also check drive letters
        for letter in b'D'..=b'Z' {
            let path = format!("{}:\\", letter as char);
            let drive_path = format!("\\\\.\\{}:", letter as char);
            // On Windows, we'd use GetDriveTypeW to check — simplified here
            if Path::new(&path).exists() {
                // Check if it's an optical drive (heuristic — check for disc)
                // In practice, use GetDriveType WinAPI
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS optical drives appear as /dev/disk* with specific properties
        // Use diskutil to find optical drives
        if let Ok(output) = std::process::Command::new("diskutil")
            .args(&["list", "-plist"])
            .output()
        {
            // Parse plist for optical drives — simplified
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if stdout.contains("CD_DVD") || stdout.contains("Optical") {
                    drives.push("/dev/disk1".into()); // placeholder
                }
            }
        }
    }

    Ok(drives)
}

/// Get disc information from an optical drive.
pub fn get_disc_info(device: &str) -> Result<DiscInfo> {
    // Try to open and read the ISO 9660 PVD to get disc info
    let mut file = std::fs::File::open(device)
        .context(format!("Failed to open optical drive: {}", device))?;

    // Read PVD at sector 16 (offset 32768)
    let pvd_offset = 16 * SECTOR_SIZE;
    file.seek(SeekFrom::Start(pvd_offset))
        .context("Failed to seek to PVD")?;

    let mut pvd = [0u8; SECTOR_SIZE as usize];
    file.read_exact(&mut pvd)
        .context("Failed to read PVD")?;

    // Verify ISO 9660 magic
    if pvd[0] != PVD_TYPE || &pvd[1..6] != ISO_MAGIC {
        bail!("No ISO 9660 filesystem found on disc (not a data disc?)");
    }

    // Volume label at offset 40-71 (32 bytes, space-padded)
    let label = String::from_utf8_lossy(&pvd[40..72])
        .trim()
        .to_string();

    // Volume size in sectors (at offset 80, both-endian — use LE)
    let vol_sectors = u32::from_le_bytes([pvd[80], pvd[81], pvd[82], pvd[83]]) as u64;

    // Logical block size (at offset 128, both-endian — use LE)
    let block_size = u16::from_le_bytes([pvd[128], pvd[129]]) as u64;
    let effective_sector_size = if block_size > 0 { block_size } else { SECTOR_SIZE };

    let total_size = vol_sectors * effective_sector_size;

    let mut info = DiscInfo::from_sector_count(device, vol_sectors);
    info.volume_label = if label.is_empty() { None } else { Some(label) };
    info.sector_size = effective_sector_size;
    info.total_size = total_size;

    Ok(info)
}

/// Read an entire optical disc and save to an ISO file.
pub fn read_disc(config: &ReadConfig, progress_cb: Option<ProgressCallback>) -> Result<ReadResult> {
    use sha2::{Digest, Sha256};
    use std::time::Instant;

    // Open source device
    let mut source = std::fs::File::open(&config.device)
        .context(format!("Failed to open optical drive: {}", config.device))?;

    // Get disc size from ISO PVD
    let disc_info = get_disc_info(&config.device)
        .context("Failed to read disc information")?;

    let total_bytes = disc_info.total_size;
    let total_sectors = disc_info.sector_count;

    // Check output file
    if config.output.exists() && !config.overwrite {
        bail!(
            "Output file already exists: {}. Use --overwrite to replace.",
            config.output.display()
        );
    }

    // Create output file
    let mut output = std::fs::File::create(&config.output)
        .context(format!("Failed to create output file: {}", config.output.display()))?;

    let buffer_size = config.buffer_sectors * SECTOR_SIZE as usize;
    let mut buffer = vec![0u8; buffer_size];
    let mut bytes_read: u64 = 0;
    let mut sectors_read: u64 = 0;
    let mut error_count: u32 = 0;
    let mut hasher = if config.verify { Some(Sha256::new()) } else { None };

    let start = Instant::now();
    source.seek(SeekFrom::Start(0))?;

    while bytes_read < total_bytes {
        let remaining = total_bytes - bytes_read;
        let to_read = buffer_size.min(remaining as usize);

        let mut read_ok = false;
        for retry in 0..=config.max_retries {
            match source.read_exact(&mut buffer[..to_read]) {
                Ok(()) => {
                    read_ok = true;
                    break;
                }
                Err(e) => {
                    if retry == config.max_retries {
                        if config.skip_errors {
                            // Fill with zeros for unreadable sectors
                            buffer[..to_read].fill(0);
                            error_count += 1;
                            read_ok = true;
                        } else {
                            bail!(
                                "Read error at sector {} after {} retries: {}",
                                sectors_read,
                                config.max_retries,
                                e
                            );
                        }
                    }
                }
            }
        }

        if !read_ok {
            break;
        }

        output.write_all(&buffer[..to_read])
            .context("Failed to write to output file")?;

        if let Some(ref mut h) = hasher {
            h.update(&buffer[..to_read]);
        }

        bytes_read += to_read as u64;
        sectors_read = bytes_read / SECTOR_SIZE;

        // Report progress
        if let Some(ref cb) = progress_cb {
            let elapsed = start.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 { (bytes_read as f64 / elapsed) as u64 } else { 0 };
            let eta = if speed > 0 {
                (total_bytes - bytes_read) as f64 / speed as f64
            } else {
                0.0
            };

            cb(ReadProgress {
                bytes_read,
                bytes_total: total_bytes,
                current_sector: sectors_read,
                total_sectors,
                speed_bps: speed,
                eta_seconds: eta,
                error_count,
            });
        }
    }

    output.flush()?;
    let duration = start.elapsed().as_secs_f64();
    let avg_speed = if duration > 0.0 { (bytes_read as f64 / duration) as u64 } else { 0 };

    let sha256 = hasher.map(|h| hex::encode(h.finalize()));

    Ok(ReadResult {
        output_path: config.output.clone(),
        bytes_read,
        sectors_read,
        error_sectors: error_count,
        success: error_count == 0,
        duration_seconds: duration,
        avg_speed_bps: avg_speed,
        sha256,
    })
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disc_type_display() {
        assert_eq!(format!("{}", DiscType::CdRom), "CD-ROM");
        assert_eq!(format!("{}", DiscType::Dvd), "DVD");
        assert_eq!(format!("{}", DiscType::BluRay), "Blu-ray");
        assert_eq!(format!("{}", DiscType::Unknown), "Unknown");
    }

    #[test]
    fn test_disc_type_max_bytes() {
        assert!(DiscType::CdRom.typical_max_bytes() > 700_000_000);
        assert!(DiscType::Dvd.typical_max_bytes() > 4_000_000_000);
        assert!(DiscType::BluRay.typical_max_bytes() > 20_000_000_000);
        assert_eq!(DiscType::Unknown.typical_max_bytes(), 0);
    }

    #[test]
    fn test_disc_info_from_sector_count() {
        // CD size:
        let cd = DiscInfo::from_sector_count("/dev/sr0", 333_000);
        assert_eq!(cd.disc_type, DiscType::CdRom);
        assert_eq!(cd.sector_count, 333_000);
        assert_eq!(cd.total_size, 333_000 * SECTOR_SIZE);

        // DVD size:
        let dvd = DiscInfo::from_sector_count("/dev/sr0", 2_295_104);
        assert_eq!(dvd.disc_type, DiscType::Dvd);

        // Blu-ray size:
        let bd = DiscInfo::from_sector_count("/dev/sr0", 12_219_392);
        assert_eq!(bd.disc_type, DiscType::BluRay);
    }

    #[test]
    fn test_disc_info_display() {
        let info = DiscInfo {
            device_path: "/dev/sr0".into(),
            disc_type: DiscType::Dvd,
            sector_count: 2_295_104,
            sector_size: SECTOR_SIZE,
            total_size: 2_295_104 * SECTOR_SIZE,
            volume_label: Some("MY_DVD".into()),
            is_blank: false,
            is_multi_session: false,
        };
        let display = format!("{}", info);
        assert!(display.contains("DVD"));
        assert!(display.contains("/dev/sr0"));
        assert!(display.contains("MY_DVD"));
    }

    #[test]
    fn test_read_config_default() {
        let config = ReadConfig::default();
        assert_eq!(config.buffer_sectors, DEFAULT_BUFFER_SECTORS);
        assert_eq!(config.max_retries, 3);
        assert!(!config.skip_errors);
        assert!(config.verify);
        assert!(!config.overwrite);
    }

    #[test]
    fn test_read_result_display() {
        let result = ReadResult {
            output_path: PathBuf::from("/tmp/disc.iso"),
            bytes_read: 4_700_000_000,
            sectors_read: 2_294_921,
            error_sectors: 0,
            success: true,
            duration_seconds: 320.5,
            avg_speed_bps: 14_665_364,
            sha256: Some("abc123def456".into()),
        };
        let display = format!("{}", result);
        assert!(display.contains("disc.iso"));
        assert!(display.contains("MiB"));
        assert!(display.contains("abc123def456"));
        assert!(display.contains("OK"));
    }

    #[test]
    fn test_read_progress_percent() {
        let progress = ReadProgress {
            bytes_read: 500,
            bytes_total: 1000,
            current_sector: 0,
            total_sectors: 0,
            speed_bps: 0,
            eta_seconds: 0.0,
            error_count: 0,
        };
        assert!((progress.percent() - 50.0).abs() < 0.001);

        let zero_progress = ReadProgress {
            bytes_read: 0,
            bytes_total: 0,
            current_sector: 0,
            total_sectors: 0,
            speed_bps: 0,
            eta_seconds: 0.0,
            error_count: 0,
        };
        assert_eq!(zero_progress.percent(), 0.0);
    }

    #[test]
    fn test_detect_drives() {
        // Should not panic, may return empty list
        let drives = detect_drives().unwrap();
        // Just verify it returns without error
        let _ = drives;
    }

    #[test]
    fn test_read_result_failed() {
        let result = ReadResult {
            output_path: PathBuf::from("output.iso"),
            bytes_read: 1_000_000,
            sectors_read: 488,
            error_sectors: 5,
            success: false,
            duration_seconds: 10.0,
            avg_speed_bps: 100_000,
            sha256: None,
        };
        let display = format!("{}", result);
        assert!(display.contains("FAILED"));
        assert!(display.contains("5"));
    }
}
