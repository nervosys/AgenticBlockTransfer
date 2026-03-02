// Drive backup — save drive contents to a compressed image file.
// Supports raw, gzip, zstd compression with inline hashing.
// Inspired by Rufus's drive-to-VHD backup feature.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Backup compression format.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BackupCompression {
    /// No compression (raw copy).
    None,
    /// Gzip compression (.gz).
    Gzip,
    /// Zstandard compression (.zst).
    Zstd,
    /// Bzip2 compression (.bz2).
    Bzip2,
    /// XZ/LZMA compression (.xz).
    Xz,
}

impl BackupCompression {
    /// Suggested file extension for the compressed output.
    pub fn extension(&self) -> &str {
        match self {
            Self::None => "img",
            Self::Gzip => "img.gz",
            Self::Zstd => "img.zst",
            Self::Bzip2 => "img.bz2",
            Self::Xz => "img.xz",
        }
    }
}

impl fmt::Display for BackupCompression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none (raw)"),
            Self::Gzip => write!(f, "gzip"),
            Self::Zstd => write!(f, "zstd"),
            Self::Bzip2 => write!(f, "bzip2"),
            Self::Xz => write!(f, "xz"),
        }
    }
}

/// Backup configuration.
#[derive(Debug, Clone)]
pub struct BackupConfig {
    /// Source device or file path.
    pub source: String,
    /// Output file path.
    pub output: PathBuf,
    /// Compression format.
    pub compression: BackupCompression,
    /// Block size for I/O.
    pub block_size: usize,
    /// Number of bytes to read (None = entire device).
    pub size: Option<u64>,
    /// Whether to compute SHA-256 hash of the raw data.
    pub compute_hash: bool,
    /// Skip zero blocks in the output (sparse backup).
    pub sparse: bool,
    /// Zstd compression level (1-22, default 3).
    pub compression_level: i32,
    /// Gzip compression level (0-9, default 6).
    pub gzip_level: u32,
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self {
            source: String::new(),
            output: PathBuf::new(),
            compression: BackupCompression::Zstd,
            block_size: 4 * 1024 * 1024, // 4 MiB
            size: None,
            compute_hash: true,
            sparse: false,
            compression_level: 3,
            gzip_level: 6,
        }
    }
}

/// Result of a backup operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    /// Source device/file path.
    pub source: String,
    /// Output file path.
    pub output: String,
    /// Compression used.
    pub compression: String,
    /// Raw (uncompressed) bytes read from source.
    pub raw_bytes: u64,
    /// Compressed output size in bytes.
    pub compressed_bytes: u64,
    /// Compression ratio (compressed / raw).
    pub compression_ratio: f64,
    /// SHA-256 hash of the raw (uncompressed) data.
    pub sha256: Option<String>,
    /// Duration in seconds.
    pub duration_secs: f64,
    /// Average read speed in bytes/sec.
    pub read_speed: f64,
    /// Zero blocks skipped (if sparse).
    pub zero_blocks_skipped: u64,
}

impl BackupResult {
    /// Format the result as human-readable text.
    pub fn format_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("Backup Complete\n"));
        out.push_str(&format!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n"));
        out.push_str(&format!("Source:       {}\n", self.source));
        out.push_str(&format!("Output:       {}\n", self.output));
        out.push_str(&format!("Compression:  {}\n", self.compression));
        out.push_str(&format!(
            "Raw size:     {}\n",
            humansize::format_size(self.raw_bytes, humansize::BINARY)
        ));
        out.push_str(&format!(
            "Output size:  {}\n",
            humansize::format_size(self.compressed_bytes, humansize::BINARY)
        ));
        out.push_str(&format!("Ratio:        {:.1}%\n", self.compression_ratio * 100.0));
        if let Some(ref hash) = self.sha256 {
            out.push_str(&format!("SHA-256:      {}\n", hash));
        }
        out.push_str(&format!("Duration:     {:.1}s\n", self.duration_secs));
        out.push_str(&format!(
            "Read speed:   {}/s\n",
            humansize::format_size(self.read_speed as u64, humansize::BINARY)
        ));
        if self.zero_blocks_skipped > 0 {
            out.push_str(&format!("Zero blocks:  {} skipped\n", self.zero_blocks_skipped));
        }
        out
    }

    /// Export as JSON.
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }
}

