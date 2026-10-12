// Signed download verification — RSA signature validation for downloaded artifacts.
//
// Inspired by Rufus's DownloadSignedFile() / ValidateOpensslSignature(): downloads
// a file AND its detached .sig signature, then verifies the RSA PKCS#1v1.5
// signature before accepting the file. Prevents supply-chain attacks on:
//   - Self-update binaries
//   - Catalog/metadata files
//   - Bootloader binaries (UEFI:NTFS, etc.)
//
// Uses SHA-256 as the digest algorithm. Public keys can be embedded at compile
// time or loaded from a key ring file.

#![allow(dead_code)]

use anyhow::{Context, Result};
use ironcrypto::hash::Sha256;
use ironcrypto::pkix::PublicKeyInfo;
use ironcrypto::prelude::Digest;
use ironcrypto::rsa::RsaPublicKey;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A public key for verification, stored as PEM-encoded RSA key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationKey {
    /// Key identifier (e.g., "abt-release-2026")
    pub key_id: String,
    /// PEM-encoded RSA public key (PKCS#8 SubjectPublicKeyInfo)
    pub pem: String,
    /// Optional: key expiry date (ISO 8601)
    pub expires: Option<String>,
    /// Whether this key is trusted for self-update verification
    pub trusted_for_updates: bool,
}

/// A key ring containing multiple verification keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeyRing {
    pub keys: Vec<VerificationKey>,
}

impl KeyRing {
    /// Create a new empty key ring.
    pub fn new() -> Self {
        Self { keys: Vec::new() }
    }

    /// Add a key to the ring.
    pub fn add_key(&mut self, key: VerificationKey) {
        self.keys.push(key);
    }

    /// Find a key by ID.
    pub fn find_key(&self, key_id: &str) -> Option<&VerificationKey> {
        self.keys.iter().find(|k| k.key_id == key_id)
    }

    /// Get all keys trusted for updates.
    pub fn update_keys(&self) -> Vec<&VerificationKey> {
        self.keys.iter().filter(|k| k.trusted_for_updates).collect()
    }

    /// Load a key ring from a JSON file.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).context("Failed to read key ring file")?;
        serde_json::from_str(&content).context("Failed to parse key ring JSON")
    }

    /// Save the key ring to a JSON file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }
}

/// Signature verification result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureVerification {
    /// Whether the signature is valid.
    pub valid: bool,
    /// SHA-256 hash of the verified file.
    pub file_hash: String,
    /// Key ID used for verification (if known).
    pub key_id: Option<String>,
    /// Signature file path.
    pub signature_path: String,
    /// Error message if verification failed.
    pub error: Option<String>,
}

/// Compute SHA-256 hash of a file.
pub fn hash_file_sha256(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open file: {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_vec())
}

