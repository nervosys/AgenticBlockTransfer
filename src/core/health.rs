// Drive health — bad block detection, fake flash drive detection, and drive diagnostics.
// Inspired by Rufus's bad blocks checking with configurable patterns.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Read, Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

/// Bad block test pattern.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TestPattern {
    /// Single pattern: 0xAA (1 pass)
    Quick,
    /// Two patterns: 0x55, 0xAA (2 passes)
    Standard,
    /// Four patterns: 0x55, 0xAA, 0xFF, 0x00 — typical for SLC NAND (4 passes)
    Slc,
    /// Six patterns: 0x55, 0xAA, 0x33, 0xCC, 0xFF, 0x00 — typical for MLC NAND (6 passes)
    Mlc,
    /// Eight patterns: 0x55, 0xAA, 0x33, 0xCC, 0x0F, 0xF0, 0xFF, 0x00 — TLC NAND (8 passes)
    Tlc,
    /// Custom pattern byte
    Custom(u8),
}

impl TestPattern {
    /// Get the bytes for each pass of this pattern.
    pub fn patterns(&self) -> Vec<u8> {
        match self {
            Self::Quick => vec![0xAA],
            Self::Standard => vec![0x55, 0xAA],
            Self::Slc => vec![0x55, 0xAA, 0xFF, 0x00],
            Self::Mlc => vec![0x55, 0xAA, 0x33, 0xCC, 0xFF, 0x00],
            Self::Tlc => vec![0x55, 0xAA, 0x33, 0xCC, 0x0F, 0xF0, 0xFF, 0x00],
            Self::Custom(b) => vec![*b],
        }
    }

    /// Number of passes for this pattern.
    pub fn pass_count(&self) -> usize {
        self.patterns().len()
    }
}

impl fmt::Display for TestPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quick => write!(f, "quick (1 pass)"),
            Self::Standard => write!(f, "standard (2 passes)"),
            Self::Slc => write!(f, "SLC (4 passes)"),
            Self::Mlc => write!(f, "MLC (6 passes)"),
            Self::Tlc => write!(f, "TLC (8 passes)"),
            Self::Custom(b) => write!(f, "custom (0x{:02X})", b),
        }
    }
}

/// A detected bad block region.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BadBlock {
    /// Byte offset of the bad block.
    pub offset: u64,
    /// Size of the bad block in bytes.
    pub size: u64,
    /// Type of error detected.
    pub error_type: BadBlockError,
    /// Pattern value that caused the error.
    pub pattern: u8,
    /// Pass number (1-based).
    pub pass: usize,
}

/// Type of bad block error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BadBlockError {
    /// Write failed at this location.
    WriteError,
    /// Read failed at this location.
    ReadError,
    /// Data read back did not match what was written.
    CorruptionError,
}

impl fmt::Display for BadBlockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteError => write!(f, "write error"),
            Self::ReadError => write!(f, "read error"),
            Self::CorruptionError => write!(f, "data corruption"),
        }
    }
}

/// Result of a drive health check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    /// Device path tested.
    pub device: String,
    /// Total size tested in bytes.
    pub total_size: u64,
    /// Block size used for testing.
    pub block_size: u64,
    /// Test pattern used.
    pub pattern_name: String,
    /// Number of passes completed.
    pub passes_completed: usize,
    /// Number of passes total.
    pub passes_total: usize,
    /// List of bad blocks found.
    pub bad_blocks: Vec<BadBlock>,
    /// Total write errors.
    pub write_errors: u64,
    /// Total read errors.
    pub read_errors: u64,
    /// Total corruption errors.
    pub corruption_errors: u64,
    /// Average write speed in bytes/sec.
    pub avg_write_speed: f64,
    /// Average read speed in bytes/sec.
    pub avg_read_speed: f64,
    /// Duration of the test.
    pub duration_secs: f64,
    /// Whether a fake/counterfeit drive was detected.
    pub fake_drive_detected: bool,
    /// Actual usable size (if fake drive detected, this is less than total_size).
    pub actual_usable_size: Option<u64>,
    /// Overall health verdict.
    pub verdict: HealthVerdict,
}