/// Perform a drive/file backup with optional compression.
pub fn backup_drive<F>(
    config: &BackupConfig,
    mut progress_cb: F,
) -> Result<BackupResult>
where
    F: FnMut(u64, u64), // (bytes_read, total_bytes)
{
    let source_size = config.size.unwrap_or_else(|| {
        std::fs::metadata(&config.source)
            .map(|m| m.len())
            .unwrap_or(0)
    });

    if source_size == 0 {
        return Err(anyhow!("source size is 0 or could not be determined — use --size to specify"));
    }

    let start = Instant::now();
    let mut hasher = if config.compute_hash {
        Some(Sha256::new())
    } else {
        None
    };

    let source_file = std::fs::File::open(&config.source)
        .map_err(|e| anyhow!("failed to open source {}: {}", config.source, e))?;
    let mut reader = BufReader::with_capacity(config.block_size, source_file);

    let output_file = std::fs::File::create(&config.output)
        .map_err(|e| anyhow!("failed to create output {}: {}", config.output.display(), e))?;
    let buf_writer = BufWriter::with_capacity(config.block_size, output_file);

    let mut bytes_read: u64 = 0;
    let mut zero_blocks_skipped: u64 = 0;
    let mut buf = vec![0u8; config.block_size];

    // Create the appropriate compressed writer
    match config.compression {
        BackupCompression::None => {
            let mut writer = buf_writer;
            loop {
                let remaining = source_size.saturating_sub(bytes_read);
                if remaining == 0 {
                    break;
                }
                let to_read = (remaining as usize).min(config.block_size);
                let n = reader.read(&mut buf[..to_read])?;
                if n == 0 {
                    break;
                }

                if let Some(ref mut h) = hasher {
                    h.update(&buf[..n]);
                }

                if config.sparse && is_zero_block(&buf[..n]) {
                    zero_blocks_skipped += 1;
                } else {
                    writer.write_all(&buf[..n])?;
                }

                bytes_read += n as u64;
                progress_cb(bytes_read, source_size);
            }
            writer.flush()?;
        }
        BackupCompression::Gzip => {
            let mut encoder = flate2::write::GzEncoder::new(
                buf_writer,
                flate2::Compression::new(config.gzip_level),
            );
            loop {
                let remaining = source_size.saturating_sub(bytes_read);
                if remaining == 0 {
                    break;
                }
                let to_read = (remaining as usize).min(config.block_size);
                let n = reader.read(&mut buf[..to_read])?;
                if n == 0 {
                    break;
                }
                if let Some(ref mut h) = hasher {
                    h.update(&buf[..n]);
                }
                encoder.write_all(&buf[..n])?;
                bytes_read += n as u64;
                progress_cb(bytes_read, source_size);
            }
            encoder.finish()?;
        }
        BackupCompression::Zstd => {
            let mut encoder = zstd::Encoder::new(buf_writer, config.compression_level)?;
            loop {
                let remaining = source_size.saturating_sub(bytes_read);
                if remaining == 0 {
                    break;
                }
                let to_read = (remaining as usize).min(config.block_size);
                let n = reader.read(&mut buf[..to_read])?;
                if n == 0 {
                    break;
                }
                if let Some(ref mut h) = hasher {
                    h.update(&buf[..n]);
                }
                encoder.write_all(&buf[..n])?;
                bytes_read += n as u64;
                progress_cb(bytes_read, source_size);
            }
            encoder.finish()?;
        }
        BackupCompression::Bzip2 => {
            let mut encoder = bzip2::write::BzEncoder::new(
                buf_writer,
                bzip2::Compression::new(config.compression_level.clamp(1, 9) as u32),
            );
            loop {
                let remaining = source_size.saturating_sub(bytes_read);
                if remaining == 0 {
                    break;
                }
                let to_read = (remaining as usize).min(config.block_size);
                let n = reader.read(&mut buf[..to_read])?;
                if n == 0 {
                    break;
                }
                if let Some(ref mut h) = hasher {
                    h.update(&buf[..n]);
                }
                encoder.write_all(&buf[..n])?;
                bytes_read += n as u64;
                progress_cb(bytes_read, source_size);
            }
            encoder.finish()?;
        }
        BackupCompression::Xz => {
            let mut encoder = xz2::write::XzEncoder::new(
                buf_writer,
                config.compression_level.clamp(0, 9) as u32,
            );
            loop {
                let remaining = source_size.saturating_sub(bytes_read);
                if remaining == 0 {
                    break;
                }
                let to_read = (remaining as usize).min(config.block_size);
                let n = reader.read(&mut buf[..to_read])?;
                if n == 0 {
                    break;
                }
                if let Some(ref mut h) = hasher {
                    h.update(&buf[..n]);
                }
                encoder.write_all(&buf[..n])?;
                bytes_read += n as u64;
                progress_cb(bytes_read, source_size);
            }
            encoder.finish()?;
        }
    }

    let elapsed = start.elapsed();
    let compressed_bytes = std::fs::metadata(&config.output)?.len();
    let sha256 = hasher.map(|h| hex::encode(h.finalize()));

    let ratio = if bytes_read > 0 {
        compressed_bytes as f64 / bytes_read as f64
    } else {
        0.0
    };

    let read_speed = if elapsed.as_secs_f64() > 0.0 {
        bytes_read as f64 / elapsed.as_secs_f64()
    } else {
        0.0
    };

    Ok(BackupResult {
        source: config.source.clone(),
        output: config.output.display().to_string(),
        compression: format!("{}", config.compression),
        raw_bytes: bytes_read,
        compressed_bytes,
        compression_ratio: ratio,
        sha256,
        duration_secs: elapsed.as_secs_f64(),
        read_speed,
        zero_blocks_skipped,
    })
}