/// Compute SHA-256 hash of a byte slice.
pub fn hash_bytes_sha256(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

/// Verify an RSA PKCS#1v1.5 signature over a SHA-256 digest.
///
/// `data_hash` is the SHA-256 digest of the signed data. `public_key_pem` is an
/// RSA SubjectPublicKeyInfo PEM. Returns `Ok(true)` only when the signature
/// recovers to the expected `DigestInfo || data_hash` encoding, `Ok(false)` when
/// it does not, and `Err` when the key or the digest is malformed.
pub fn verify_rsa_sha256_signature(
    data_hash: &[u8],
    signature: &[u8],
    public_key_pem: &str,
) -> Result<bool> {
    if data_hash.len() != SHA256_LEN {
        anyhow::bail!(
            "SHA-256 digest must be {} bytes, got {}",
            SHA256_LEN,
            data_hash.len()
        );
    }

    let der = decode_pem_public_key(public_key_pem)?;
    let (modulus, exponent) = match PublicKeyInfo::from_der(&der) {
        Ok(PublicKeyInfo::Rsa { modulus, exponent }) => (modulus, exponent),
        Ok(_) => anyhow::bail!("Public key is not an RSA key"),
        Err(e) => anyhow::bail!("Malformed public key: {}", e),
    };
    let key = RsaPublicKey::from_components(modulus, exponent)
        .map_err(|e| anyhow::anyhow!("Unusable RSA public key: {}", e))?;

    let key_size = key.size();
    if signature.len() != key_size {
        debug!(
            "Signature size mismatch: expected {} bytes, got {}",
            key_size,
            signature.len()
        );
        return Ok(false);
    }

    // RSAVP1 (RFC 8017 §5.2.2): recover the encoded message as s^e mod n.
    let mut em = vec![0u8; key_size];
    if key.raw_public(signature, &mut em).is_err() {
        // s >= n has no valid representative, so it cannot be a signature.
        return Ok(false);
    }

    // EMSA-PKCS1-v1_5 (RFC 8017 §9.2): 0x00 0x01 PS 0x00 T, with PS at least
    // 8 bytes of 0xFF and T = DigestInfo(SHA-256) || digest.
    let t_len = SHA256_DIGEST_INFO.len() + SHA256_LEN;
    if key_size < t_len + 11 {
        anyhow::bail!("RSA key too small for a SHA-256 PKCS#1 v1.5 signature");
    }
    let mut expected = Vec::with_capacity(key_size);
    expected.extend_from_slice(&[0x00, 0x01]);
    expected.resize(key_size - t_len - 1, 0xFF);
    expected.push(0x00);
    expected.extend_from_slice(&SHA256_DIGEST_INFO);
    expected.extend_from_slice(data_hash);

    debug!(
        "Signature verification: key_size={}, sig_len={}",
        key_size,
        signature.len()
    );

    // Public-key operation on public data, so a plain comparison is fine.
    Ok(em == expected)
}

/// SHA-256 digest length in bytes.
const SHA256_LEN: usize = 32;

/// DER `DigestInfo` prefix for SHA-256 (RFC 8017 §9.2, Note 1).
const SHA256_DIGEST_INFO: [u8; 19] = [
    0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01, 0x05,
    0x00, 0x04, 0x20,
];

/// Decode a PEM-encoded public key to DER bytes.
pub fn decode_pem_public_key(pem: &str) -> Result<Vec<u8>> {
    let pem = pem.trim();
    let b64: String = pem
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect::<Vec<&str>>()
        .join("");

    if b64.is_empty() {
        anyhow::bail!("Empty PEM public key");
    }

    // Simple base64 decode
    base64_decode(&b64).context("Failed to decode PEM base64")
}

/// Minimal base64 decoder (no external dependency needed).
fn base64_decode(input: &str) -> Result<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let input: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'\n' && b != b'\r' && b != b' ')
        .collect();
    let mut output = Vec::with_capacity(input.len() * 3 / 4);

    for chunk in input.chunks(4) {
        let mut buf = [0u8; 4];
        let mut count = 0;
        for (i, &byte) in chunk.iter().enumerate() {
            if byte == b'=' {
                break;
            }
            buf[i] = TABLE
                .iter()
                .position(|&c| c == byte)
                .ok_or_else(|| anyhow::anyhow!("Invalid base64 character: {}", byte as char))?
                as u8;
            count = i + 1;
        }
        if count >= 2 {
            output.push((buf[0] << 2) | (buf[1] >> 4));
        }
        if count >= 3 {
            output.push((buf[1] << 4) | (buf[2] >> 2));
        }
        if count >= 4 {
            output.push((buf[2] << 6) | buf[3]);
        }
    }

    Ok(output)
}

