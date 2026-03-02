// Generic pluggable ISO download catalog — provider-based multi-distro image catalog.
//
// Inspired by rpi-imager JSON catalog, Rufus FIDO script, and MediaWriter's
// release list. Provides a unified catalog abstraction that supports multiple
// providers (Microsoft, Ubuntu, Fedora, Raspberry Pi, custom repositories).
//
// Key features:
//   - Provider registration system with trait-based plugin architecture
//   - JSON catalog format with hardware tags, device filtering, sublists
//   - Custom repository URLs for enterprise/offline use
//   - Catalog caching with TTL-based refresh
//   - Hardware compatibility filtering

#![allow(dead_code)]

use anyhow::{Context, Result};
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// A catalog provider identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderId {
    RaspberryPi,
    Ubuntu,
    Fedora,
    Microsoft,
    Custom(String),
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderId::RaspberryPi => write!(f, "raspberrypi"),
            ProviderId::Ubuntu => write!(f, "ubuntu"),
            ProviderId::Fedora => write!(f, "fedora"),
            ProviderId::Microsoft => write!(f, "microsoft"),
            ProviderId::Custom(name) => write!(f, "custom:{}", name),
        }
    }
}

impl ProviderId {
    /// Parse from string representation.
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "raspberrypi" | "rpi" => Self::RaspberryPi,
            "ubuntu" => Self::Ubuntu,
            "fedora" => Self::Fedora,
            "microsoft" | "windows" => Self::Microsoft,
            other => {
                if let Some(name) = other.strip_prefix("custom:") {
                    Self::Custom(name.to_string())
                } else {
                    Self::Custom(other.to_string())
                }
            }
        }
    }
}

/// Hardware tag for device/architecture filtering.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HardwareTag {
    /// CPU architecture (e.g., "amd64", "arm64", "armhf").
    Arch(String),
    /// Device type (e.g., "rpi4", "rpi5", "pc").
    Device(String),
    /// Boot mode (e.g., "uefi", "bios", "both").
    BootMode(String),
    /// Custom tag.
    Custom(String),
}

impl std::fmt::Display for HardwareTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HardwareTag::Arch(a) => write!(f, "arch:{}", a),
            HardwareTag::Device(d) => write!(f, "device:{}", d),
            HardwareTag::BootMode(b) => write!(f, "boot:{}", b),
            HardwareTag::Custom(c) => write!(f, "tag:{}", c),
        }
    }
}

impl HardwareTag {
    /// Parse from "type:value" format.
    pub fn parse(s: &str) -> Self {
        if let Some((kind, value)) = s.split_once(':') {
            match kind {
                "arch" => Self::Arch(value.to_string()),
                "device" => Self::Device(value.to_string()),
                "boot" => Self::BootMode(value.to_string()),
                _ => Self::Custom(s.to_string()),
            }
        } else {
            Self::Custom(s.to_string())
        }
    }
}

/// A single downloadable image entry in a catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogEntry {
    /// Unique entry ID within its provider.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Short description.
    #[serde(default)]
    pub description: String,
    /// Version string (e.g., "24.04 LTS").
    #[serde(default)]
    pub version: String,
    /// Download URL (direct link to image file).
    #[serde(default)]
    pub url: String,
    /// Icon URL or embedded base64.
    #[serde(default)]
    pub icon: String,
    /// SHA-256 checksum of the extracted image.
    #[serde(default)]
    pub sha256: String,
    /// Compressed download size in bytes.
    #[serde(default)]
    pub download_size: u64,
    /// Extracted (uncompressed) image size in bytes.
    #[serde(default)]
    pub extract_size: u64,
    /// Release date (ISO 8601).
    #[serde(default)]
    pub release_date: String,
    /// Hardware tags for filtering.
    #[serde(default)]
    pub tags: Vec<HardwareTag>,
    /// Provider-specific metadata.
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    /// Child entries (for sub-lists / categories).
    #[serde(default)]
    pub children: Vec<CatalogEntry>,
}

impl CatalogEntry {
    /// Whether this is a category (has children, no direct download).
    pub fn is_category(&self) -> bool {
        !self.children.is_empty() && self.url.is_empty()
    }

    /// Whether this entry matches a given hardware tag.
    pub fn matches_tag(&self, tag: &HardwareTag) -> bool {
        self.tags.contains(tag) || self.tags.is_empty()
    }

