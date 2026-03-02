// Image cache — local download cache with SHA-256 verification and reuse.
// Avoids re-downloading images that have already been fetched.
// Inspired by rpi-imager's CacheManager with background verification.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Metadata about a cached image file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Original download URL.
    pub url: String,
    /// Local file path within the cache directory.
    pub local_path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// SHA-256 hash of the file contents.
    pub sha256: String,
    /// When the file was cached (seconds since UNIX epoch).
    pub cached_at: u64,
    /// When the file was last accessed.
    pub last_accessed: u64,
    /// Optional ETag from the HTTP response for conditional requests.
    pub etag: Option<String>,
    /// Whether the hash has been verified after caching.
    pub verified: bool,
}

/// Cache statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_size_bytes: u64,
    pub verified_entries: usize,
    pub unverified_entries: usize,
    pub oldest_entry_age_secs: Option<u64>,
}

/// Policy for cache eviction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EvictionPolicy {
    /// Remove entries older than N seconds.
    MaxAge(u64),
    /// Keep at most N entries (evict oldest first).
    MaxEntries(usize),
    /// Keep total size under N bytes (evict oldest first).
    MaxSize(u64),
}

impl fmt::Display for EvictionPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MaxAge(s) => write!(f, "max-age={}s", s),
            Self::MaxEntries(n) => write!(f, "max-entries={}", n),
            Self::MaxSize(b) => write!(f, "max-size={}B", b),
        }
    }
}

/// Cache manifest (stored as JSON in the cache directory).
#[derive(Debug, Serialize, Deserialize)]
pub struct CacheManifest {
    pub version: u32,
    pub entries: HashMap<String, CacheEntry>,
}

impl Default for CacheManifest {
    fn default() -> Self {
        Self {
            version: 1,
            entries: HashMap::new(),
        }
    }
}

/// Image download cache manager.
pub struct ImageCache {
    cache_dir: PathBuf,
    manifest: CacheManifest,
    max_size: u64,
}

impl ImageCache {
    /// Create or open an image cache at the given directory.
    pub fn open(cache_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(cache_dir)?;
        let manifest_path = cache_dir.join("manifest.json");
        let manifest = if manifest_path.exists() {
            let data = std::fs::read_to_string(&manifest_path)?;
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            CacheManifest::default()
        };

        Ok(Self {
            cache_dir: cache_dir.to_path_buf(),
            manifest,
            max_size: 10 * 1024 * 1024 * 1024, // 10 GiB default
        })
    }

