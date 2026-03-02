use anyhow::Result;
use log::info;
use std::io::Read;
use std::path::Path;

use super::hasher;
use super::progress::{OperationPhase, Progress};
use super::types::HashAlgorithm;

/// Verify a device against a source image using hash comparison.
pub fn verify_by_hash(
    source: &Path,
    target: &str,
    algorithm: HashAlgorithm,
    progress: &Progress,
) -> Result<bool> {
    progress.set_phase(OperationPhase::Verifying);

    // Hash the source
    info!("Hashing source: {}", source.display());
    let src_hash = hasher::hash_file(source, algorithm, progress)?;
    info!("Source {}: {}", algorithm, src_hash);

    // Hash the target (only the number of bytes in the source)
    let source_size = std::fs::metadata(source)?.len();
    info!("Hashing target: {} ({} bytes)", target, source_size);

    let target_file = std::fs::File::open(target)?;
    let mut target_reader = std::io::BufReader::with_capacity(
        4 * 1024 * 1024,
        target_file.take(source_size),
    );
    let tgt_hash = hasher::hash_reader(&mut target_reader, algorithm, progress)?;
    info!("Target {}: {}", algorithm, tgt_hash);

    if src_hash == tgt_hash {
        info!("Verification passed: hashes match");
        progress.set_phase(OperationPhase::Completed);
        Ok(true)
    } else {
        info!("Verification FAILED: hash mismatch");
        progress.set_phase(OperationPhase::Failed);
        Ok(false)
    }
}

/// Verify a written image against an expected hash string.
pub fn verify_against_hash(
    target: &str,
    expected_hash: &str,
    algorithm: HashAlgorithm,
    size: Option<u64>,
    progress: &Progress,
) -> Result<bool> {
    progress.set_phase(OperationPhase::Verifying);

    let target_file = std::fs::File::open(target)?;
    let reader: Box<dyn Read + Send> = if let Some(sz) = size {
        Box::new(target_file.take(sz))
    } else {
        Box::new(target_file)
    };
    let mut buf_reader = std::io::BufReader::with_capacity(4 * 1024 * 1024, reader);
    let actual_hash = hasher::hash_reader(&mut buf_reader, algorithm, progress)?;

    let matches = actual_hash.eq_ignore_ascii_case(expected_hash);
    if matches {
        info!("Verification passed");
        progress.set_phase(OperationPhase::Completed);
    } else {
        info!(
            "Verification FAILED: expected {}, got {}",
            expected_hash, actual_hash
        );
        progress.set_phase(OperationPhase::Failed);
    }
    Ok(matches)
}

/// Memory-mapped verification: mmap the target file and hash it using
/// zero-copy I/O. This avoids read(2) syscall overhead for large files
/// and lets the kernel manage page-level caching optimally.
#[allow(dead_code)]
///
/// Falls back to `verify_by_hash` if the target cannot be mmapped
/// (e.g., raw block devices on some platforms).
pub fn verify_mmap(
    source: &Path,
    target: &str,
    algorithm: HashAlgorithm,
    progress: &Progress,
) -> Result<bool> {
    progress.set_phase(OperationPhase::Verifying);

    // Hash the source (regular file — always use standard I/O)
    info!("Hashing source: {}", source.display());
    let src_hash = hasher::hash_file(source, algorithm, progress)?;
    info!("Source {}: {}", algorithm, src_hash);

    let source_size = std::fs::metadata(source)?.len();
    info!(
        "Verifying target (mmap): {} ({} bytes)",
        target, source_size
    );

    // Try memory-mapped read of the target
    let target_file = std::fs::File::open(target)?;
    let mmap = match unsafe { memmap2::Mmap::map(&target_file) } {
        Ok(m) => m,
        Err(e) => {
            info!(
                "mmap failed ({}), falling back to standard I/O verification",
                e
            );
            // Fall back to buffered I/O
            drop(target_file);
            return verify_by_hash(source, target, algorithm, progress);
        }
    };

    // Limit to source size
    let len = source_size.min(mmap.len() as u64) as usize;
    let data = &mmap[..len];

    // Hash the mmap'd slice
    let tgt_hash = hasher::hash_reader(&mut std::io::Cursor::new(data), algorithm, progress)?;
    info!("Target(mmap) {}: {}", algorithm, tgt_hash);

    if src_hash == tgt_hash {
        info!("Verification passed (mmap): hashes match");
        progress.set_phase(OperationPhase::Completed);
        Ok(true)
    } else {
        info!("Verification FAILED (mmap): hash mismatch");
        progress.set_phase(OperationPhase::Failed);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_verify_mmap_matching() {
        let mut src = NamedTempFile::new().unwrap();
        let data = b"Hello, mmap verification!";
        src.write_all(data).unwrap();
        src.flush().unwrap();

        let mut tgt = NamedTempFile::new().unwrap();
        tgt.write_all(data).unwrap();
        tgt.flush().unwrap();

        let progress = Progress::new(data.len() as u64);
        let result = verify_mmap(
            src.path(),
            tgt.path().to_str().unwrap(),
            HashAlgorithm::Sha256,
            &progress,
        )
        .unwrap();
        assert!(result);
    }

    #[test]
    fn test_verify_mmap_mismatch() {
        let mut src = NamedTempFile::new().unwrap();
        src.write_all(b"source data").unwrap();
        src.flush().unwrap();

        let mut tgt = NamedTempFile::new().unwrap();
        tgt.write_all(b"target data").unwrap();
        tgt.flush().unwrap();

        let progress = Progress::new(11);
        let result = verify_mmap(
            src.path(),
            tgt.path().to_str().unwrap(),
            HashAlgorithm::Sha256,
            &progress,
        )
        .unwrap();
        assert!(!result);
    }

    #[test]
    fn test_verify_against_hash_pass() {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(b"test").unwrap();
        f.flush().unwrap();

        let progress = Progress::new(4);
        // SHA-256 of "test"
        let expected = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        let result = verify_against_hash(
            f.path().to_str().unwrap(),
            expected,
            HashAlgorithm::Sha256,
            Some(4),
            &progress,
        )
        .unwrap();
        assert!(result);
    }
}