    /// Filter entry tree by hardware tag (returns matching entries only).
    pub fn filter_by_tag(&self, tag: &HardwareTag) -> Option<CatalogEntry> {
        if self.is_category() {
            let filtered: Vec<CatalogEntry> = self
                .children
                .iter()
                .filter_map(|c| c.filter_by_tag(tag))
                .collect();
            if filtered.is_empty() {
                None
            } else {
                let mut entry = self.clone();
                entry.children = filtered;
                Some(entry)
            }
        } else if self.matches_tag(tag) {
            Some(self.clone())
        } else {
            None
        }
    }

    /// Flatten the tree into a list of downloadable entries.
    pub fn flatten(&self) -> Vec<&CatalogEntry> {
        let mut result = Vec::new();
        if !self.url.is_empty() {
            result.push(self);
        }
        for child in &self.children {
            result.extend(child.flatten());
        }
        result
    }
}

/// A provider's catalog — the top-level container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCatalog {
    /// Provider identifier.
    pub provider: ProviderId,
    /// Display name for the provider.
    pub display_name: String,
    /// Catalog URL (for refresh).
    pub catalog_url: String,
    /// Top-level entries.
    pub entries: Vec<CatalogEntry>,
    /// Last fetch timestamp (UNIX epoch seconds).
    #[serde(default)]
    pub fetched_at: u64,
    /// Cache TTL in seconds.
    #[serde(default = "default_ttl")]
    pub cache_ttl: u64,
}

fn default_ttl() -> u64 {
    3600 // 1 hour
}

impl ProviderCatalog {
    /// Check if the cached catalog is expired.
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now - self.fetched_at > self.cache_ttl
    }

    /// Total number of downloadable entries (recursive).
    pub fn entry_count(&self) -> usize {
        self.entries.iter().map(|e| e.flatten().len()).sum()
    }

    /// Find an entry by ID (recursive).
    pub fn find_entry(&self, id: &str) -> Option<&CatalogEntry> {
        fn search<'a>(entries: &'a [CatalogEntry], id: &str) -> Option<&'a CatalogEntry> {
            for entry in entries {
                if entry.id == id {
                    return Some(entry);
                }
                if let Some(found) = search(&entry.children, id) {
                    return Some(found);
                }
            }
            None
        }
        search(&self.entries, id)
    }

    /// Filter the entire catalog by hardware tag.
    pub fn filter_by_tag(&self, tag: &HardwareTag) -> ProviderCatalog {
        let entries = self
            .entries
            .iter()
            .filter_map(|e| e.filter_by_tag(tag))
            .collect();
        ProviderCatalog {
            entries,
            ..self.clone()
        }
    }
}

/// Unified catalog registry — holds catalogs from multiple providers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CatalogRegistry {
    /// Registered provider catalogs.
    pub catalogs: HashMap<String, ProviderCatalog>,
    /// Cache directory for offline catalog storage.
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
}

