// Checksum file parser — parse and validate standard checksum files.
//
// Parses checksum files in common distribution formats:
//   - GNU coreutils format: `<hash>  <filename>` or `<hash> *<filename>`
//   - BSD format: `SHA256 (<filename>) = <hash>`
//   - Simple format: `<hash>` (single hash, no filename)
//
// Supports SHA-256 and SHA-512 hash formats. MD5, SHA-1, and BLAKE3 entries are
// skipped with a warning.
// Used for `abt verify --checksum-file SHA256SUMS` to validate downloads
// against distribution-provided checksum files.
//
// Inspired by Rufus's MD5SUMS parsing in hash.c and Ubuntu/Fedora/Debian
// release checksum file formats.

use anyhow::{Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Detected hash algorithm from file content or filename.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Sha256,
    Sha512,
}

impl HashAlgorithm {
    /// Expected hex string length for this algorithm.
    pub fn hex_len(&self) -> usize {
        match self {
            HashAlgorithm::Sha256 => 64,
            HashAlgorithm::Sha512 => 128,
        }
    }

    /// Detect algorithm from a hex hash string length.
    pub fn from_hex_len(len: usize) -> Option<HashAlgorithm> {
        match len {
            64 => Some(HashAlgorithm::Sha256),
            128 => Some(HashAlgorithm::Sha512),
            _ => None,
        }
    }

    /// Detect algorithm from a checksum filename.
    pub fn from_filename(filename: &str) -> Option<HashAlgorithm> {
        let lower = filename.to_lowercase();
        if lower.contains("sha256") || lower.ends_with(".sha256") {
            Some(HashAlgorithm::Sha256)
        } else if lower.contains("sha512") || lower.ends_with(".sha512") {
            Some(HashAlgorithm::Sha512)
        } else {
            None
        }
    }

    /// Algorithm name string.
    pub fn name(&self) -> &'static str {
        match self {
            HashAlgorithm::Sha256 => "SHA-256",
            HashAlgorithm::Sha512 => "SHA-512",
        }
    }
}

impl std::fmt::Display for HashAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// A single entry from a checksum file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumEntry {
    /// The hash value (lowercase hex).
    pub hash: String,
    /// The filename this hash applies to.
    pub filename: String,
    /// Whether the file was marked as binary (indicated by `*` prefix in GNU format).
    pub binary: bool,
    /// Detected hash algorithm.
    pub algorithm: HashAlgorithm,
}

/// Parsed checksum file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecksumFile {
    /// All entries parsed from the file.
    pub entries: Vec<ChecksumEntry>,
    /// The detected hash algorithm (from filename or content).
    pub algorithm: Option<HashAlgorithm>,
    /// Source filename of the checksum file.
    pub source_file: String,
}

impl ChecksumFile {
    /// Look up a hash by filename (case-insensitive basename match).
    pub fn find_hash(&self, filename: &str) -> Option<&ChecksumEntry> {
        let target = Path::new(filename)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(filename)
            .to_lowercase();

        self.entries.iter().find(|e| {
            let entry_name = Path::new(&e.filename)
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or(&e.filename)
                .to_lowercase();
            entry_name == target
        })
    }

    /// Get all entries as a hash map (filename -> hash).
    pub fn as_map(&self) -> HashMap<String, String> {
        self.entries
            .iter()
            .map(|e| (e.filename.clone(), e.hash.clone()))
            .collect()
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the checksum file is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Parse a checksum file from its content.
///
/// Automatically detects the format (GNU coreutils, BSD, or simple).
///
/// # Arguments
/// * `content` — The text content of the checksum file.
/// * `source_filename` — The name of the checksum file (used for algorithm detection).
///
/// # Returns
/// A `ChecksumFile` with all parsed entries.
pub fn parse_checksum_file(content: &str, source_filename: &str) -> Result<ChecksumFile> {
    let algo_hint = HashAlgorithm::from_filename(source_filename);
    let mut entries = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }

        // Try BSD format: `SHA256 (filename) = hash`
        if let Some(entry) = parse_bsd_line(line) {
            entries.push(entry);
            continue;
        }

        // Try GNU coreutils format: `hash  filename` or `hash *filename`
        if let Some(entry) = parse_gnu_line(line, algo_hint) {
            entries.push(entry);
            continue;
        }

        // Try simple format: just a hash on its own line
        if let Some(entry) = parse_simple_line(line, algo_hint) {
            entries.push(entry);
            continue;
        }

        // Skip unrecognized or unsupported (MD5, SHA-1, BLAKE3) lines
        warn!(
            "Skipping unrecognized or unsupported checksum line (only SHA-256 and SHA-512 are supported): {}",
            line
        );
    }