/// Download a file and its detached signature, then verify.
///
/// Convention: signature file is at `{url}.sig` (matching Rufus).
/// Falls back to `{url}.asc` if `.sig` is not found.
pub async fn download_and_verify(
    url: &str,
    output_dir: &Path,
    key_ring: &KeyRing,
) -> Result<SignedDownloadResult> {
    info!("Downloading with signature verification: {}", url);

    let client = reqwest::Client::builder()
        .user_agent(format!("abt/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(3600))
        .build()?;

    // Download the main file
    let filename = url.rsplit('/').next().unwrap_or("download");
    let file_path = output_dir.join(filename);

    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} downloading {}", resp.status().as_u16(), url);
    }
    let file_bytes = resp.bytes().await?;
    std::fs::write(&file_path, &file_bytes)?;

    // Try to download signature (.sig first, then .asc)
    let sig_result = download_signature(&client, url).await;

    match sig_result {
        Ok((sig_bytes, sig_url)) => {
            info!("Found signature: {}", sig_url);
            let sig_path = output_dir.join(format!("{}.sig", filename));
            std::fs::write(&sig_path, &sig_bytes)?;

            // Compute file hash
            let file_hash = hash_bytes_sha256(&file_bytes);
            let hash_hex = hex::encode(&file_hash);

            // Try each key in the ring
            let mut verified = false;
            let mut used_key_id = None;
            for key in &key_ring.keys {
                match verify_rsa_sha256_signature(&file_hash, &sig_bytes, &key.pem) {
                    Ok(true) => {
                        info!("Signature verified with key: {}", key.key_id);
                        verified = true;
                        used_key_id = Some(key.key_id.clone());
                        break;
                    }
                    Ok(false) => {
                        debug!("Signature does not match key: {}", key.key_id);
                    }
                    Err(e) => {
                        warn!("Error verifying with key {}: {}", key.key_id, e);
                    }
                }
            }

            Ok(SignedDownloadResult {
                path: file_path,
                signature_path: Some(sig_path),
                file_hash: hash_hex,
                signature_valid: verified,
                key_id: used_key_id,
                signature_available: true,
            })
        }
        Err(_) => {
            warn!("No signature file found for {}", url);
            let file_hash = hex::encode(hash_bytes_sha256(&file_bytes));
            Ok(SignedDownloadResult {
                path: file_path,
                signature_path: None,
                file_hash,
                signature_valid: false,
                key_id: None,
                signature_available: false,
            })
        }
    }
}

/// Try to download a detached signature file.
async fn download_signature(client: &reqwest::Client, file_url: &str) -> Result<(Vec<u8>, String)> {
    // Try .sig first
    let sig_url = format!("{}.sig", file_url);
    if let Ok(resp) = client.get(&sig_url).send().await {
        if resp.status().is_success() {
            let bytes = resp.bytes().await?;
            return Ok((bytes.to_vec(), sig_url));
        }
    }

    // Try .asc
    let asc_url = format!("{}.asc", file_url);
    if let Ok(resp) = client.get(&asc_url).send().await {
        if resp.status().is_success() {
            let bytes = resp.bytes().await?;
            return Ok((bytes.to_vec(), asc_url));
        }
    }

    anyhow::bail!("No signature file found (.sig or .asc)")
}

/// Result of a signed download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedDownloadResult {
    /// Path to the downloaded file.
    pub path: PathBuf,
    /// Path to the signature file (if found).
    pub signature_path: Option<PathBuf>,
    /// SHA-256 hash of the downloaded file.
    pub file_hash: String,
    /// Whether the signature was valid.
    pub signature_valid: bool,
    /// Key ID used for successful verification.
    pub key_id: Option<String>,
    /// Whether a signature file was available.
    pub signature_available: bool,
}