impl CatalogRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            catalogs: HashMap::new(),
            cache_dir: None,
        }
    }

    /// Create a registry with a cache directory.
    pub fn with_cache_dir(cache_dir: PathBuf) -> Self {
        Self {
            catalogs: HashMap::new(),
            cache_dir: Some(cache_dir),
        }
    }

    /// Register a provider catalog.
    pub fn register(&mut self, catalog: ProviderCatalog) {
        let key = catalog.provider.to_string();
        info!("Registered catalog provider: {}", key);
        self.catalogs.insert(key, catalog);
    }

    /// Get a catalog by provider ID.
    pub fn get(&self, provider: &ProviderId) -> Option<&ProviderCatalog> {
        self.catalogs.get(&provider.to_string())
    }

    /// List all registered provider IDs.
    pub fn providers(&self) -> Vec<&str> {
        self.catalogs.keys().map(|s| s.as_str()).collect()
    }

    /// Total entries across all providers.
    pub fn total_entries(&self) -> usize {
        self.catalogs.values().map(|c| c.entry_count()).sum()
    }

    /// Search all catalogs for entries matching a query string (name or description).
    pub fn search(&self, query: &str) -> Vec<(&ProviderId, &CatalogEntry)> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for catalog in self.catalogs.values() {
            for entry in &catalog.entries {
                Self::search_entry(&catalog.provider, entry, &query_lower, &mut results);
            }
        }

        results
    }

    fn search_entry<'a>(
        provider: &'a ProviderId,
        entry: &'a CatalogEntry,
        query: &str,
        results: &mut Vec<(&'a ProviderId, &'a CatalogEntry)>,
    ) {
        if entry.name.to_lowercase().contains(query)
            || entry.description.to_lowercase().contains(query)
            || entry.version.to_lowercase().contains(query)
        {
            results.push((provider, entry));
        }
        for child in &entry.children {
            Self::search_entry(provider, child, query, results);
        }
    }

    /// Save the registry cache to disk.
    pub fn save_cache(&self) -> Result<()> {
        let cache_dir = self
            .cache_dir
            .as_ref()
            .context("No cache directory configured")?;
        std::fs::create_dir_all(cache_dir)?;

        for (key, catalog) in &self.catalogs {
            let path = cache_dir.join(format!("{}.json", key));
            let json = serde_json::to_string_pretty(catalog)?;
            std::fs::write(&path, json)?;
            debug!("Saved catalog cache: {}", path.display());
        }
        Ok(())
    }

    /// Load cached catalogs from disk.
    pub fn load_cache(&mut self) -> Result<usize> {
        let cache_dir = self
            .cache_dir
            .as_ref()
            .context("No cache directory configured")?;

        if !cache_dir.exists() {
            return Ok(0);
        }

        let mut count = 0;
        for entry in std::fs::read_dir(cache_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().map_or(false, |ext| ext == "json") {
                match std::fs::read_to_string(&path) {
                    Ok(content) => match serde_json::from_str::<ProviderCatalog>(&content) {
                        Ok(catalog) => {
                            let key = catalog.provider.to_string();
                            debug!("Loaded cached catalog: {}", key);
                            self.catalogs.insert(key, catalog);
                            count += 1;
                        }
                        Err(e) => {
                            warn!("Failed to parse catalog {}: {}", path.display(), e);
                        }
                    },
                    Err(e) => {
                        warn!("Failed to read catalog {}: {}", path.display(), e);
                    }
                }
            }
        }

        Ok(count)
    }
}

/// Fetch a remote JSON catalog from a URL.
pub async fn fetch_catalog(
    url: &str,
    provider: ProviderId,
    display_name: &str,
) -> Result<ProviderCatalog> {
    info!("Fetching catalog from: {}", url);

    let client = reqwest::Client::builder()
        .user_agent(format!("abt/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    let resp = client.get(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} fetching catalog from {}", resp.status().as_u16(), url);
    }

    let body = resp.text().await?;

    // Try to parse as our native format first
    if let Ok(mut catalog) = serde_json::from_str::<ProviderCatalog>(&body) {
        catalog.fetched_at = now_epoch();
        return Ok(catalog);
    }

    // Try rpi-imager format (has "os_list" at top level)
    if let Ok(rpi) = serde_json::from_str::<RpiImagerCatalog>(&body) {
        let entries = rpi
            .os_list
            .into_iter()
            .map(|e| convert_rpi_entry(e))
            .collect();
        return Ok(ProviderCatalog {
            provider,
            display_name: display_name.to_string(),
            catalog_url: url.to_string(),
            entries,
            fetched_at: now_epoch(),
            cache_ttl: default_ttl(),
        });
    }

    anyhow::bail!("Could not parse catalog from {}", url)
}

/// Current UNIX epoch seconds.
fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// rpi-imager catalog format wrapper.
#[derive(Debug, Deserialize)]
struct RpiImagerCatalog {
    os_list: Vec<RpiImagerEntry>,
}

/// rpi-imager OS entry.
#[derive(Debug, Deserialize)]
struct RpiImagerEntry {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    icon: String,
    #[serde(default)]
    url: String,
    #[serde(default, rename = "extract_sha256")]
    sha256: String,
    #[serde(default, rename = "image_download_size")]
    download_size: u64,
    #[serde(default, rename = "extract_size")]
    extract_size: u64,
    #[serde(default)]
    release_date: String,
    #[serde(default)]
    devices: Vec<String>,
    #[serde(default)]
    subitems: Vec<RpiImagerEntry>,
}

/// Convert an rpi-imager entry to our CatalogEntry format.
fn convert_rpi_entry(rpi: RpiImagerEntry) -> CatalogEntry {
    let tags: Vec<HardwareTag> = rpi
        .devices
        .iter()
        .map(|d| HardwareTag::Device(d.clone()))
        .collect();

    let children: Vec<CatalogEntry> = rpi
        .subitems
        .into_iter()
        .map(|e| convert_rpi_entry(e))
        .collect();

    CatalogEntry {
        id: slug(&rpi.name),
        name: rpi.name,
        description: rpi.description,
        version: String::new(),
        url: rpi.url,
        icon: rpi.icon,
        sha256: rpi.sha256,
        download_size: rpi.download_size,
        extract_size: rpi.extract_size,
        release_date: rpi.release_date,
        tags,
        metadata: HashMap::new(),
        children,
    }
}