/// Health verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HealthVerdict {
    /// No bad blocks found.
    Healthy,
    /// Some bad blocks found but drive is usable.
    Degraded,
    /// Many bad blocks or fake drive — do not use.
    Failed,
    /// Drive is a fake/counterfeit (reports wrong capacity).
    Counterfeit,
}

impl fmt::Display for HealthVerdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Healthy => write!(f, "HEALTHY"),
            Self::Degraded => write!(f, "DEGRADED"),
            Self::Failed => write!(f, "FAILED"),
            Self::Counterfeit => write!(f, "COUNTERFEIT"),
        }
    }
}

impl HealthReport {
    /// Format the report as human-readable text.
    pub fn format_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Drive Health Report: {}\n", self.device));
        out.push_str(&format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n"));
        out.push_str(&format!(
            "Size tested:    {} ({} bytes)\n",
            humansize::format_size(self.total_size, humansize::BINARY),
            self.total_size
        ));
        out.push_str(&format!("Block size:     {} bytes\n", self.block_size));
        out.push_str(&format!("Pattern:        {}\n", self.pattern_name));
        out.push_str(&format!(
            "Passes:         {}/{}\n",
            self.passes_completed, self.passes_total
        ));
        out.push_str(&format!("Duration:       {:.1}s\n", self.duration_secs));
        out.push_str(&format!(
            "Write speed:    {}/s\n",
            humansize::format_size(self.avg_write_speed as u64, humansize::BINARY)
        ));
        out.push_str(&format!(
            "Read speed:     {}/s\n",
            humansize::format_size(self.avg_read_speed as u64, humansize::BINARY)
        ));
        out.push_str(&format!("\n"));
        out.push_str(&format!("Write errors:       {}\n", self.write_errors));
        out.push_str(&format!("Read errors:        {}\n", self.read_errors));
        out.push_str(&format!("Corruption errors:  {}\n", self.corruption_errors));
        out.push_str(&format!("Bad blocks:         {}\n", self.bad_blocks.len()));
        if self.fake_drive_detected {
            out.push_str(&format!("\n⚠ COUNTERFEIT DRIVE DETECTED!\n"));
            if let Some(actual) = self.actual_usable_size {
                out.push_str(&format!(
                    "  Reported size: {}, actual usable: {}\n",
                    humansize::format_size(self.total_size, humansize::BINARY),
                    humansize::format_size(actual, humansize::BINARY)
                ));
            }
        }
        out.push_str(&format!("\nVerdict: {}\n", self.verdict));
        out
    }

    /// Export as JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// Configuration for a health check.
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Test pattern to use.
    pub pattern: TestPattern,
    /// Block size for I/O operations.
    pub block_size: usize,
    /// Maximum number of bad blocks before aborting.
    pub max_bad_blocks: usize,
    /// Whether to check for fake/counterfeit drives.
    pub detect_fake: bool,
    /// Region to test (None = full device).
    pub test_region: Option<(u64, u64)>,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            pattern: TestPattern::Standard,
            block_size: 128 * 1024, // 128 KiB
            max_bad_blocks: 1000,
            detect_fake: true,
            test_region: None,
        }
    }
}