/// Verify a local file against a local signature file.
pub fn verify_local_file(
    file_path: &Path,
    sig_path: &Path,
    key_ring: &KeyRing,
) -> Result<SignatureVerification> {
    let file_hash = hash_file_sha256(file_path)?;
    let hash_hex = hex::encode(&file_hash);
    let sig_bytes = std::fs::read(sig_path)
        .with_context(|| format!("Failed to read signature: {}", sig_path.display()))?;

    for key in &key_ring.keys {
        match verify_rsa_sha256_signature(&file_hash, &sig_bytes, &key.pem) {
            Ok(true) => {
                return Ok(SignatureVerification {
                    valid: true,
                    file_hash: hash_hex,
                    key_id: Some(key.key_id.clone()),
                    signature_path: sig_path.display().to_string(),
                    error: None,
                });
            }
            Ok(false) => continue,
            Err(e) => {
                debug!("Key {} error: {}", key.key_id, e);
                continue;
            }
        }
    }

    Ok(SignatureVerification {
        valid: false,
        file_hash: hash_hex,
        key_id: None,
        signature_path: sig_path.display().to_string(),
        error: Some("No matching key found in key ring".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_ring_new() {
        let kr = KeyRing::new();
        assert!(kr.keys.is_empty());
    }

    #[test]
    fn test_key_ring_add_and_find() {
        let mut kr = KeyRing::new();
        kr.add_key(VerificationKey {
            key_id: "test-key".into(),
            pem: "-----BEGIN PUBLIC KEY-----\nMIIBIjAN...\n-----END PUBLIC KEY-----".into(),
            expires: None,
            trusted_for_updates: true,
        });
        assert_eq!(kr.keys.len(), 1);
        assert!(kr.find_key("test-key").is_some());
        assert!(kr.find_key("nonexistent").is_none());
    }

    #[test]
    fn test_key_ring_update_keys() {
        let mut kr = KeyRing::new();
        kr.add_key(VerificationKey {
            key_id: "update-key".into(),
            pem: String::new(),
            expires: None,
            trusted_for_updates: true,
        });
        kr.add_key(VerificationKey {
            key_id: "catalog-key".into(),
            pem: String::new(),
            expires: None,
            trusted_for_updates: false,
        });
        assert_eq!(kr.update_keys().len(), 1);
        assert_eq!(kr.update_keys()[0].key_id, "update-key");
    }

    #[test]
    fn test_key_ring_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keyring.json");
        let mut kr = KeyRing::new();
        kr.add_key(VerificationKey {
            key_id: "roundtrip".into(),
            pem: "test-pem".into(),
            expires: Some("2027-01-01".into()),
            trusted_for_updates: false,
        });
        kr.save(&path).unwrap();
        let loaded = KeyRing::load(&path).unwrap();
        assert_eq!(loaded.keys.len(), 1);
        assert_eq!(loaded.keys[0].key_id, "roundtrip");
        assert_eq!(loaded.keys[0].expires.as_deref(), Some("2027-01-01"));
    }

    #[test]
    fn test_hash_bytes_sha256() {
        let hash = hash_bytes_sha256(b"hello world");
        let hex_str = hex::encode(&hash);
        assert_eq!(
            hex_str,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_hash_bytes_sha256_empty() {
        let hash = hash_bytes_sha256(b"");
        let hex_str = hex::encode(&hash);
        assert_eq!(
            hex_str,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_hash_file_sha256() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bin");
        std::fs::write(&path, b"test data").unwrap();
        let hash = hash_file_sha256(&path).unwrap();
        let hex_str = hex::encode(&hash);
        // SHA-256 of "test data"
        assert_eq!(hex_str.len(), 64);
    }

    #[test]
    fn test_base64_decode_simple() {
        let decoded = base64_decode("SGVsbG8=").unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn test_base64_decode_no_padding() {
        let decoded = base64_decode("SGVsbG8gV29ybGQ").unwrap();
        assert_eq!(decoded, b"Hello World");
    }

    #[test]
    fn test_base64_decode_with_newlines() {
        let decoded = base64_decode("SGVs\nbG8=").unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn test_base64_decode_invalid() {
        assert!(base64_decode("!!!").is_err());
    }

    #[test]
    fn test_decode_pem_public_key() {
        let pem = "-----BEGIN PUBLIC KEY-----\nMIIBIjAN\n-----END PUBLIC KEY-----";
        let der = decode_pem_public_key(pem).unwrap();
        assert!(!der.is_empty());
    }

    #[test]
    fn test_decode_pem_empty() {
        let pem = "-----BEGIN PUBLIC KEY-----\n-----END PUBLIC KEY-----";
        assert!(decode_pem_public_key(pem).is_err());
    }

    /// RSA-2048 SubjectPublicKeyInfo for the signature vector below.
    const TEST_PUB_PEM: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA3qUiCYxQW6nWYBavmvEs
DlA9EUTE31ogVLxF674QKYX02zGAckC8DHqLO5GojWhY0INjzbCprHVRvO6Osaok
jx5w5DGE7fbI0LvsQvN8gmLrKlMaXiTb6BvvMIpXw+ZjdfgYIq8vyl6Wdv/mEPqU
mKZslhM9TO4Fy1dRUIPDLaNAYWnXmvHtphVf0B5KtoXFtixXWu8qEkuRnOe71IPj
qdfsjV5qx+4ceiqDmcgbNZ366HXPuon5ca0664Y91+AKlkQE5nIzjtAHBOcK0OF3
qyClxrv/tYf+9wrLGM2q+5SAN2pVdlcdOKCNnrOitm9zw8INpVzO5pL7Iy8SCT/w
NwIDAQAB
-----END PUBLIC KEY-----";

    /// Message signed with OpenSSL (`openssl dgst -sha256 -sign`), so these tests
    /// check against an independent implementation rather than IronCrypto itself.
    const TEST_MSG: &[u8] = b"abt signature test vector\n";

    /// PKCS#1 v1.5 SHA-256 signature of `TEST_MSG` under `TEST_PUB_PEM`.
    const TEST_SIG_HEX: &str = "93adbd821bdc06fc46879a3b7574b430cb0c30a98f96a03213df149c3c4d62911470909d36ef33e9ca82e173f6593c954dc5eb809b422312bd4937c1fb02e74477fe31a833b3dc771db36fd00e01c06cc8cb328e94962e09983a9a89ffc63c14aa17d5a604986b52a6710e3f4558f505dcbf75987394cbb48990be4de9247fce3b87f286c50efb3c1b4dc10d1281c1df8b218f27447693abbfcecfc2fc2f53fb0a04a04f717ec05badc57455c355422f26b189c37128d70fec03e8d0db62a982c39c86e2066d85cfe4b463e2f4f30c69775ef06ab557120c9d5fcbc988eda242d5b2607a93d44f2c6f0185569d404c350d8915856c3c93801df76485e170429f";

    #[test]
    fn test_verify_rsa_accepts_openssl_signature() {
        let sig = hex::decode(TEST_SIG_HEX).unwrap();
        let digest = hash_bytes_sha256(TEST_MSG);
        assert!(verify_rsa_sha256_signature(&digest, &sig, TEST_PUB_PEM).unwrap());
    }

    #[test]
    fn test_verify_rsa_rejects_other_message() {
        let sig = hex::decode(TEST_SIG_HEX).unwrap();
        let digest = hash_bytes_sha256(b"abt signature test vector!\n");
        assert!(!verify_rsa_sha256_signature(&digest, &sig, TEST_PUB_PEM).unwrap());
    }

    #[test]
    fn test_verify_rsa_rejects_tampered_signature() {
        let mut sig = hex::decode(TEST_SIG_HEX).unwrap();
        sig[100] ^= 0x01;
        let digest = hash_bytes_sha256(TEST_MSG);
        assert!(!verify_rsa_sha256_signature(&digest, &sig, TEST_PUB_PEM).unwrap());
    }

    #[test]
    fn test_verify_rsa_rejects_wrong_length_signature() {
        let sig = hex::decode(TEST_SIG_HEX).unwrap();
        let digest = hash_bytes_sha256(TEST_MSG);
        assert!(!verify_rsa_sha256_signature(&digest, &sig[..255], TEST_PUB_PEM).unwrap());
    }

    #[test]
    fn test_verify_rsa_rejects_non_sha256_digest() {
        let sig = hex::decode(TEST_SIG_HEX).unwrap();
        assert!(verify_rsa_sha256_signature(&[0u8; 20], &sig, TEST_PUB_PEM).is_err());
    }

    #[test]
    fn test_verify_rsa_rejects_malformed_key() {
        let sig = hex::decode(TEST_SIG_HEX).unwrap();
        let digest = hash_bytes_sha256(TEST_MSG);
        let pem = "-----BEGIN PUBLIC KEY-----\nAAAA\n-----END PUBLIC KEY-----";
        assert!(verify_rsa_sha256_signature(&digest, &sig, pem).is_err());
    }

    #[test]
    fn test_signed_download_result_serde() {
        let result = SignedDownloadResult {
            path: PathBuf::from("/tmp/test.iso"),
            signature_path: Some(PathBuf::from("/tmp/test.iso.sig")),
            file_hash: "abc123".into(),
            signature_valid: true,
            key_id: Some("key-1".into()),
            signature_available: true,
        };
        let json = serde_json::to_string(&result).unwrap();
        let deser: SignedDownloadResult = serde_json::from_str(&json).unwrap();
        assert!(deser.signature_valid);
        assert_eq!(deser.key_id.as_deref(), Some("key-1"));
    }

    #[test]
    fn test_verify_local_file_no_keys() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("data.bin");
        let sig_path = dir.path().join("data.bin.sig");
        std::fs::write(&file_path, b"test data").unwrap();
        std::fs::write(&sig_path, b"fake signature").unwrap();

        let kr = KeyRing::new();
        let result = verify_local_file(&file_path, &sig_path, &kr).unwrap();
        assert!(!result.valid);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_signature_verification_serde() {
        let sv = SignatureVerification {
            valid: false,
            file_hash: "deadbeef".into(),
            key_id: None,
            signature_path: "/tmp/test.sig".into(),
            error: Some("No key".into()),
        };
        let json = serde_json::to_string(&sv).unwrap();
        assert!(json.contains("deadbeef"));
    }
}
