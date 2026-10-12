use anyhow::Result;
use ironcrypto::hash::{Sha256, Sha512};
use ironcrypto::prelude::Digest as IcDigest;
use std::io::Read;
use std::path::Path;

use super::progress::Progress;
use super::types::HashAlgorithm;

/// Buffer size for hashing — 4 MiB, shared across all algorithms.
pub(crate) const HASH_BUF_SIZE: usize = 4 * 1024 * 1024;

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
///
/// # FIPS Compliance
/// In FIPS mode, validates the algorithm against FIPS 180-4 / SP 800-131A
/// before proceeding. CRC32 is rejected. Only SHA-256 and SHA-512 remain
/// available as FIPS-approved hashes.
///
/// # Safety Invariant SI-2
/// The output hash is deterministic: calling this function twice with the same
/// reader content and algorithm MUST produce the same hex string.
pub fn hash_reader(
    reader: &mut dyn Read,
    algorithm: HashAlgorithm,
    progress: &Progress,
) -> Result<String> {
    // FIPS 180-4 / SP 800-131A: Validate algorithm is permitted
    super::compliance::validate_hash_algorithm(algorithm).map_err(|e| anyhow::anyhow!(e))?;
    // SI-2: HASH_BUF_SIZE must be positive
    const _: () = assert!(
        HASH_BUF_SIZE > 0,
        "INVARIANT: HASH_BUF_SIZE must be positive"
    );

    let mut buf = vec![0u8; HASH_BUF_SIZE];

    // Create the appropriate hasher behind a trait object so the read loop
    // is written exactly once.
    let mut hasher = create_hasher(algorithm);

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

    let result = hasher.finalize_hex();

    // SI-2: Postcondition — hash output must be valid lowercase hex
    debug_assert!(
        !result.is_empty(),
        "POSTCONDITION VIOLATED: hash output must not be empty"
    );
    debug_assert!(
        result.chars().all(|c| c.is_ascii_hexdigit()),
        "POSTCONDITION VIOLATED: hash output must be valid hex"
    );

    Ok(result)
}

// ── Trait-object hashing abstraction ───────────────────────────────────────────

/// Internal trait that unifies all hash algorithm APIs behind a single interface.
pub(crate) trait DynHasher: Send {
    fn update(&mut self, data: &[u8]);
    fn finalize_hex(self: Box<Self>) -> String;
}

/// Wrapper for an IronCrypto digest (SHA-256, SHA-512).
struct IcDigestHasher<D: IcDigest + Send>(D);

impl<D: IcDigest + Send> DynHasher for IcDigestHasher<D> {
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize_hex(self: Box<Self>) -> String {
        hex::encode(self.0.finalize())
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
        HashAlgorithm::Sha256 => Box::new(IcDigestHasher(Sha256::new())),
        HashAlgorithm::Sha512 => Box::new(IcDigestHasher(Sha512::new())),
        HashAlgorithm::Crc32 => Box::new(Crc32Hasher(crc32fast::Hasher::new())),
    }
}