/// Check if a block is all zeros (word-aligned comparison).
fn is_zero_block(data: &[u8]) -> bool {
    // Check as u64 words for speed
    let (prefix, words, suffix) = unsafe { data.align_to::<u64>() };
    prefix.iter().all(|&b| b == 0)
        && words.iter().all(|&w| w == 0)
        && suffix.iter().all(|&b| b == 0)
}

/// Suggest an output filename based on source and compression.
pub fn suggest_output_name(source: &str, compression: BackupCompression) -> PathBuf {
    let base = Path::new(source)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("backup");

    let sanitized: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();

    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    PathBuf::from(format!("{}_{}.{}", sanitized, timestamp, compression.extension()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_extension() {
        assert_eq!(BackupCompression::None.extension(), "img");
        assert_eq!(BackupCompression::Gzip.extension(), "img.gz");
        assert_eq!(BackupCompression::Zstd.extension(), "img.zst");
        assert_eq!(BackupCompression::Bzip2.extension(), "img.bz2");
        assert_eq!(BackupCompression::Xz.extension(), "img.xz");
    }

    #[test]
    fn test_compression_display() {
        assert_eq!(format!("{}", BackupCompression::None), "none (raw)");
        assert_eq!(format!("{}", BackupCompression::Gzip), "gzip");
        assert_eq!(format!("{}", BackupCompression::Zstd), "zstd");
    }

    #[test]
    fn test_default_config() {
        let cfg = BackupConfig::default();
        assert_eq!(cfg.compression, BackupCompression::Zstd);
        assert_eq!(cfg.block_size, 4 * 1024 * 1024);
        assert!(cfg.compute_hash);
    }

    #[test]
    fn test_is_zero_block() {
        assert!(is_zero_block(&[0u8; 4096]));
        assert!(!is_zero_block(&[1u8; 4096]));
        let mut data = vec![0u8; 4096];
        data[2048] = 1;
        assert!(!is_zero_block(&data));
    }

    #[test]
    fn test_backup_raw() {
        let src = tempfile::NamedTempFile::new().unwrap();
        let data = vec![0xABu8; 16384];
        std::fs::write(src.path(), &data).unwrap();

        let dst = tempfile::NamedTempFile::new().unwrap();
        let config = BackupConfig {
            source: src.path().to_str().unwrap().to_string(),
            output: dst.path().to_path_buf(),
            compression: BackupCompression::None,
            block_size: 4096,
            size: Some(16384),
            compute_hash: true,
            sparse: false,
            compression_level: 3,
            gzip_level: 6,
        };

        let result = backup_drive(&config, |_, _| {}).unwrap();
        assert_eq!(result.raw_bytes, 16384);
        assert_eq!(result.compressed_bytes, 16384);
        assert!(result.sha256.is_some());
        assert!(result.duration_secs >= 0.0);
    }

    #[test]
    fn test_backup_gzip() {
        let src = tempfile::NamedTempFile::new().unwrap();
        let data = vec![0x00u8; 32768]; // Compressible data
        std::fs::write(src.path(), &data).unwrap();

        let dst = tempfile::NamedTempFile::new().unwrap();
        let config = BackupConfig {
            source: src.path().to_str().unwrap().to_string(),
            output: dst.path().to_path_buf(),
            compression: BackupCompression::Gzip,
            block_size: 4096,
            size: Some(32768),
            compute_hash: true,
            sparse: false,
            compression_level: 3,
            gzip_level: 6,
        };

        let result = backup_drive(&config, |_, _| {}).unwrap();
        assert_eq!(result.raw_bytes, 32768);
        assert!(result.compressed_bytes < 32768); // Should compress well
        assert!(result.compression_ratio < 1.0);
    }

    #[test]
    fn test_backup_zstd() {
        let src = tempfile::NamedTempFile::new().unwrap();
        let data = vec![0x55u8; 16384];
        std::fs::write(src.path(), &data).unwrap();

        let dst = tempfile::NamedTempFile::new().unwrap();
        let config = BackupConfig {
            source: src.path().to_str().unwrap().to_string(),
            output: dst.path().to_path_buf(),
            compression: BackupCompression::Zstd,
            block_size: 4096,
            size: Some(16384),
            compute_hash: false,
            sparse: false,
            compression_level: 3,
            gzip_level: 6,
        };

        let result = backup_drive(&config, |_, _| {}).unwrap();
        assert_eq!(result.raw_bytes, 16384);
        assert!(result.sha256.is_none()); // Hash not requested
    }

    #[test]
    fn test_backup_bzip2() {
        let src = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(src.path(), vec![0xCC; 8192]).unwrap();

        let dst = tempfile::NamedTempFile::new().unwrap();
        let config = BackupConfig {
            source: src.path().to_str().unwrap().to_string(),
            output: dst.path().to_path_buf(),
            compression: BackupCompression::Bzip2,
            block_size: 4096,
            size: Some(8192),
            compute_hash: true,
            sparse: false,
            compression_level: 3,
            gzip_level: 6,
        };

        let result = backup_drive(&config, |_, _| {}).unwrap();
        assert_eq!(result.raw_bytes, 8192);
    }

    #[test]
    fn test_backup_xz() {
        let src = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(src.path(), vec![0x11; 8192]).unwrap();

        let dst = tempfile::NamedTempFile::new().unwrap();
        let config = BackupConfig {
            source: src.path().to_str().unwrap().to_string(),
            output: dst.path().to_path_buf(),
            compression: BackupCompression::Xz,
            block_size: 4096,
            size: Some(8192),
            compute_hash: true,
            sparse: false,
            compression_level: 3,
            gzip_level: 6,
        };

        let result = backup_drive(&config, |_, _| {}).unwrap();
        assert_eq!(result.raw_bytes, 8192);
    }

    #[test]
    fn test_backup_result_text() {
        let result = BackupResult {
            source: "/dev/sdb".into(),
            output: "backup.img.zst".into(),
            compression: "zstd".into(),
            raw_bytes: 1_000_000,
            compressed_bytes: 500_000,
            compression_ratio: 0.5,
            sha256: Some("abc123".into()),
            duration_secs: 2.5,
            read_speed: 400_000.0,
            zero_blocks_skipped: 0,
        };
        let text = result.format_text();
        assert!(text.contains("Backup Complete"));
        assert!(text.contains("/dev/sdb"));
        assert!(text.contains("abc123"));
    }

    #[test]
    fn test_backup_result_json() {
        let result = BackupResult {
            source: "test".into(),
            output: "out.img".into(),
            compression: "none".into(),
            raw_bytes: 1024,
            compressed_bytes: 1024,
            compression_ratio: 1.0,
            sha256: None,
            duration_secs: 0.1,
            read_speed: 10240.0,
            zero_blocks_skipped: 0,
        };
        let json = result.to_json().unwrap();
        assert!(json.contains("\"source\": \"test\""));
    }

    #[test]
    fn test_suggest_output_name() {
        let name = suggest_output_name("/dev/sdb", BackupCompression::Zstd);
        assert!(name.to_str().unwrap().contains("img.zst"));
    }

    #[test]
    fn test_backup_sparse() {
        let src = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(src.path(), vec![0u8; 8192]).unwrap(); // All zeros

        let dst = tempfile::NamedTempFile::new().unwrap();
        let config = BackupConfig {
            source: src.path().to_str().unwrap().to_string(),
            output: dst.path().to_path_buf(),
            compression: BackupCompression::None,
            block_size: 4096,
            size: Some(8192),
            compute_hash: true,
            sparse: true,
            compression_level: 3,
            gzip_level: 6,
        };

        let result = backup_drive(&config, |_, _| {}).unwrap();
        assert_eq!(result.zero_blocks_skipped, 2); // 8192 / 4096 = 2 zero blocks
        assert_eq!(result.compressed_bytes, 0); // Nothing written
    }

    #[test]
    fn test_backup_progress_callback() {
        let src = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(src.path(), vec![0xAA; 8192]).unwrap();

        let dst = tempfile::NamedTempFile::new().unwrap();
        let config = BackupConfig {
            source: src.path().to_str().unwrap().to_string(),
            output: dst.path().to_path_buf(),
            compression: BackupCompression::None,
            block_size: 4096,
            size: Some(8192),
            compute_hash: false,
            sparse: false,
            compression_level: 3,
            gzip_level: 6,
        };

        let mut callbacks = 0u32;
        backup_drive(&config, |_, _| callbacks += 1).unwrap();
        assert!(callbacks >= 2);
    }
}
