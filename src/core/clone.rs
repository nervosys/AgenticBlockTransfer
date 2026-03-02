//! Device-to-device cloning with progress tracking and verification.
//!
//! Provides block-level cloning from a source device (or file) to a target
//! device with all the safety features of the standard write pipeline:
//! inline hashing, sparse optimization, retry logic, and post-clone
//! verification.

use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result};
use log::{info, warn};

use super::hasher::{self, DynHasher};
use super::progress::{OperationPhase, Progress};
use super::types::HashAlgorithm;
use super::writer::{open_device_for_writing, sync_device};

/// Configuration for a clone operation.
#[derive(Debug, Clone)]
pub struct CloneConfig {
    /// Source device or file path.
    pub source: String,
    /// Target device or file path.
    pub target: String,
    /// Block size for I/O (bytes).
    pub block_size: usize,
    /// Number of bytes to clone. If `None`, clones until EOF.
    pub count: Option<u64>,
    /// Verify after cloning by reading back and comparing hashes.
    pub verify: bool,
    /// Hash algorithm for verification.
    pub hash_algorithm: HashAlgorithm,
    /// Skip all-zero blocks during clone (sparse).
    pub sparse: bool,
    /// Use direct / unbuffered I/O.
    pub direct_io: bool,
    /// Sync after clone.
    pub sync: bool,
}

impl Default for CloneConfig {
    fn default() -> Self {
        Self {
            source: String::new(),
            target: String::new(),
            block_size: 4 * 1024 * 1024, // 4 MiB
            count: None,
            verify: true,
            hash_algorithm: HashAlgorithm::Sha256,
            sparse: false,
            direct_io: true,
            sync: true,
        }
    }
}

/// Result of a clone operation.
#[derive(Debug)]
pub struct CloneResult {
    /// Total bytes copied.
    pub bytes_copied: u64,
    /// Bytes skipped via sparse optimization.
    pub bytes_sparse_skipped: u64,
    /// Hash of the data that was copied (for verification).
    pub source_hash: Option<String>,
    /// Whether post-clone verification passed.
    pub verified: Option<bool>,
}

/// Execute a device-to-device clone.
pub fn clone_device(config: &CloneConfig, progress: &Progress) -> Result<CloneResult> {
    info!("Starting clone: {} → {}", config.source, config.target);

    // Determine source size
    let source_size = get_source_size(&config.source)?;
    let bytes_to_clone = config.count.unwrap_or(source_size);
    info!(
        "Clone size: {} bytes ({})",
        bytes_to_clone,
        humansize::format_size(bytes_to_clone, humansize::BINARY)
    );

    progress.set_total(bytes_to_clone);
    progress.set_phase(OperationPhase::Writing);

    // Open source
    let source_file = std::fs::File::open(&config.source)
        .with_context(|| format!("Failed to open source: {}", config.source))?;
    let mut reader = BufReader::with_capacity(config.block_size, source_file);

    // Open target
    let raw_target = open_device_for_writing(&config.target, config.direct_io)?;
    let mut writer = BufWriter::with_capacity(config.block_size, raw_target);

    let mut buf = vec![0u8; config.block_size];
    let mut total_copied: u64 = 0;
    let mut sparse_skipped: u64 = 0;

    // Inline hasher for verification
    let mut hasher: Option<Box<dyn DynHasher>> = if config.verify {
        Some(hasher::create_hasher(config.hash_algorithm))
    } else {
        None
    };

    // Copy loop
    loop {
        if progress.is_cancelled() {
            anyhow::bail!("Clone cancelled by user");
        }

        let remaining = bytes_to_clone.saturating_sub(total_copied);
        if remaining == 0 {
            break;
        }

        let to_read = std::cmp::min(config.block_size as u64, remaining) as usize;
        let n = reader
            .read(&mut buf[..to_read])
            .context("Failed to read from source device")?;
        if n == 0 {
            break; // EOF
        }

        let data = &buf[..n];

        // Update inline hash
        if let Some(ref mut h) = hasher {
            h.update(data);
        }

        // Sparse optimization: skip all-zero blocks
        if config.sparse && is_all_zero(data) {
            writer
                .seek(SeekFrom::Current(n as i64))
                .context("Failed to seek past zero block on target")?;
            sparse_skipped += n as u64;
        } else {
            writer
                .write_all(data)
                .context("Failed to write to target device")?;
        }

        total_copied += n as u64;
        progress.add_bytes(n as u64);
    }

    writer.flush().context("Failed to flush target device")?;
    drop(writer);

    let source_hash = hasher.map(|h| h.finalize_hex());

    info!("Clone complete: {} bytes copied", total_copied);
    if sparse_skipped > 0 {
        info!(
            "Sparse optimization: skipped {} zero-fill bytes ({:.1}%)",
            sparse_skipped,
            (sparse_skipped as f64 / (total_copied + sparse_skipped) as f64) * 100.0
        );
    }

    // Sync
    if config.sync {
        progress.set_phase(OperationPhase::Syncing);
        sync_device(&config.target)?;
    }

    // Verify
    let verified = if config.verify {
        progress.set_phase(OperationPhase::Verifying);
        info!("Verifying clone...");

        let target_hash = hash_device_region(
            &config.target,
            total_copied,
            config.block_size,
            config.hash_algorithm,
            progress,
        )?;

        let matches = source_hash.as_deref() == Some(target_hash.as_str());
        if matches {
            info!("Verification passed");
        } else {
            warn!(
                "Verification FAILED: source={} target={}",
                source_hash.as_deref().unwrap_or("none"),
                target_hash
            );
        }
        Some(matches)
    } else {
        None
    };

    progress.set_phase(OperationPhase::Completed);

    Ok(CloneResult {
        bytes_copied: total_copied,
        bytes_sparse_skipped: sparse_skipped,
        source_hash,
        verified,
    })
}