    let detected_algo = if !entries.is_empty() {
        Some(entries[0].algorithm)
    } else {
        algo_hint
    };

    Ok(ChecksumFile {
        entries,
        algorithm: detected_algo,
        source_file: source_filename.to_string(),
    })
}

/// Parse a BSD-format line: `ALGO (filename) = hash`
fn parse_bsd_line(line: &str) -> Option<ChecksumEntry> {
    // Pattern: "SHA256 (ubuntu.iso) = abcdef..."
    let algo_end = line.find(" (")?;
    let algo_str = &line[..algo_end];
    let paren_start = algo_end + 2;
    let paren_end = line.find(") = ")?;
    let filename = &line[paren_start..paren_end];
    let hash_start = paren_end + 4;
    let hash = line[hash_start..].trim().to_lowercase();

    let algorithm = match algo_str.to_uppercase().as_str() {
        "SHA256" => HashAlgorithm::Sha256,
        "SHA512" => HashAlgorithm::Sha512,
        _ => return None,
    };

    if !is_valid_hex(&hash) || hash.len() != algorithm.hex_len() {
        return None;
    }

    Some(ChecksumEntry {
        hash,
        filename: filename.to_string(),
        binary: false,
        algorithm,
    })
}

/// Parse a GNU coreutils-format line: `hash  filename` or `hash *filename`
fn parse_gnu_line(line: &str, algo_hint: Option<HashAlgorithm>) -> Option<ChecksumEntry> {
    // Find the split between hash and filename.
    // GNU format uses two spaces or space+asterisk.
    let (hash, rest) = if let Some(pos) = line.find("  ") {
        (&line[..pos], line[pos + 2..].trim())
    } else {
        let pos = line.find(" *")?;
        (&line[..pos], line[pos + 2..].trim())
    };

    let hash = hash.trim().to_lowercase();

    if !is_valid_hex(&hash) || rest.is_empty() {
        return None;
    }

    let binary = line.contains(" *");
    let algorithm = detect_entry_algorithm(&hash, algo_hint)?;

    Some(ChecksumEntry {
        hash,
        filename: rest.to_string(),
        binary,
        algorithm,
    })
}

/// Pick the algorithm for a bare hash: the filename hint if it fits the hash length,
/// otherwise the algorithm implied by the length. `None` if neither fits, so a line
/// whose length doesn't match its file's algorithm is skipped rather than misread.
fn detect_entry_algorithm(hash: &str, algo_hint: Option<HashAlgorithm>) -> Option<HashAlgorithm> {
    let algorithm = match algo_hint {
        Some(hint) if hint.hex_len() == hash.len() => hint,
        Some(_) => return None,
        None => HashAlgorithm::from_hex_len(hash.len())?,
    };
    Some(algorithm)
}

/// Parse a simple line that's just a hash value (no filename).
fn parse_simple_line(line: &str, algo_hint: Option<HashAlgorithm>) -> Option<ChecksumEntry> {
    let hash = line.trim().to_lowercase();
    if !is_valid_hex(&hash) {
        return None;
    }

    let algorithm = detect_entry_algorithm(&hash, algo_hint)?;

    Some(ChecksumEntry {
        hash,
        filename: String::new(),
        binary: false,
        algorithm,
    })
}

/// Check if a string is valid hexadecimal.
fn is_valid_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Load and parse a checksum file from disk.
pub fn load_checksum_file(path: &Path) -> Result<ChecksumFile> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read checksum file: {}", path.display()))?;
    let filename = path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("unknown");
    parse_checksum_file(&content, filename)
}

/// Download and parse a checksum file from a URL.
pub async fn fetch_checksum_file(url: &str) -> Result<ChecksumFile> {
    info!("Fetching checksum file: {}", url);

    let client = reqwest::Client::builder()
        .user_agent(format!("abt/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!(
            "Failed to fetch checksum file: HTTP {}",
            resp.status().as_u16()
        );
    }

    let content = resp.text().await?;
    let filename = url.rsplit('/').next().unwrap_or("checksums");
    parse_checksum_file(&content, filename)
}

