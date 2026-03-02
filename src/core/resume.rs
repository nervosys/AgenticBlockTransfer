// Error recovery — resume interrupted writes from a checkpoint.
//
// When a write operation is interrupted (Ctrl+C, power loss, transient error),
// a checkpoint file records how far the write got. On restart, `abt write --resume`
// picks up from where it left off, re-hashing the already-written region to
// verify integrity before continuing.
//
// Checkpoint file format: JSON at `<target>.abt-checkpoint`
//   { "source": "...", "target": "...", "bytes_written": N, "hash_at_checkpoint": "...",
//     "algorithm": "...", "block_size": N, "timestamp": "..." }

#![allow(dead_code)]

use anyhow::{bail, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{PathBuf};

/// Checkpoint data persisted between interrupted sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteCheckpoint {
    /// Source image path (absolute).
    pub source: String,
    /// Target device/file path.
    pub target: String,
    /// Number of bytes successfully written and verified at checkpoint time.
    pub bytes_written: u64,
    /// Cumulative hash of all data written up to `bytes_written`.
    pub hash_at_checkpoint: String,
    /// Hash algorithm used.
    pub algorithm: String,
    /// Block size used for the original write.
    pub block_size: usize,
    /// ISO 8601 timestamp of checkpoint creation.
    pub timestamp: String,
    /// abt version that created the checkpoint.
    pub version: String,
}