    /// Open the default cache directory (~/.cache/abt/images).
    pub fn open_default() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from(".cache"))
            .join("abt")
            .join("images");
        Self::open(&cache_dir)
    }

    /// Set maximum cache size in bytes.
    pub fn set_max_size(&mut self, max_bytes: u64) {
        self.max_size = max_bytes;
    }

    /// Check if a URL is cached and the cached file is valid.
    pub fn lookup(&self, url: &str) -> Option<&CacheEntry> {
        self.manifest.entries.get(url).and_then(|entry| {
            if entry.local_path.exists() {
                Some(entry)
            } else {
                None
            }
        })
    }

    /// Get the local path for a cached URL, if available and verified.
    pub fn get_path(&self, url: &str) -> Option<PathBuf> {
        self.lookup(url).and_then(|entry| {
            if entry.verified && entry.local_path.exists() {
                Some(entry.local_path.clone())
            } else {
                None
            }
        })
    }

    /// Register a downloaded file in the cache.
    pub fn insert(&mut self, url: &str, local_path: &Path, sha256: &str, etag: Option<&str>) -> Result<()> {
        let size = std::fs::metadata(local_path)?.len();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Copy file into cache directory with hash-based name
        let ext = local_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("img");
        let hash_prefix = &sha256[..std::cmp::min(16, sha256.len())];
        let cache_filename = format!("{}.{}", hash_prefix, ext);
        let cache_path = self.cache_dir.join(&cache_filename);

        if !cache_path.exists() {
            std::fs::copy(local_path, &cache_path)?;
        }

        let entry = CacheEntry {
            url: url.to_string(),
            local_path: cache_path,
            size,
            sha256: sha256.to_string(),
            cached_at: now,
            last_accessed: now,
            etag: etag.map(|s| s.to_string()),
            verified: false,
        };

        self.manifest.entries.insert(url.to_string(), entry);
        self.save_manifest()?;
        Ok(())
    }

    /// Verify a cached entry by re-computing its SHA-256 hash.
    pub fn verify(&mut self, url: &str) -> Result<bool> {
        let entry = self
            .manifest
            .entries
            .get(url)
            .ok_or_else(|| anyhow!("URL not in cache: {}", url))?
            .clone();

        if !entry.local_path.exists() {
            self.manifest.entries.remove(url);
            self.save_manifest()?;
            return Ok(false);
        }

        let computed = hash_file_sha256(&entry.local_path)?;
        let valid = computed == entry.sha256;

        if let Some(e) = self.manifest.entries.get_mut(url) {
            e.verified = valid;
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            e.last_accessed = now;
        }

        self.save_manifest()?;
        Ok(valid)
    }

    /// Remove a cached entry.
    pub fn remove(&mut self, url: &str) -> Result<()> {
        if let Some(entry) = self.manifest.entries.remove(url) {
            if entry.local_path.exists() {
                std::fs::remove_file(&entry.local_path)?;
            }
        }
        self.save_manifest()?;
        Ok(())
    }

    /// Remove all cached entries.
    pub fn clear(&mut self) -> Result<()> {
        for entry in self.manifest.entries.values() {
            if entry.local_path.exists() {
                let _ = std::fs::remove_file(&entry.local_path);
            }
        }
        self.manifest.entries.clear();
        self.save_manifest()?;
        Ok(())
    }

    /// Apply an eviction policy.
    pub fn evict(&mut self, policy: &EvictionPolicy) -> Result<usize> {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut to_remove = Vec::new();

        match policy {
            EvictionPolicy::MaxAge(max_age) => {
                for (url, entry) in &self.manifest.entries {
                    if now.saturating_sub(entry.last_accessed) > *max_age {
                        to_remove.push(url.clone());
                    }
                }
            }
            EvictionPolicy::MaxEntries(max) => {
                let mut entries: Vec<_> = self.manifest.entries.iter().collect();
                entries.sort_by_key(|(_, e)| e.last_accessed);
                let excess = entries.len().saturating_sub(*max);
                for (url, _) in entries.iter().take(excess) {
                    to_remove.push(url.to_string());
                }
            }
            EvictionPolicy::MaxSize(max_bytes) => {
                let mut entries: Vec<_> = self.manifest.entries.iter().collect();
                entries.sort_by_key(|(_, e)| e.last_accessed);
                let mut total: u64 = entries.iter().map(|(_, e)| e.size).sum();
                for (url, entry) in &entries {
                    if total <= *max_bytes {
                        break;
                    }
                    total = total.saturating_sub(entry.size);
                    to_remove.push(url.to_string());
                }
            }
        }

        let count = to_remove.len();
        for url in to_remove {
            self.remove(&url)?;
        }
        Ok(count)
    }

    /// Get cache statistics.
    pub fn stats(&self) -> CacheStats {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let total_size_bytes: u64 = self.manifest.entries.values().map(|e| e.size).sum();
        let verified_entries = self.manifest.entries.values().filter(|e| e.verified).count();
        let oldest = self
            .manifest
            .entries
            .values()
            .map(|e| now.saturating_sub(e.cached_at))
            .max();

        CacheStats {
            total_entries: self.manifest.entries.len(),
            total_size_bytes,
            verified_entries,
            unverified_entries: self.manifest.entries.len() - verified_entries,
            oldest_entry_age_secs: oldest,
        }
    }

    /// List all cached entries.
    pub fn list(&self) -> Vec<&CacheEntry> {
        self.manifest.entries.values().collect()
    }

    /// Save the manifest to disk.
    fn save_manifest(&self) -> Result<()> {
        let path = self.cache_dir.join("manifest.json");
        let json = serde_json::to_string_pretty(&self.manifest)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Get the cache directory path.
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

/// Compute SHA-256 hash of a file.
pub fn hash_file_sha256(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache() -> (tempfile::TempDir, ImageCache) {
        let dir = tempfile::tempdir().unwrap();
        let cache = ImageCache::open(dir.path()).unwrap();
        (dir, cache)
    }

    #[test]
    fn test_open_creates_dir() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub").join("cache");
        let _cache = ImageCache::open(&sub).unwrap();
        assert!(sub.exists());
    }

    #[test]
    fn test_empty_cache_stats() {
        let (_dir, cache) = temp_cache();
        let stats = cache.stats();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.total_size_bytes, 0);
    }

    #[test]
    fn test_insert_and_lookup() {
        let (_dir, mut cache) = temp_cache();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello world").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();

        cache
            .insert("https://example.com/image.iso", tmp.path(), &hash, None)
            .unwrap();

        let entry = cache.lookup("https://example.com/image.iso");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().sha256, hash);
    }

    #[test]
    fn test_lookup_missing() {
        let (_dir, cache) = temp_cache();
        assert!(cache.lookup("https://missing.com/nope").is_none());
    }

    #[test]
    fn test_verify_valid() {
        let (_dir, mut cache) = temp_cache();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"verify me").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();

        cache
            .insert("https://example.com/v.img", tmp.path(), &hash, None)
            .unwrap();
        assert!(cache.verify("https://example.com/v.img").unwrap());
    }

    #[test]
    fn test_verify_invalid() {
        let (_dir, mut cache) = temp_cache();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"original").unwrap();

        cache
            .insert("https://example.com/bad.img", tmp.path(), "badhash", None)
            .unwrap();
        assert!(!cache.verify("https://example.com/bad.img").unwrap());
    }

    #[test]
    fn test_remove() {
        let (_dir, mut cache) = temp_cache();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"removeme").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();

        cache
            .insert("https://example.com/rm.img", tmp.path(), &hash, None)
            .unwrap();
        assert!(cache.lookup("https://example.com/rm.img").is_some());

        cache.remove("https://example.com/rm.img").unwrap();
        assert!(cache.lookup("https://example.com/rm.img").is_none());
    }

    #[test]
    fn test_clear() {
        let (_dir, mut cache) = temp_cache();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"data").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();

        cache.insert("https://a.com/1", tmp.path(), &hash, None).unwrap();
        cache.insert("https://a.com/2", tmp.path(), &hash, None).unwrap();

        cache.clear().unwrap();
        assert_eq!(cache.stats().total_entries, 0);
    }

    #[test]
    fn test_evict_max_entries() {
        let (_dir, mut cache) = temp_cache();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"x").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();

        for i in 0..5 {
            cache
                .insert(&format!("https://a.com/{}", i), tmp.path(), &hash, None)
                .unwrap();
        }

        let evicted = cache.evict(&EvictionPolicy::MaxEntries(3)).unwrap();
        assert_eq!(evicted, 2);
        assert_eq!(cache.stats().total_entries, 3);
    }

    #[test]
    fn test_stats() {
        let (_dir, mut cache) = temp_cache();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"stats test data").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();

        cache
            .insert("https://a.com/s", tmp.path(), &hash, None)
            .unwrap();
        let stats = cache.stats();
        assert_eq!(stats.total_entries, 1);
        assert!(stats.total_size_bytes > 0);
        assert_eq!(stats.verified_entries, 0);
        assert_eq!(stats.unverified_entries, 1);
    }

    #[test]
    fn test_hash_file_sha256() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"abc").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_eviction_policy_display() {
        assert_eq!(format!("{}", EvictionPolicy::MaxAge(3600)), "max-age=3600s");
        assert_eq!(format!("{}", EvictionPolicy::MaxEntries(10)), "max-entries=10");
        assert_eq!(format!("{}", EvictionPolicy::MaxSize(1024)), "max-size=1024B");
    }

    #[test]
    fn test_get_path_unverified() {
        let (_dir, mut cache) = temp_cache();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"data").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();

        cache
            .insert("https://a.com/p", tmp.path(), &hash, None)
            .unwrap();
        // Not yet verified, so get_path returns None
        assert!(cache.get_path("https://a.com/p").is_none());
    }

    #[test]
    fn test_get_path_verified() {
        let (_dir, mut cache) = temp_cache();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"verified data").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();

        cache
            .insert("https://a.com/vp", tmp.path(), &hash, None)
            .unwrap();
        cache.verify("https://a.com/vp").unwrap();
        assert!(cache.get_path("https://a.com/vp").is_some());
    }

    #[test]
    fn test_manifest_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"persist").unwrap();
        let hash = hash_file_sha256(tmp.path()).unwrap();

        {
            let mut cache = ImageCache::open(dir.path()).unwrap();
            cache
                .insert("https://a.com/persist", tmp.path(), &hash, None)
                .unwrap();
        }

        // Re-open and verify persistence
        let cache2 = ImageCache::open(dir.path()).unwrap();
        assert!(cache2.lookup("https://a.com/persist").is_some());
    }
}
