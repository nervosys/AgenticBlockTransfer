//! Differential / incremental write — only write blocks that differ between source and target.
//!
//! Useful for re-flashing a device when most of the image is unchanged (e.g.,
//! updating a Raspberry Pi OS image in-place). Dramatically reduces wear on
//! flash media and speeds up repeat writes.
//!
//! Algorithm:
//! 1. Read source block and target block in parallel
//! 2. Compare (memcmp-fast)
//! 3. If identical, skip (seek past)
//! 4. If different, write the source block
//! 5. Track statistics (blocks skipped vs written)

use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::time::Instant;

use anyhow::{Context, Result};
use log::{debug, info};
use serde::{Deserialize, Serialize};

use super::progress::{OperationPhase, Progress};

/// Configuration for differential write.
#[derive(Debug, Clone)]
pub struct DiffWriteConfig {
    /// Path to the source image file.
    pub source: String,
    /// Path to the target device/file.
    pub target: String,
    /// Block size for comparison (should match device sector size or be a multiple).
    pub block_size: usize,
    /// Verify after writing (re-read changed blocks and compare).
    pub verify: bool,
    /// Only report what would change — don't actually write.
    pub dry_run: bool,
}

impl Default for DiffWriteConfig {
    fn default() -> Self {
        Self {
            source: String::new(),
            target: String::new(),
            block_size: 4 * 1024 * 1024, // 4 MiB
            verify: true,
            dry_run: false,
        }
    }
}

/// Result of a differential write operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffWriteResult {
    /// Total blocks examined.
    pub total_blocks: u64,
    /// Blocks that were identical (skipped).
    pub skipped_blocks: u64,
    /// Blocks that differed (written).
    pub written_blocks: u64,
    /// Total bytes written (only changed blocks).
    pub bytes_written: u64,
    /// Total bytes examined.
    pub bytes_examined: u64,
    /// Duration in milliseconds.
    pub duration_ms: f64,
    /// Effective throughput in MiB/s (counting only changed data).
    pub effective_throughput_mib_s: f64,
    /// Percentage of data that was skipped.
    pub skip_percentage: f64,
    /// Verification passed (if verification was requested).
    pub verified: Option<bool>,
}

