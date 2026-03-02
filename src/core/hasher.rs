use anyhow::Result;
use sha2::{digest, Digest, Sha256, Sha512};
use std::io::Read;
use std::path::Path;

use super::progress::Progress;
use super::types::HashAlgorithm;

/// Buffer size for hashing — 4 MiB, shared across all algorithms.
const HASH_BUF_SIZE: usize = 4 * 1024 * 1024;

/// Compute a checksum/hash of a file.
pub fn hash_file(path: &Path, algorithm: HashAlgorithm, progress: &Progress) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    progress.set_total(metadata.len());

    let mut reader = std::io::BufReader::with_capacity(HASH_BUF_SIZE, file);
    hash_reader(&mut reader, algorithm, progress)
}

/// Compute hash from a reader. Uses a single shared buffer and a unified
/// read loop regardless of algorithm, eliminating the previous per-algorithm
/// code duplication.
pub fn hash_reader(
    reader: &mut dyn Read,
    algorithm: HashAlgorithm,
    progress: &Progress,
) -> Result<String> {
    let mut buf = vec![0u8; HASH_BUF_SIZE];

    // Create the appropriate hasher behind a trait object so the read loop
    // is written exactly once.
    let mut hasher: Box<dyn DynHasher> = match algorithm {
        HashAlgorithm::Md5 => Box::new(DigestHasher(md5::Md5::new())),
        HashAlgorithm::Sha1 => Box::new(DigestHasher(sha1::Sha1::new())),
        HashAlgorithm::Sha256 => Box::new(DigestHasher(Sha256::new())),
        HashAlgorithm::Sha512 => Box::new(DigestHasher(Sha512::new())),
        HashAlgorithm::Blake3 => Box::new(Blake3Hasher(blake3::Hasher::new())),
        HashAlgorithm::Crc32 => Box::new(Crc32Hasher(crc32fast::Hasher::new())),
    };

    loop {
        if progress.is_cancelled() {
            anyhow::bail!("Hash computation cancelled");
        }
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        progress.add_bytes(n as u64);
    }

    Ok(hasher.finalize_hex())
}

// ── Trait-object hashing abstraction ───────────────────────────────────────────

/// Internal trait that unifies all hash algorithm APIs behind a single interface.
pub(crate) trait DynHasher: Send {
    fn update(&mut self, data: &[u8]);
    fn finalize_hex(self: Box<Self>) -> String;
}

/// Wrapper for any `sha2::Digest`-compatible hasher (MD5, SHA-1, SHA-256, SHA-512).
struct DigestHasher<D: Digest + Send>(D);

impl<D: Digest + Send> DynHasher for DigestHasher<D>
where
    digest::Output<D>: std::fmt::LowerHex,
{
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize_hex(self: Box<Self>) -> String {
        format!("{:x}", self.0.finalize())
    }
}

/// Wrapper for BLAKE3 (different API than `Digest`).
struct Blake3Hasher(blake3::Hasher);

impl DynHasher for Blake3Hasher {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize_hex(self: Box<Self>) -> String {
        self.0.finalize().to_hex().to_string()
    }
}

/// Wrapper for CRC32 (u32 result, zero-padded to 8 hex chars).
struct Crc32Hasher(crc32fast::Hasher);

impl DynHasher for Crc32Hasher {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize_hex(self: Box<Self>) -> String {
        format!("{:08x}", self.0.finalize())
    }
}

/// Create a hasher for the given algorithm.
pub(crate) fn create_hasher(algorithm: HashAlgorithm) -> Box<dyn DynHasher> {
    match algorithm {
        HashAlgorithm::Md5 => Box::new(DigestHasher(md5::Md5::new())),
        HashAlgorithm::Sha1 => Box::new(DigestHasher(sha1::Sha1::new())),
        HashAlgorithm::Sha256 => Box::new(DigestHasher(Sha256::new())),
        HashAlgorithm::Sha512 => Box::new(DigestHasher(Sha512::new())),
        HashAlgorithm::Blake3 => Box::new(Blake3Hasher(blake3::Hasher::new())),
        HashAlgorithm::Crc32 => Box::new(Crc32Hasher(crc32fast::Hasher::new())),
    }
}