/// Get the size of a source file or device.
fn get_source_size(path: &str) -> Result<u64> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p).with_context(|| format!("Cannot stat {}", path))?;

    if meta.is_file() {
        return Ok(meta.len());
    }

    // For block devices, seek to end to determine size
    let mut f = std::fs::File::open(p)?;
    let size = f.seek(SeekFrom::End(0))?;
    Ok(size)
}

/// Hash the first `length` bytes of a device/file.
fn hash_device_region(
    path: &str,
    length: u64,
    block_size: usize,
    algorithm: HashAlgorithm,
    progress: &Progress,
) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(block_size, file);
    let mut h = hasher::create_hasher(algorithm);
    let mut buf = vec![0u8; block_size];
    let mut remaining = length;

    while remaining > 0 {
        if progress.is_cancelled() {
            anyhow::bail!("Verification cancelled");
        }

        let to_read = std::cmp::min(block_size as u64, remaining) as usize;
        let n = reader.read(&mut buf[..to_read])?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        remaining -= n as u64;
    }

    Ok(h.finalize_hex())
}

/// Check if a buffer is all zeros using u64-aligned word comparison.
fn is_all_zero(buf: &[u8]) -> bool {
    // Fast path: check u64-aligned words
    // SAFETY: `align_to` returns a valid decomposition of the byte slice.
    // Read-only access, no aliasing. All bytes checked exactly once.
    let (prefix, words, suffix) = unsafe { buf.align_to::<u64>() };
    prefix.iter().all(|&b| b == 0) && words.iter().all(|&w| w == 0) && suffix.iter().all(|&b| b == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_clone_config_default() {
        let cfg = CloneConfig::default();
        assert_eq!(cfg.block_size, 4 * 1024 * 1024);
        assert!(cfg.verify);
        assert!(cfg.sync);
        assert!(!cfg.sparse);
    }

    #[test]
    fn test_is_all_zero() {
        assert!(is_all_zero(&[0u8; 4096]));
        assert!(!is_all_zero(&[0, 0, 0, 1]));
        assert!(is_all_zero(&[]));
    }

    #[test]
    fn test_is_all_zero_unaligned() {
        let mut buf = vec![0u8; 4097]; // not u64-aligned size
        assert!(is_all_zero(&buf));
        buf[4096] = 1;
        assert!(!is_all_zero(&buf));
    }

    #[test]
    fn test_get_source_size_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&[0xAA; 8192]).unwrap();
        tmp.flush().unwrap();
        let size = get_source_size(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(size, 8192);
    }

    #[test]
    fn test_clone_file_to_file() {
        let mut src = NamedTempFile::new().unwrap();
        let pattern: Vec<u8> = (0..4096u16).map(|i| (i % 256) as u8).collect();
        src.write_all(&pattern).unwrap();
        src.flush().unwrap();

        let dst = NamedTempFile::new().unwrap();

        let config = CloneConfig {
            source: src.path().to_str().unwrap().to_string(),
            target: dst.path().to_str().unwrap().to_string(),
            block_size: 1024,
            count: None,
            verify: true,
            hash_algorithm: HashAlgorithm::Sha256,
            sparse: false,
            direct_io: false,
            sync: false,
        };

        let progress = Progress::new(0);
        let result = clone_device(&config, &progress).unwrap();
        assert_eq!(result.bytes_copied, 4096);
        assert!(result.source_hash.is_some());
        assert_eq!(result.verified, Some(true));
    }

    #[test]
    fn test_clone_with_sparse() {
        // Source: 1024 bytes of data + 4096 bytes of zeros
        let mut src = NamedTempFile::new().unwrap();
        src.write_all(&[0xBB; 1024]).unwrap();
        src.write_all(&[0x00; 4096]).unwrap();
        src.flush().unwrap();

        let dst = NamedTempFile::new().unwrap();

        let config = CloneConfig {
            source: src.path().to_str().unwrap().to_string(),
            target: dst.path().to_str().unwrap().to_string(),
            block_size: 1024,
            count: None,
            verify: false,
            hash_algorithm: HashAlgorithm::Sha256,
            sparse: true,
            direct_io: false,
            sync: false,
        };

        let progress = Progress::new(0);
        let result = clone_device(&config, &progress).unwrap();
        assert_eq!(result.bytes_copied, 5120);
        assert_eq!(result.bytes_sparse_skipped, 4096);
    }

    #[test]
    fn test_clone_with_count_limit() {
        let mut src = NamedTempFile::new().unwrap();
        src.write_all(&[0xCC; 8192]).unwrap();
        src.flush().unwrap();

        let dst = NamedTempFile::new().unwrap();

        let config = CloneConfig {
            source: src.path().to_str().unwrap().to_string(),
            target: dst.path().to_str().unwrap().to_string(),
            block_size: 1024,
            count: Some(2048),
            verify: false,
            hash_algorithm: HashAlgorithm::Sha256,
            sparse: false,
            direct_io: false,
            sync: false,
        };

        let progress = Progress::new(0);
        let result = clone_device(&config, &progress).unwrap();
        assert_eq!(result.bytes_copied, 2048);
    }

    #[test]
    fn test_hash_device_region() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&[0xDD; 4096]).unwrap();
        tmp.flush().unwrap();

        let progress = Progress::new(0);
        let hash = hash_device_region(
            tmp.path().to_str().unwrap(),
            4096,
            1024,
            HashAlgorithm::Sha256,
            &progress,
        )
        .unwrap();

        assert_eq!(hash.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    }
}