/// Perform a differential write: only write blocks that differ between source and target.
pub fn diff_write(config: &DiffWriteConfig, progress: &Progress) -> Result<DiffWriteResult> {
    let start = Instant::now();

    let source_file = std::fs::File::open(&config.source)
        .with_context(|| format!("Cannot open source: {}", config.source))?;
    let source_size = source_file.metadata()?.len();

    let target_file = std::fs::OpenOptions::new()
        .read(true)
        .write(!config.dry_run)
        .open(&config.target)
        .with_context(|| format!("Cannot open target: {}", config.target))?;

    progress.set_total(source_size);
    progress.set_phase(OperationPhase::Writing);

    let mut source_reader = BufReader::with_capacity(config.block_size, source_file);

    let block_size = config.block_size;
    let mut source_buf = vec![0u8; block_size];
    let mut target_buf = vec![0u8; block_size];

    let mut total_blocks: u64 = 0;
    let mut skipped_blocks: u64 = 0;
    let mut written_blocks: u64 = 0;
    let mut bytes_written: u64 = 0;
    let mut offset: u64 = 0;

    // We need separate reader and writer handles for the target.
    // On POSIX, opening the same file twice works. On Windows, we need
    // read+write on the same handle if it's a device.
    let mut target_reader = BufReader::with_capacity(block_size, target_file.try_clone()?);
    let mut target_writer = if config.dry_run {
        None
    } else {
        Some(BufWriter::with_capacity(block_size, target_file))
    };

    info!(
        "Differential write: {} → {} (block_size={}, dry_run={})",
        config.source, config.target, block_size, config.dry_run
    );

    loop {
        if progress.is_cancelled() {
            info!("Differential write cancelled at offset {}", offset);
            break;
        }

        // Read source block
        let source_n = read_full_block(&mut source_reader, &mut source_buf)?;
        if source_n == 0 {
            break; // End of source
        }

        // Read target block at same offset
        target_reader.seek(SeekFrom::Start(offset))?;
        let target_n = read_full_block(&mut target_reader, &mut target_buf)?;

        total_blocks += 1;

        // Compare
        let blocks_match = source_n == target_n
            && source_buf[..source_n] == target_buf[..target_n];

        if blocks_match {
            skipped_blocks += 1;
            debug!("Block {} at offset {} — identical, skipping", total_blocks, offset);
        } else {
            written_blocks += 1;
            bytes_written += source_n as u64;

            if let Some(ref mut writer) = target_writer {
                writer.seek(SeekFrom::Start(offset))?;
                writer.write_all(&source_buf[..source_n])?;
            }

            debug!(
                "Block {} at offset {} — differs, {}",
                total_blocks,
                offset,
                if config.dry_run { "would write" } else { "writing" }
            );
        }

        offset += source_n as u64;
        progress.set_bytes(offset);
    }

    // Flush and sync
    if let Some(ref mut writer) = target_writer {
        writer.flush()?;
        writer.get_ref().sync_all()?;
    }

    let duration = start.elapsed();
    let secs = duration.as_secs_f64();
    let effective_throughput = if secs > 0.0 && bytes_written > 0 {
        (bytes_written as f64) / (1024.0 * 1024.0) / secs
    } else {
        0.0
    };
    let skip_pct = if total_blocks > 0 {
        (skipped_blocks as f64 / total_blocks as f64) * 100.0
    } else {
        0.0
    };

    // Optional verification of written blocks
    let verified = if config.verify && !config.dry_run && written_blocks > 0 {
        info!("Verifying {} changed blocks...", written_blocks);
        progress.set_phase(OperationPhase::Verifying);
        Some(verify_diff_write(config, progress)?)
    } else {
        None
    };

    let result = DiffWriteResult {
        total_blocks,
        skipped_blocks,
        written_blocks,
        bytes_written,
        bytes_examined: offset,
        duration_ms: duration.as_secs_f64() * 1000.0,
        effective_throughput_mib_s: effective_throughput,
        skip_percentage: skip_pct,
        verified,
    };

    info!(
        "Differential write complete: {} total blocks, {} written, {} skipped ({:.1}%)",
        result.total_blocks, result.written_blocks, result.skipped_blocks, result.skip_percentage
    );

    Ok(result)
}