/// Run a bad blocks check on a file/device. Returns a HealthReport.
///
/// WARNING: This is a destructive test — all data on the device will be lost.
pub fn check_bad_blocks<F>(
    path: &str,
    size: u64,
    config: &HealthCheckConfig,
    progress_cb: F,
) -> Result<HealthReport>
where
    F: Fn(u64, u64, usize, usize), // (bytes_tested, total_bytes, current_pass, total_passes)
{
    let patterns = config.pattern.patterns();
    let total_passes = patterns.len();
    let block_size = config.block_size;

    let (test_start, test_size) = config
        .test_region
        .unwrap_or((0, size));

    let total_blocks = (test_size + block_size as u64 - 1) / block_size as u64;

    let mut bad_blocks = Vec::new();
    let mut write_errors: u64 = 0;
    let mut read_errors: u64 = 0;
    let mut corruption_errors: u64 = 0;
    let mut total_write_bytes: u64 = 0;
    let mut total_read_bytes: u64 = 0;
    let mut write_duration = Duration::ZERO;
    let mut read_duration = Duration::ZERO;

    let start_time = Instant::now();

    let _write_buf = vec![0u8; block_size];
    let mut read_buf = vec![0u8; block_size];

    for (pass_idx, &pattern) in patterns.iter().enumerate() {
        let pass_num = pass_idx + 1;

        // Write pass
        {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .map_err(|e| anyhow!("failed to open {} for writing: {}", path, e))?;
            file.seek(SeekFrom::Start(test_start))?;

            let pattern_buf: Vec<u8> = vec![pattern; block_size];
            let mut offset = test_start;

            for block_idx in 0..total_blocks {
                let remaining = test_size.saturating_sub(block_idx * block_size as u64);
                let this_block = (remaining as usize).min(block_size);

                let t = Instant::now();
                match file.write_all(&pattern_buf[..this_block]) {
                    Ok(()) => {
                        write_duration += t.elapsed();
                        total_write_bytes += this_block as u64;
                    }
                    Err(_e) => {
                        write_errors += 1;
                        bad_blocks.push(BadBlock {
                            offset,
                            size: this_block as u64,
                            error_type: BadBlockError::WriteError,
                            pattern,
                            pass: pass_num,
                        });
                        if bad_blocks.len() >= config.max_bad_blocks {
                            break;
                        }
                    }
                }
                offset += this_block as u64;
                progress_cb(
                    (block_idx + 1) * block_size as u64,
                    test_size * 2, // write + read
                    pass_num,
                    total_passes,
                );
            }
            file.sync_all()?;
        }

        if bad_blocks.len() >= config.max_bad_blocks {
            break;
        }

        // Read-back / verify pass
        {
            let mut file = std::fs::OpenOptions::new()
                .read(true)
                .open(path)
                .map_err(|e| anyhow!("failed to open {} for reading: {}", path, e))?;
            file.seek(SeekFrom::Start(test_start))?;

            let mut offset = test_start;

            for block_idx in 0..total_blocks {
                let remaining = test_size.saturating_sub(block_idx * block_size as u64);
                let this_block = (remaining as usize).min(block_size);

                let t = Instant::now();
                match file.read_exact(&mut read_buf[..this_block]) {
                    Ok(()) => {
                        read_duration += t.elapsed();
                        total_read_bytes += this_block as u64;

                        // Verify pattern
                        if read_buf[..this_block].iter().any(|&b| b != pattern) {
                            corruption_errors += 1;
                            bad_blocks.push(BadBlock {
                                offset,
                                size: this_block as u64,
                                error_type: BadBlockError::CorruptionError,
                                pattern,
                                pass: pass_num,
                            });
                        }
                    }
                    Err(_e) => {
                        read_errors += 1;
                        bad_blocks.push(BadBlock {
                            offset,
                            size: this_block as u64,
                            error_type: BadBlockError::ReadError,
                            pattern,
                            pass: pass_num,
                        });
                    }
                }
                offset += this_block as u64;
                progress_cb(
                    test_size + (block_idx + 1) * block_size as u64,
                    test_size * 2,
                    pass_num,
                    total_passes,
                );

                if bad_blocks.len() >= config.max_bad_blocks {
                    break;
                }
            }
        }

        if bad_blocks.len() >= config.max_bad_blocks {
            break;
        }
    }

    let elapsed = start_time.elapsed();
    let avg_write_speed = if write_duration.as_secs_f64() > 0.0 {
        total_write_bytes as f64 / write_duration.as_secs_f64()
    } else {
        0.0
    };
    let avg_read_speed = if read_duration.as_secs_f64() > 0.0 {
        total_read_bytes as f64 / read_duration.as_secs_f64()
    } else {
        0.0
    };

    // Determine fake drive
    let fake_drive_detected = config.detect_fake && detect_fake_capacity(&bad_blocks, test_size);
    let actual_usable_size = if fake_drive_detected {
        find_actual_capacity(&bad_blocks)
    } else {
        None
    };

    // Determine verdict
    let verdict = if fake_drive_detected {
        HealthVerdict::Counterfeit
    } else if bad_blocks.is_empty() {
        HealthVerdict::Healthy
    } else if bad_blocks.len() < 10 {
        HealthVerdict::Degraded
    } else {
        HealthVerdict::Failed
    };

    let passes_completed = patterns
        .iter()
        .take_while(|_| bad_blocks.len() < config.max_bad_blocks)
        .count()
        .max(1);

    Ok(HealthReport {
        device: path.to_string(),
        total_size: test_size,
        block_size: config.block_size as u64,
        pattern_name: format!("{}", config.pattern),
        passes_completed,
        passes_total: total_passes,
        bad_blocks,
        write_errors,
        read_errors,
        corruption_errors,
        avg_write_speed,
        avg_read_speed,
        duration_secs: elapsed.as_secs_f64(),
        fake_drive_detected,
        actual_usable_size,
        verdict,
    })
}