/// Verify a file against a checksum entry by computing its hash.
pub async fn verify_file_checksum(file_path: &Path, expected: &ChecksumEntry) -> Result<bool> {
    use ironcrypto::hash::{Sha256, Sha512};
    use ironcrypto::prelude::Digest;

    info!(
        "Verifying {} hash of {}...",
        expected.algorithm,
        file_path.display()
    );

    let data = tokio::fs::read(file_path)
        .await
        .with_context(|| format!("Failed to read file: {}", file_path.display()))?;

    let computed = match expected.algorithm {
        HashAlgorithm::Sha256 => {
            let mut hasher = Sha256::new();
            hasher.update(&data);
            hex::encode(hasher.finalize())
        }
        HashAlgorithm::Sha512 => {
            let mut hasher = Sha512::new();
            hasher.update(&data);
            hex::encode(hasher.finalize())
        }
    };

    let matches = computed == expected.hash;
    if matches {
        info!("Checksum OK: {} matches expected hash", file_path.display());
    } else {
        warn!(
            "Checksum MISMATCH for {}: expected {}, got {}",
            file_path.display(),
            expected.hash,
            computed
        );
    }

    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_gnu_format() {
        let content = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  ubuntu-24.04.iso\n\
                        a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2  fedora-40.iso";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        assert_eq!(cf.entries.len(), 2);
        assert_eq!(cf.entries[0].filename, "ubuntu-24.04.iso");
        assert_eq!(cf.entries[0].algorithm, HashAlgorithm::Sha256);
        assert_eq!(cf.entries[1].filename, "fedora-40.iso");
    }

    #[test]
    fn test_parse_gnu_binary_format() {
        let content =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855 *ubuntu.iso";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        assert_eq!(cf.entries.len(), 1);
        assert!(cf.entries[0].binary);
        assert_eq!(cf.entries[0].filename, "ubuntu.iso");
    }

    #[test]
    fn test_parse_bsd_format() {
        let content = "SHA256 (ubuntu-24.04.iso) = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let cf = parse_checksum_file(content, "checksums.txt").unwrap();
        assert_eq!(cf.entries.len(), 1);
        assert_eq!(cf.entries[0].filename, "ubuntu-24.04.iso");
        assert_eq!(cf.entries[0].algorithm, HashAlgorithm::Sha256);
    }

    #[test]
    fn test_parse_bsd_format_md5_is_skipped() {
        let content = "MD5 (file.bin) = d41d8cd98f00b204e9800998ecf8427e";
        let cf = parse_checksum_file(content, "checksums.txt").unwrap();
        assert!(cf.entries.is_empty());
    }

    #[test]
    fn test_parse_simple_format() {
        let content = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let cf = parse_checksum_file(content, "image.iso.sha256").unwrap();
        assert_eq!(cf.entries.len(), 1);
        assert_eq!(cf.entries[0].filename, "");
        assert_eq!(cf.entries[0].algorithm, HashAlgorithm::Sha256);
    }

    #[test]
    fn test_parse_md5sums_is_skipped() {
        let content = "d41d8cd98f00b204e9800998ecf8427e  empty.txt\n\
                        098f6bcd4621d373cade4e832627b4f6  test.txt";
        let cf = parse_checksum_file(content, "MD5SUMS").unwrap();
        assert!(cf.entries.is_empty());
    }

    #[test]
    fn test_parse_rejects_length_mismatch_for_filename_algorithm() {
        // A 40-character (SHA-1-length) hash in a SHA256SUMS file must not be read as SHA-256.
        let content = "da39a3ee5e6b4b0d3255bfef95601890afd80709  file.iso";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        assert!(cf.entries.is_empty());
    }

    #[test]
    fn test_parse_with_comments_and_blanks() {
        let content = "# This is a comment\n\
                        \n\
                        // Another comment\n\
                        e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  file.iso\n\
                        \n";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        assert_eq!(cf.entries.len(), 1);
    }

    #[test]
    fn test_find_hash_by_filename() {
        let content = "abc123def456abc123def456abc123def456abc123def456abc123def456abc123de  image-a.iso\n\
                        e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  image-b.iso";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        let found = cf.find_hash("image-b.iso").unwrap();
        assert_eq!(
            found.hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_find_hash_case_insensitive() {
        let content =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  Ubuntu.ISO";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        assert!(cf.find_hash("ubuntu.iso").is_some());
    }

    #[test]
    fn test_find_hash_with_path() {
        let content =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  ./images/ubuntu.iso";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        assert!(cf.find_hash("ubuntu.iso").is_some());
    }

    #[test]
    fn test_find_hash_not_found() {
        let content =
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  ubuntu.iso";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        assert!(cf.find_hash("fedora.iso").is_none());
    }

    #[test]
    fn test_as_map() {
        let _content = "aaaa  file1.iso\nbbbb  file2.iso";
        // These are not valid hex for known algos but we can use short hashes
        let content = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  file1.iso\n\
                        a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2  file2.iso";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        let map = cf.as_map();
        assert_eq!(map.len(), 2);
        assert!(map.contains_key("file1.iso"));
        assert!(map.contains_key("file2.iso"));
    }

    #[test]
    fn test_algorithm_from_filename() {
        assert_eq!(
            HashAlgorithm::from_filename("SHA256SUMS"),
            Some(HashAlgorithm::Sha256)
        );
        assert_eq!(
            HashAlgorithm::from_filename("sha512sums.txt"),
            Some(HashAlgorithm::Sha512)
        );
        assert_eq!(HashAlgorithm::from_filename("MD5SUMS"), None);
        assert_eq!(HashAlgorithm::from_filename("image.iso.sha1"), None);
        assert_eq!(HashAlgorithm::from_filename("file.b3"), None);
        assert_eq!(HashAlgorithm::from_filename("random.txt"), None);
    }

    #[test]
    fn test_algorithm_hex_len() {
        assert_eq!(HashAlgorithm::Sha256.hex_len(), 64);
        assert_eq!(HashAlgorithm::Sha512.hex_len(), 128);
    }

    #[test]
    fn test_algorithm_from_hex_len() {
        assert_eq!(HashAlgorithm::from_hex_len(32), None);
        assert_eq!(HashAlgorithm::from_hex_len(40), None);
        assert_eq!(HashAlgorithm::from_hex_len(64), Some(HashAlgorithm::Sha256));
        assert_eq!(
            HashAlgorithm::from_hex_len(128),
            Some(HashAlgorithm::Sha512)
        );
        assert_eq!(HashAlgorithm::from_hex_len(99), None);
    }

    #[test]
    fn test_is_valid_hex() {
        assert!(is_valid_hex("abcdef0123456789"));
        assert!(is_valid_hex("ABCDEF"));
        assert!(!is_valid_hex(""));
        assert!(!is_valid_hex("xyz"));
        assert!(!is_valid_hex("abc xyz"));
    }

    #[test]
    fn test_checksum_file_len_is_empty() {
        let cf = ChecksumFile {
            entries: vec![],
            algorithm: None,
            source_file: "test".into(),
        };
        assert_eq!(cf.len(), 0);
        assert!(cf.is_empty());
    }

    #[test]
    fn test_hash_algorithm_display() {
        assert_eq!(format!("{}", HashAlgorithm::Sha256), "SHA-256");
        assert_eq!(format!("{}", HashAlgorithm::Sha512), "SHA-512");
    }

    #[test]
    fn test_checksum_entry_serde() {
        let entry = ChecksumEntry {
            hash: "abc123".into(),
            filename: "test.iso".into(),
            binary: true,
            algorithm: HashAlgorithm::Sha256,
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: ChecksumEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.hash, "abc123");
        assert!(parsed.binary);
        assert_eq!(parsed.algorithm, HashAlgorithm::Sha256);
    }

    #[test]
    fn test_parse_sha512_file() {
        // SHA-512 of "hello" (128 hex characters).
        let hash_512 = "9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043";
        let content = format!("{}  large-file.bin", hash_512);
        let cf = parse_checksum_file(&content, "SHA512SUMS").unwrap();
        assert_eq!(cf.entries.len(), 1);
        assert_eq!(cf.entries[0].algorithm, HashAlgorithm::Sha512);
    }

    #[test]
    fn test_parse_mixed_invalid_lines() {
        let content = "this is not a checksum\n\
                        e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  valid.iso\n\
                        also not valid $$$ ###";
        let cf = parse_checksum_file(content, "SHA256SUMS").unwrap();
        assert_eq!(cf.entries.len(), 1);
        assert_eq!(cf.entries[0].filename, "valid.iso");
    }
}