/// Read a full block from a reader, handling partial reads.
fn read_full_block(reader: &mut impl Read, buf: &mut [u8]) -> Result<usize> {
    let mut total = 0;
    while total < buf.len() {
        match reader.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(total)
}

/// Verify a differential write by re-reading source and target and comparing block-by-block.
fn verify_diff_write(config: &DiffWriteConfig, progress: &Progress) -> Result<bool> {
    let source_file = std::fs::File::open(&config.source)?;
    let target_file = std::fs::File::open(&config.target)?;

    let source_size = source_file.metadata()?.len();
    progress.set_total(source_size);
    progress.reset_bytes();

    let mut source_reader = BufReader::with_capacity(config.block_size, source_file);
    let mut target_reader = BufReader::with_capacity(config.block_size, target_file);

    let mut source_buf = vec![0u8; config.block_size];
    let mut target_buf = vec![0u8; config.block_size];
    let mut offset: u64 = 0;

    loop {
        if progress.is_cancelled() {
            return Ok(false);
        }

        let sn = read_full_block(&mut source_reader, &mut source_buf)?;
        let tn = read_full_block(&mut target_reader, &mut target_buf)?;

        if sn == 0 && tn == 0 {
            break;
        }

        if sn != tn || source_buf[..sn] != target_buf[..tn] {
            info!(
                "Verification mismatch at offset {} (source={} bytes, target={} bytes)",
                offset, sn, tn
            );
            return Ok(false);
        }

        offset += sn as u64;
        progress.set_bytes(offset);
    }

    info!("Differential write verification passed at {} bytes", offset);
    Ok(true)
}

/// Format a DiffWriteResult for terminal display.
pub fn format_diff_result(result: &DiffWriteResult) -> String {
    let mut out = String::new();
    out.push_str("Differential Write Results\n");
    out.push_str(&"═".repeat(50));
    out.push('\n');
    out.push_str(&format!("Total blocks examined:  {}\n", result.total_blocks));
    out.push_str(&format!("Blocks written:        {}\n", result.written_blocks));
    out.push_str(&format!("Blocks skipped:        {}\n", result.skipped_blocks));
    out.push_str(&format!("Data written:          {:.1} MiB\n",
        result.bytes_written as f64 / (1024.0 * 1024.0)));
    out.push_str(&format!("Data examined:         {:.1} MiB\n",
        result.bytes_examined as f64 / (1024.0 * 1024.0)));
    out.push_str(&format!("Skip percentage:       {:.1}%\n", result.skip_percentage));
    out.push_str(&format!("Duration:              {:.1} ms\n", result.duration_ms));
    out.push_str(&format!("Effective throughput:   {:.1} MiB/s\n",
        result.effective_throughput_mib_s));

    if let Some(verified) = result.verified {
        out.push_str(&format!("Verification:          {}\n",
            if verified { "PASSED" } else { "FAILED" }));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    fn create_test_file(data: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(data).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn test_diff_write_config_default() {
        let config = DiffWriteConfig::default();
        assert_eq!(config.block_size, 4 * 1024 * 1024);
        assert!(config.verify);
        assert!(!config.dry_run);
    }

    #[test]
    fn test_diff_write_identical_files() {
        let data = vec![0xABu8; 8192];
        let source = create_test_file(&data);
        let target = create_test_file(&data);

        let config = DiffWriteConfig {
            source: source.path().to_str().unwrap().to_string(),
            target: target.path().to_str().unwrap().to_string(),
            block_size: 4096,
            verify: false,
            dry_run: false,
        };

        let progress = Progress::new(0);
        let result = diff_write(&config, &progress).unwrap();

        assert_eq!(result.total_blocks, 2);
        assert_eq!(result.skipped_blocks, 2);
        assert_eq!(result.written_blocks, 0);
        assert_eq!(result.bytes_written, 0);
        assert!((result.skip_percentage - 100.0).abs() < 0.1);
    }

    #[test]
    fn test_diff_write_different_files() {
        let source_data = vec![0xAAu8; 8192];
        let target_data = vec![0xBBu8; 8192];
        let source = create_test_file(&source_data);
        let target = create_test_file(&target_data);

        let config = DiffWriteConfig {
            source: source.path().to_str().unwrap().to_string(),
            target: target.path().to_str().unwrap().to_string(),
            block_size: 4096,
            verify: false,
            dry_run: false,
        };

        let progress = Progress::new(0);
        let result = diff_write(&config, &progress).unwrap();

        assert_eq!(result.total_blocks, 2);
        assert_eq!(result.written_blocks, 2);
        assert_eq!(result.skipped_blocks, 0);
        assert_eq!(result.bytes_written, 8192);

        // Verify target now matches source
        let target_final = std::fs::read(target.path()).unwrap();
        assert_eq!(target_final, source_data);
    }

    #[test]
    fn test_diff_write_partial_change() {
        // First block identical, second block different
        let mut source_data = vec![0xAAu8; 8192];
        let mut target_data = vec![0xAAu8; 8192];
        // Change only the second block in source
        for i in 4096..8192 {
            source_data[i] = 0xCC;
        }

        let source = create_test_file(&source_data);
        let target = create_test_file(&target_data);

        let config = DiffWriteConfig {
            source: source.path().to_str().unwrap().to_string(),
            target: target.path().to_str().unwrap().to_string(),
            block_size: 4096,
            verify: false,
            dry_run: false,
        };

        let progress = Progress::new(0);
        let result = diff_write(&config, &progress).unwrap();

        assert_eq!(result.total_blocks, 2);
        assert_eq!(result.skipped_blocks, 1);
        assert_eq!(result.written_blocks, 1);
        assert_eq!(result.bytes_written, 4096);
    }

    #[test]
    fn test_diff_write_dry_run() {
        let source_data = vec![0xAAu8; 4096];
        let target_data = vec![0xBBu8; 4096];
        let source = create_test_file(&source_data);
        let target = create_test_file(&target_data);

        let config = DiffWriteConfig {
            source: source.path().to_str().unwrap().to_string(),
            target: target.path().to_str().unwrap().to_string(),
            block_size: 4096,
            verify: false,
            dry_run: true,
        };

        let progress = Progress::new(0);
        let result = diff_write(&config, &progress).unwrap();

        assert_eq!(result.written_blocks, 1);
        // Target should be UNCHANGED because dry_run
        let target_final = std::fs::read(target.path()).unwrap();
        assert_eq!(target_final, target_data);
    }

    #[test]
    fn test_diff_write_with_verify() {
        let source_data = vec![0xAAu8; 4096];
        let target_data = vec![0xBBu8; 4096];
        let source = create_test_file(&source_data);
        let target = create_test_file(&target_data);

        let config = DiffWriteConfig {
            source: source.path().to_str().unwrap().to_string(),
            target: target.path().to_str().unwrap().to_string(),
            block_size: 4096,
            verify: true,
            dry_run: false,
        };

        let progress = Progress::new(0);
        let result = diff_write(&config, &progress).unwrap();

        assert_eq!(result.verified, Some(true));
    }

    #[test]
    fn test_diff_write_empty_source() {
        let source = create_test_file(&[]);
        let target = create_test_file(&[0u8; 4096]);

        let config = DiffWriteConfig {
            source: source.path().to_str().unwrap().to_string(),
            target: target.path().to_str().unwrap().to_string(),
            block_size: 4096,
            verify: false,
            dry_run: false,
        };

        let progress = Progress::new(0);
        let result = diff_write(&config, &progress).unwrap();

        assert_eq!(result.total_blocks, 0);
        assert_eq!(result.written_blocks, 0);
    }

    #[test]
    fn test_format_diff_result() {
        let result = DiffWriteResult {
            total_blocks: 100,
            skipped_blocks: 80,
            written_blocks: 20,
            bytes_written: 20 * 4096,
            bytes_examined: 100 * 4096,
            duration_ms: 500.0,
            effective_throughput_mib_s: 156.25,
            skip_percentage: 80.0,
            verified: Some(true),
        };

        let output = format_diff_result(&result);
        assert!(output.contains("Differential Write Results"));
        assert!(output.contains("80.0%"));
        assert!(output.contains("PASSED"));
    }

    #[test]
    fn test_read_full_block() {
        let data = vec![0xABu8; 1000];
        let mut reader = std::io::Cursor::new(data.clone());
        let mut buf = vec![0u8; 1000];
        let n = read_full_block(&mut reader, &mut buf).unwrap();
        assert_eq!(n, 1000);
        assert_eq!(&buf[..n], &data[..]);
    }

    #[test]
    fn test_diff_write_cancel() {
        let source_data = vec![0xAAu8; 16384];
        let target_data = vec![0xBBu8; 16384];
        let source = create_test_file(&source_data);
        let target = create_test_file(&target_data);

        let config = DiffWriteConfig {
            source: source.path().to_str().unwrap().to_string(),
            target: target.path().to_str().unwrap().to_string(),
            block_size: 4096,
            verify: false,
            dry_run: false,
        };

        let progress = Progress::new(0);
        progress.cancel(); // Cancel before starting

        let result = diff_write(&config, &progress).unwrap();
        // Should have 0 blocks since we cancelled immediately
        assert_eq!(result.total_blocks, 0);
    }
}