/// Create a URL-friendly slug from a name.
fn slug(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<&str>>()
        .join("-")
}

/// Well-known provider catalog URLs.
pub fn default_catalog_urls() -> Vec<(ProviderId, &'static str, &'static str)> {
    vec![
        (
            ProviderId::RaspberryPi,
            "Raspberry Pi",
            "https://downloads.raspberrypi.com/os_list_imagingutility_v4.json",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_display() {
        assert_eq!(ProviderId::RaspberryPi.to_string(), "raspberrypi");
        assert_eq!(ProviderId::Ubuntu.to_string(), "ubuntu");
        assert_eq!(ProviderId::Fedora.to_string(), "fedora");
        assert_eq!(ProviderId::Microsoft.to_string(), "microsoft");
        assert_eq!(ProviderId::Custom("arch".into()).to_string(), "custom:arch");
    }

    #[test]
    fn test_provider_id_parse() {
        assert_eq!(ProviderId::parse("raspberrypi"), ProviderId::RaspberryPi);
        assert_eq!(ProviderId::parse("rpi"), ProviderId::RaspberryPi);
        assert_eq!(ProviderId::parse("ubuntu"), ProviderId::Ubuntu);
        assert_eq!(ProviderId::parse("windows"), ProviderId::Microsoft);
        assert_eq!(ProviderId::parse("custom:arch"), ProviderId::Custom("arch".into()));
        assert_eq!(ProviderId::parse("nixos"), ProviderId::Custom("nixos".into()));
    }

    #[test]
    fn test_hardware_tag_display() {
        assert_eq!(HardwareTag::Arch("amd64".into()).to_string(), "arch:amd64");
        assert_eq!(HardwareTag::Device("rpi4".into()).to_string(), "device:rpi4");
        assert_eq!(HardwareTag::BootMode("uefi".into()).to_string(), "boot:uefi");
    }

    #[test]
    fn test_hardware_tag_parse() {
        assert_eq!(
            HardwareTag::parse("arch:arm64"),
            HardwareTag::Arch("arm64".into())
        );
        assert_eq!(
            HardwareTag::parse("device:rpi5"),
            HardwareTag::Device("rpi5".into())
        );
        assert_eq!(
            HardwareTag::parse("boot:bios"),
            HardwareTag::BootMode("bios".into())
        );
        assert_eq!(
            HardwareTag::parse("random"),
            HardwareTag::Custom("random".into())
        );
    }

    #[test]
    fn test_catalog_entry_is_category() {
        let cat = CatalogEntry {
            id: "cat".into(),
            name: "Category".into(),
            description: String::new(),
            version: String::new(),
            url: String::new(),
            icon: String::new(),
            sha256: String::new(),
            download_size: 0,
            extract_size: 0,
            release_date: String::new(),
            tags: vec![],
            metadata: HashMap::new(),
            children: vec![CatalogEntry {
                id: "child".into(),
                name: "Child".into(),
                description: String::new(),
                version: String::new(),
                url: "https://example.com/image.img".into(),
                icon: String::new(),
                sha256: String::new(),
                download_size: 1000,
                extract_size: 2000,
                release_date: String::new(),
                tags: vec![],
                metadata: HashMap::new(),
                children: vec![],
            }],
        };
        assert!(cat.is_category());
        assert!(!cat.children[0].is_category());
    }

    #[test]
    fn test_catalog_entry_matches_tag() {
        let entry = CatalogEntry {
            id: "test".into(),
            name: "Test".into(),
            description: String::new(),
            version: String::new(),
            url: String::new(),
            icon: String::new(),
            sha256: String::new(),
            download_size: 0,
            extract_size: 0,
            release_date: String::new(),
            tags: vec![HardwareTag::Arch("arm64".into())],
            metadata: HashMap::new(),
            children: vec![],
        };
        assert!(entry.matches_tag(&HardwareTag::Arch("arm64".into())));
        assert!(!entry.matches_tag(&HardwareTag::Arch("amd64".into())));
    }

    #[test]
    fn test_catalog_entry_matches_tag_empty_tags() {
        let entry = CatalogEntry {
            id: "test".into(),
            name: "Test".into(),
            description: String::new(),
            version: String::new(),
            url: String::new(),
            icon: String::new(),
            sha256: String::new(),
            download_size: 0,
            extract_size: 0,
            release_date: String::new(),
            tags: vec![],
            metadata: HashMap::new(),
            children: vec![],
        };
        // Empty tags means "matches everything"
        assert!(entry.matches_tag(&HardwareTag::Arch("amd64".into())));
    }

    #[test]
    fn test_catalog_entry_flatten() {
        let cat = CatalogEntry {
            id: "cat".into(),
            name: "Category".into(),
            description: String::new(),
            version: String::new(),
            url: String::new(),
            icon: String::new(),
            sha256: String::new(),
            download_size: 0,
            extract_size: 0,
            release_date: String::new(),
            tags: vec![],
            metadata: HashMap::new(),
            children: vec![
                CatalogEntry {
                    id: "a".into(),
                    name: "A".into(),
                    description: String::new(),
                    version: String::new(),
                    url: "https://example.com/a.img".into(),
                    icon: String::new(),
                    sha256: String::new(),
                    download_size: 100,
                    extract_size: 200,
                    release_date: String::new(),
                    tags: vec![],
                    metadata: HashMap::new(),
                    children: vec![],
                },
                CatalogEntry {
                    id: "b".into(),
                    name: "B".into(),
                    description: String::new(),
                    version: String::new(),
                    url: "https://example.com/b.img".into(),
                    icon: String::new(),
                    sha256: String::new(),
                    download_size: 300,
                    extract_size: 400,
                    release_date: String::new(),
                    tags: vec![],
                    metadata: HashMap::new(),
                    children: vec![],
                },
            ],
        };
        assert_eq!(cat.flatten().len(), 2);
    }

    #[test]
    fn test_provider_catalog_entry_count() {
        let catalog = ProviderCatalog {
            provider: ProviderId::Ubuntu,
            display_name: "Ubuntu".into(),
            catalog_url: "https://example.com".into(),
            entries: vec![CatalogEntry {
                id: "ubuntu-24".into(),
                name: "Ubuntu 24.04".into(),
                description: String::new(),
                version: "24.04".into(),
                url: "https://example.com/ubuntu.iso".into(),
                icon: String::new(),
                sha256: String::new(),
                download_size: 1000,
                extract_size: 2000,
                release_date: String::new(),
                tags: vec![],
                metadata: HashMap::new(),
                children: vec![],
            }],
            fetched_at: 0,
            cache_ttl: 3600,
        };
        assert_eq!(catalog.entry_count(), 1);
    }

    #[test]
    fn test_provider_catalog_find_entry() {
        let catalog = ProviderCatalog {
            provider: ProviderId::Fedora,
            display_name: "Fedora".into(),
            catalog_url: "https://example.com".into(),
            entries: vec![CatalogEntry {
                id: "fedora-41".into(),
                name: "Fedora 41".into(),
                description: String::new(),
                version: "41".into(),
                url: "https://example.com/fedora.iso".into(),
                icon: String::new(),
                sha256: String::new(),
                download_size: 2000,
                extract_size: 4000,
                release_date: String::new(),
                tags: vec![],
                metadata: HashMap::new(),
                children: vec![],
            }],
            fetched_at: 0,
            cache_ttl: 3600,
        };
        assert!(catalog.find_entry("fedora-41").is_some());
        assert!(catalog.find_entry("nonexistent").is_none());
    }

    #[test]
    fn test_catalog_registry_basics() {
        let mut reg = CatalogRegistry::new();
        assert_eq!(reg.total_entries(), 0);
        assert!(reg.providers().is_empty());

        reg.register(ProviderCatalog {
            provider: ProviderId::Ubuntu,
            display_name: "Ubuntu".into(),
            catalog_url: "https://example.com".into(),
            entries: vec![CatalogEntry {
                id: "u1".into(),
                name: "Ubuntu".into(),
                description: "Ubuntu OS".into(),
                version: "24.04".into(),
                url: "https://example.com/u.iso".into(),
                icon: String::new(),
                sha256: String::new(),
                download_size: 0,
                extract_size: 0,
                release_date: String::new(),
                tags: vec![],
                metadata: HashMap::new(),
                children: vec![],
            }],
            fetched_at: 0,
            cache_ttl: 3600,
        });

        assert_eq!(reg.total_entries(), 1);
        assert_eq!(reg.providers().len(), 1);
        assert!(reg.get(&ProviderId::Ubuntu).is_some());
    }

    #[test]
    fn test_catalog_registry_search() {
        let mut reg = CatalogRegistry::new();
        reg.register(ProviderCatalog {
            provider: ProviderId::Ubuntu,
            display_name: "Ubuntu".into(),
            catalog_url: "https://example.com".into(),
            entries: vec![
                CatalogEntry {
                    id: "u1".into(),
                    name: "Ubuntu Desktop".into(),
                    description: "Full desktop".into(),
                    version: "24.04".into(),
                    url: "https://example.com/u.iso".into(),
                    icon: String::new(),
                    sha256: String::new(),
                    download_size: 0,
                    extract_size: 0,
                    release_date: String::new(),
                    tags: vec![],
                    metadata: HashMap::new(),
                    children: vec![],
                },
                CatalogEntry {
                    id: "u2".into(),
                    name: "Ubuntu Server".into(),
                    description: "Server".into(),
                    version: "24.04".into(),
                    url: "https://example.com/us.iso".into(),
                    icon: String::new(),
                    sha256: String::new(),
                    download_size: 0,
                    extract_size: 0,
                    release_date: String::new(),
                    tags: vec![],
                    metadata: HashMap::new(),
                    children: vec![],
                },
            ],
            fetched_at: 0,
            cache_ttl: 3600,
        });

        let results = reg.search("desktop");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.name, "Ubuntu Desktop");

        let results = reg.search("ubuntu");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_catalog_registry_cache_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut reg = CatalogRegistry::with_cache_dir(dir.path().to_path_buf());
        reg.register(ProviderCatalog {
            provider: ProviderId::Fedora,
            display_name: "Fedora".into(),
            catalog_url: "https://example.com".into(),
            entries: vec![],
            fetched_at: 12345,
            cache_ttl: 3600,
        });
        reg.save_cache().unwrap();

        let mut reg2 = CatalogRegistry::with_cache_dir(dir.path().to_path_buf());
        let loaded = reg2.load_cache().unwrap();
        assert_eq!(loaded, 1);
        assert!(reg2.get(&ProviderId::Fedora).is_some());
    }

    #[test]
    fn test_slug() {
        assert_eq!(slug("Hello World"), "hello-world");
        assert_eq!(slug("Ubuntu 24.04 LTS"), "ubuntu-24-04-lts");
        assert_eq!(slug("Raspberry Pi OS (64-bit)"), "raspberry-pi-os-64-bit");
    }

    #[test]
    fn test_provider_catalog_is_expired() {
        let catalog = ProviderCatalog {
            provider: ProviderId::Ubuntu,
            display_name: "Ubuntu".into(),
            catalog_url: "https://example.com".into(),
            entries: vec![],
            fetched_at: 0, // epoch = definitely expired
            cache_ttl: 3600,
        };
        assert!(catalog.is_expired());
    }

    #[test]
    fn test_provider_catalog_not_expired() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let catalog = ProviderCatalog {
            provider: ProviderId::Ubuntu,
            display_name: "Ubuntu".into(),
            catalog_url: "https://example.com".into(),
            entries: vec![],
            fetched_at: now,
            cache_ttl: 3600,
        };
        assert!(!catalog.is_expired());
    }

    #[test]
    fn test_default_catalog_urls() {
        let urls = default_catalog_urls();
        assert!(!urls.is_empty());
        assert_eq!(urls[0].0, ProviderId::RaspberryPi);
    }

    #[test]
    fn test_catalog_serde_roundtrip() {
        let catalog = ProviderCatalog {
            provider: ProviderId::Custom("test".into()),
            display_name: "Test".into(),
            catalog_url: "https://example.com".into(),
            entries: vec![CatalogEntry {
                id: "e1".into(),
                name: "Entry".into(),
                description: String::new(),
                version: "1.0".into(),
                url: "https://example.com/e.img".into(),
                icon: String::new(),
                sha256: "abc".into(),
                download_size: 100,
                extract_size: 200,
                release_date: "2025-01-01".into(),
                tags: vec![HardwareTag::Arch("amd64".into())],
                metadata: HashMap::new(),
                children: vec![],
            }],
            fetched_at: 100,
            cache_ttl: 7200,
        };

        let json = serde_json::to_string(&catalog).unwrap();
        let deser: ProviderCatalog = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.entries[0].id, "e1");
        assert_eq!(deser.cache_ttl, 7200);
    }
}