/// Detect if bad blocks form a pattern consistent with a fake/counterfeit drive.
/// Fake drives typically work fine up to their real capacity, then fail consistently beyond.
fn detect_fake_capacity(bad_blocks: &[BadBlock], total_size: u64) -> bool {
    if bad_blocks.len() < 3 {
        return false;
    }

    // Check if all errors are corruption and start at roughly the same offset
    let corruption_blocks: Vec<_> = bad_blocks
        .iter()
        .filter(|b| b.error_type == BadBlockError::CorruptionError)
        .collect();

    if corruption_blocks.len() < 3 {
        return false;
    }

    // Find the lowest offset with corruption  
    let min_offset = corruption_blocks.iter().map(|b| b.offset).min().unwrap_or(total_size);
    
    // If corruption starts before 90% of total and all blocks beyond that point are bad,
    // it's likely a fake drive
    let threshold = total_size * 9 / 10;
    if min_offset < threshold {
        // Check density of errors beyond min_offset
        let errors_beyond = corruption_blocks
            .iter()
            .filter(|b| b.offset >= min_offset)
            .count();
        let expected_blocks = ((total_size - min_offset) / bad_blocks[0].size) as usize;
        if expected_blocks > 0 && errors_beyond * 2 > expected_blocks {
            return true;
        }
    }

    false
}

/// Find the actual usable capacity based on where corruption starts.
fn find_actual_capacity(bad_blocks: &[BadBlock]) -> Option<u64> {
    bad_blocks
        .iter()
        .filter(|b| b.error_type == BadBlockError::CorruptionError)
        .map(|b| b.offset)
        .min()
}