impl WriteCheckpoint {
    /// Create a new checkpoint.
    pub fn new(
        source: &str,
        target: &str,
        bytes_written: u64,
        hash_at_checkpoint: &str,
        algorithm: &str,
        block_size: usize,
    ) -> Self {
        Self {
            source: source.to_string(),
            target: target.to_string(),
            bytes_written,
            hash_at_checkpoint: hash_at_checkpoint.to_string(),
            algorithm: algorithm.to_string(),
            block_size,
            timestamp: chrono::Utc::now().to_rfc3339(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Derive the checkpoint file path from a target path.
    /// e.g., `/dev/sdb` → `/tmp/abt-checkpoint-sdb.json`
    /// e.g., `C:\test.img` → `C:\Users\...\AppData\Local\Temp\abt-checkpoint-test.img.json`
    pub fn checkpoint_path(target: &str) -> PathBuf {
        let sanitized: String = target
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
            .collect();
        std::env::temp_dir().join(format!("abt-checkpoint-{}.json", sanitized))
    }

    /// Save this checkpoint to disk.
    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::checkpoint_path(&self.target);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, &json)?;
        info!("Checkpoint saved: {} ({} bytes written)", path.display(), self.bytes_written);
        Ok(path)
    }

    /// Load a checkpoint for a given target, if one exists.
    pub fn load(target: &str) -> Result<Option<Self>> {
        let path = Self::checkpoint_path(target);
        if !path.exists() {
            return Ok(None);
        }

        let json = std::fs::read_to_string(&path)?;
        let checkpoint: Self = serde_json::from_str(&json)?;

        info!(
            "Found checkpoint for {}: {} bytes written at {}",
            target, checkpoint.bytes_written, checkpoint.timestamp
        );

        Ok(Some(checkpoint))
    }

    /// Remove the checkpoint file after a successful write.
    pub fn remove(target: &str) -> Result<()> {
        let path = Self::checkpoint_path(target);
        if path.exists() {
            std::fs::remove_file(&path)?;
            info!("Checkpoint removed: {}", path.display());
        }
        Ok(())
    }

    /// Validate that this checkpoint is compatible with the current write operation.
    /// Checks source path, target path, and algorithm match.
    pub fn validate(&self, source: &str, target: &str, algorithm: &str) -> Result<()> {
        if self.target != target {
            bail!(
                "Checkpoint target mismatch: checkpoint is for '{}', but write target is '{}'",
                self.target,
                target
            );
        }

        if self.source != source {
            warn!(
                "Checkpoint source differs: checkpoint='{}', current='{}'. \
                 Will re-hash already-written region to verify.",
                self.source, source
            );
        }

        if self.algorithm != algorithm {
            bail!(
                "Checkpoint hash algorithm mismatch: checkpoint used '{}', current uses '{}'",
                self.algorithm,
                algorithm
            );
        }

        if self.bytes_written == 0 {
            warn!("Checkpoint has 0 bytes written — starting from beginning");
        }

        Ok(())
    }

    /// Verify the already-written region of the target matches the checkpoint hash.
    /// This prevents resuming from a corrupted state.
    pub fn verify_written_region(&self, target: &str) -> Result<bool> {
        use crate::core::hasher;
        use crate::core::progress::Progress;
        use crate::core::types::HashAlgorithm;

        let algo: HashAlgorithm = match self.algorithm.to_lowercase().as_str() {
            "md5" => HashAlgorithm::Md5,
            "sha1" => HashAlgorithm::Sha1,
            "sha256" => HashAlgorithm::Sha256,
            "sha512" => HashAlgorithm::Sha512,
            "blake3" => HashAlgorithm::Blake3,
            "crc32" => HashAlgorithm::Crc32,
            _ => bail!("Unknown hash algorithm in checkpoint: {}", self.algorithm),
        };

        info!(
            "Verifying {} bytes of already-written data on {}...",
            self.bytes_written, target
        );

        let file = std::fs::File::open(target)?;
        let limited = file.take(self.bytes_written);
        let mut reader = std::io::BufReader::with_capacity(4 * 1024 * 1024, limited);

        let progress = Progress::new(self.bytes_written);
        let actual_hash = hasher::hash_reader(&mut reader, algo, &progress)?;

        if actual_hash == self.hash_at_checkpoint {
            info!("Checkpoint verification passed — written region is intact");
            Ok(true)
        } else {
            warn!(
                "Checkpoint verification FAILED: expected {}, got {}",
                self.hash_at_checkpoint, actual_hash
            );
            Ok(false)
        }
    }
}

/// Resume information returned to the writer.
#[derive(Debug, Clone)]
pub struct ResumeInfo {
    /// Byte offset to resume writing from.
    pub resume_offset: u64,
    /// Block size from the checkpoint (should match current config).
    pub block_size: usize,
    /// The checkpoint data.
    pub checkpoint: WriteCheckpoint,
}

/// Attempt to load and validate a resume checkpoint for a write operation.
/// Returns None if no valid checkpoint exists.
pub fn try_resume(
    source: &str,
    target: &str,
    algorithm: &str,
) -> Result<Option<ResumeInfo>> {
    let checkpoint = match WriteCheckpoint::load(target)? {
        Some(cp) => cp,
        None => return Ok(None),
    };

    // Validate compatibility
    if let Err(e) = checkpoint.validate(source, target, algorithm) {
        warn!("Cannot resume: {}. Starting fresh.", e);
        WriteCheckpoint::remove(target)?;
        return Ok(None);
    }

    // Verify the already-written data
    match checkpoint.verify_written_region(target) {
        Ok(true) => {
            info!(
                "Resuming write from offset {} ({} already written)",
                checkpoint.bytes_written,
                humansize::format_size(checkpoint.bytes_written, humansize::BINARY)
            );
            Ok(Some(ResumeInfo {
                resume_offset: checkpoint.bytes_written,
                block_size: checkpoint.block_size,
                checkpoint,
            }))
        }
        Ok(false) => {
            warn!("Written region corrupted — cannot resume. Starting fresh.");
            WriteCheckpoint::remove(target)?;
            Ok(None)
        }
        Err(e) => {
            warn!("Could not verify checkpoint: {}. Starting fresh.", e);
            WriteCheckpoint::remove(target)?;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_path_sanitization() {
        let path = WriteCheckpoint::checkpoint_path("/dev/sdb");
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("abt-checkpoint-"));
        assert!(name.ends_with(".json"));
        assert!(!name.contains('/'));
    }

    #[test]
    fn checkpoint_path_windows_style() {
        let path = WriteCheckpoint::checkpoint_path(r"\\.\PhysicalDrive1");
        let name = path.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("abt-checkpoint-"));
        assert!(!name.contains('\\'));
    }

    #[test]
    fn checkpoint_roundtrip() {
        let cp = WriteCheckpoint::new(
            "/tmp/test.img",
            "/tmp/target.img",
            65536,
            "abc123def456",
            "sha256",
            4 * 1024 * 1024,
        );

        let path = cp.save().unwrap();
        assert!(path.exists());

        let loaded = WriteCheckpoint::load("/tmp/target.img").unwrap().unwrap();
        assert_eq!(loaded.source, "/tmp/test.img");
        assert_eq!(loaded.bytes_written, 65536);
        assert_eq!(loaded.hash_at_checkpoint, "abc123def456");
        assert_eq!(loaded.algorithm, "sha256");
        assert_eq!(loaded.block_size, 4 * 1024 * 1024);

        WriteCheckpoint::remove("/tmp/target.img").unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn checkpoint_validate_matching() {
        let cp = WriteCheckpoint::new("src.img", "tgt.img", 1024, "hash", "sha256", 4096);
        assert!(cp.validate("src.img", "tgt.img", "sha256").is_ok());
    }

    #[test]
    fn checkpoint_validate_algo_mismatch() {
        let cp = WriteCheckpoint::new("src.img", "tgt.img", 1024, "hash", "sha256", 4096);
        assert!(cp.validate("src.img", "tgt.img", "blake3").is_err());
    }

    #[test]
    fn checkpoint_validate_target_mismatch() {
        let cp = WriteCheckpoint::new("src.img", "tgt.img", 1024, "hash", "sha256", 4096);
        assert!(cp.validate("src.img", "other.img", "sha256").is_err());
    }

    #[test]
    fn load_nonexistent_returns_none() {
        let result = WriteCheckpoint::load("/nonexistent/device/path/xyz123").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn new_checkpoint_has_version() {
        let cp = WriteCheckpoint::new("s", "t", 0, "h", "sha256", 4096);
        assert!(!cp.version.is_empty());
        assert!(!cp.timestamp.is_empty());
    }
}