/// Quick read-only health check — just test if the device is readable.
pub fn quick_read_check(path: &str, size: u64, block_size: usize) -> Result<(u64, u64, f64)> {
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; block_size];
    let mut bytes_read: u64 = 0;
    let mut errors: u64 = 0;
    let start = Instant::now();

    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => bytes_read += n as u64,
            Err(_) => {
                errors += 1;
                // Try to seek past the bad region
                let pos = file.seek(SeekFrom::Current(block_size as i64)).unwrap_or(size);
                if pos >= size {
                    break;
                }
            }
        }
        if bytes_read >= size {
            break;
        }
    }

    let speed = if start.elapsed().as_secs_f64() > 0.0 {
        bytes_read as f64 / start.elapsed().as_secs_f64()
    } else {
        0.0
    };

    Ok((bytes_read, errors, speed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_quick() {
        let p = TestPattern::Quick;
        assert_eq!(p.patterns(), vec![0xAA]);
        assert_eq!(p.pass_count(), 1);
    }

    #[test]
    fn test_pattern_standard() {
        let p = TestPattern::Standard;
        assert_eq!(p.patterns(), vec![0x55, 0xAA]);
        assert_eq!(p.pass_count(), 2);
    }

    #[test]
    fn test_pattern_slc() {
        let p = TestPattern::Slc;
        assert_eq!(p.pass_count(), 4);
    }

    #[test]
    fn test_pattern_mlc() {
        let p = TestPattern::Mlc;
        assert_eq!(p.pass_count(), 6);
    }

    #[test]
    fn test_pattern_tlc() {
        let p = TestPattern::Tlc;
        assert_eq!(p.pass_count(), 8);
    }

    #[test]
    fn test_pattern_custom() {
        let p = TestPattern::Custom(0x42);
        assert_eq!(p.patterns(), vec![0x42]);
    }

    #[test]
    fn test_pattern_display() {
        assert_eq!(format!("{}", TestPattern::Quick), "quick (1 pass)");
        assert_eq!(format!("{}", TestPattern::Standard), "standard (2 passes)");
        assert_eq!(format!("{}", TestPattern::Custom(0xFF)), "custom (0xFF)");
    }

    #[test]
    fn test_bad_block_error_display() {
        assert_eq!(format!("{}", BadBlockError::WriteError), "write error");
        assert_eq!(format!("{}", BadBlockError::ReadError), "read error");
        assert_eq!(format!("{}", BadBlockError::CorruptionError), "data corruption");
    }

    #[test]
    fn test_health_verdict_display() {
        assert_eq!(format!("{}", HealthVerdict::Healthy), "HEALTHY");
        assert_eq!(format!("{}", HealthVerdict::Degraded), "DEGRADED");
        assert_eq!(format!("{}", HealthVerdict::Failed), "FAILED");
        assert_eq!(format!("{}", HealthVerdict::Counterfeit), "COUNTERFEIT");
    }

    #[test]
    fn test_health_check_config_default() {
        let cfg = HealthCheckConfig::default();
        assert_eq!(cfg.pattern, TestPattern::Standard);
        assert_eq!(cfg.block_size, 128 * 1024);
        assert!(cfg.detect_fake);
    }

    #[test]
    fn test_check_bad_blocks_on_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let size: u64 = 64 * 1024; // 64 KiB test
        std::fs::write(tmp.path(), vec![0u8; size as usize]).unwrap();

        let config = HealthCheckConfig {
            pattern: TestPattern::Quick,
            block_size: 4096,
            max_bad_blocks: 100,
            detect_fake: false,
            test_region: None,
        };

        let report = check_bad_blocks(
            tmp.path().to_str().unwrap(),
            size,
            &config,
            |_, _, _, _| {},
        )
        .unwrap();

        assert_eq!(report.verdict, HealthVerdict::Healthy);
        assert!(report.bad_blocks.is_empty());
        assert_eq!(report.write_errors, 0);
        assert_eq!(report.read_errors, 0);
        assert_eq!(report.corruption_errors, 0);
    }

    #[test]
    fn test_health_report_text() {
        let report = HealthReport {
            device: "/dev/sdb".into(),
            total_size: 1024 * 1024,
            block_size: 4096,
            pattern_name: "quick (1 pass)".into(),
            passes_completed: 1,
            passes_total: 1,
            bad_blocks: vec![],
            write_errors: 0,
            read_errors: 0,
            corruption_errors: 0,
            avg_write_speed: 100_000_000.0,
            avg_read_speed: 200_000_000.0,
            duration_secs: 0.5,
            fake_drive_detected: false,
            actual_usable_size: None,
            verdict: HealthVerdict::Healthy,
        };
        let text = report.format_text();
        assert!(text.contains("/dev/sdb"));
        assert!(text.contains("HEALTHY"));
        assert!(text.contains("quick (1 pass)"));
    }

    #[test]
    fn test_health_report_json() {
        let report = HealthReport {
            device: "test".into(),
            total_size: 1024,
            block_size: 512,
            pattern_name: "quick".into(),
            passes_completed: 1,
            passes_total: 1,
            bad_blocks: vec![],
            write_errors: 0,
            read_errors: 0,
            corruption_errors: 0,
            avg_write_speed: 0.0,
            avg_read_speed: 0.0,
            duration_secs: 0.1,
            fake_drive_detected: false,
            actual_usable_size: None,
            verdict: HealthVerdict::Healthy,
        };
        let json = report.to_json().unwrap();
        assert!(json.contains("\"verdict\": \"Healthy\""));
    }

    #[test]
    fn test_detect_fake_no_errors() {
        assert!(!detect_fake_capacity(&[], 1024 * 1024));
    }

    #[test]
    fn test_detect_fake_few_errors() {
        let blocks = vec![BadBlock {
            offset: 1000,
            size: 512,
            error_type: BadBlockError::CorruptionError,
            pattern: 0xAA,
            pass: 1,
        }];
        assert!(!detect_fake_capacity(&blocks, 1024 * 1024));
    }

    #[test]
    fn test_quick_read_check() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), vec![0xABu8; 8192]).unwrap();
        let (bytes, errors, speed) =
            quick_read_check(tmp.path().to_str().unwrap(), 8192, 4096).unwrap();
        assert_eq!(bytes, 8192);
        assert_eq!(errors, 0);
        assert!(speed > 0.0);
    }
}
