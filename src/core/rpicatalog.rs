//! Raspberry Pi OS catalog — fetch and parse the rpi-imager OS list.
//!
//! The official Raspberry Pi Imager publishes an OS catalog as JSON.
//! This module fetches, parses, and presents that catalog, enabling
//! users to browse and select OS images for writing.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Raspberry Pi Imager catalog URL.
const CATALOG_URL: &str =
    "https://downloads.raspberrypi.com/os_list_imagingutility_v4.json";

/// A parsed OS catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsCatalog {
    /// Top-level OS entries (may contain sub-lists).
    pub os_list: Vec<OsEntry>,
}

/// A single OS entry in the catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsEntry {
    /// Display name (e.g. "Raspberry Pi OS (64-bit)").
    pub name: String,

    /// Short description.
    #[serde(default)]
    pub description: String,

    /// Icon URL.
    #[serde(default)]
    pub icon: String,

    /// Direct download URL for the image (absent for category entries).
    #[serde(default)]
    pub url: String,

    /// Expected SHA-256 of the extracted image file.
    #[serde(default, rename = "extract_sha256")]
    pub sha256: String,

    /// Compressed image size in bytes.
    #[serde(default, rename = "image_download_size")]
    pub download_size: u64,

    /// Extracted image size in bytes.
    #[serde(default, rename = "extract_size")]
    pub extract_size: u64,

    /// Release date string.
    #[serde(default)]
    pub release_date: String,

    /// Nested sub-list (for category entries like "Other general-purpose OS").
    #[serde(default)]
    pub subitems: Vec<OsEntry>,

    /// URL to a sub-list JSON (alternative to inline subitems).
    #[serde(default)]
    pub subitems_url: String,
}

#[allow(dead_code)]
impl OsEntry {
    /// Whether this entry is directly downloadable.
    pub fn is_downloadable(&self) -> bool {
        !self.url.is_empty() && !self.url.ends_with(".json")
    }

    /// Whether this entry is a category with sub-entries.
    pub fn is_category(&self) -> bool {
        !self.subitems.is_empty() || !self.subitems_url.is_empty()
    }

    /// Human-readable file size.
    pub fn download_size_human(&self) -> String {
        humanize_bytes(self.download_size)
    }

    /// Human-readable extracted size.
    pub fn extract_size_human(&self) -> String {
        humanize_bytes(self.extract_size)
    }
}

fn humanize_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "—".to_string();
    }
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut size = bytes as f64;
    for &unit in UNITS {
        if size < 1024.0 {
            return format!("{:.1} {}", size, unit);
        }
        size /= 1024.0;
    }
    format!("{:.1} PiB", size)
}

/// Fetch the Raspberry Pi OS catalog from the official endpoint.
pub async fn fetch_catalog() -> Result<OsCatalog> {
    fetch_catalog_from(CATALOG_URL).await
}

/// Fetch an OS catalog from a custom URL (used for sub-lists too).
pub async fn fetch_catalog_from(url: &str) -> Result<OsCatalog> {
    let client = reqwest::Client::builder()
        .user_agent(format!("abt/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch RPi catalog from {}", url))?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("HTTP {} fetching RPi catalog from {}", status, url);
    }

    let body = resp
        .text()
        .await
        .context("Failed to read RPi catalog response body")?;

    parse_catalog(&body)
}

/// Parse a catalog JSON string.
pub fn parse_catalog(json: &str) -> Result<OsCatalog> {
    let catalog: OsCatalog =
        serde_json::from_str(json).context("Failed to parse RPi OS catalog JSON")?;
    Ok(catalog)
}

/// Flatten all downloadable entries (recursively expand sub-items).
pub fn flatten_downloadable(entries: &[OsEntry]) -> Vec<&OsEntry> {
    let mut result = Vec::new();
    for entry in entries {
        if entry.is_downloadable() {
            result.push(entry);
        }
        if !entry.subitems.is_empty() {
            result.extend(flatten_downloadable(&entry.subitems));
        }
    }
    result
}

/// Format the catalog for terminal display.
pub fn format_catalog(catalog: &OsCatalog) -> String {
    let mut output = String::new();
    output.push_str("Raspberry Pi OS Catalog\n");
    output.push_str(&"═".repeat(60));
    output.push('\n');

    for (i, entry) in catalog.os_list.iter().enumerate() {
        format_entry(&mut output, entry, i + 1, 0);
    }
    output
}

fn format_entry(output: &mut String, entry: &OsEntry, index: usize, depth: usize) {
    let indent = "  ".repeat(depth);

    if entry.is_downloadable() {
        output.push_str(&format!(
            "{}{}. {} {}\n",
            indent,
            index,
            entry.name,
            if entry.download_size > 0 {
                format!("[{}]", entry.download_size_human())
            } else {
                String::new()
            }
        ));
        if !entry.description.is_empty() {
            output.push_str(&format!("{}   {}\n", indent, entry.description));
        }
        if !entry.release_date.is_empty() {
            output.push_str(&format!("{}   Released: {}\n", indent, entry.release_date));
        }
    } else {
        output.push_str(&format!("{}{}. {} (category)\n", indent, index, entry.name));
        if !entry.description.is_empty() {
            output.push_str(&format!("{}   {}\n", indent, entry.description));
        }
    }

    for (j, sub) in entry.subitems.iter().enumerate() {
        format_entry(output, sub, j + 1, depth + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CATALOG: &str = r#"{
        "os_list": [
            {
                "name": "Raspberry Pi OS (64-bit)",
                "description": "A port of Debian with the Raspberry Pi Desktop",
                "icon": "https://example.com/rpi.png",
                "url": "https://downloads.raspberrypi.com/raspios_arm64/images/2024-01-01/image.img.xz",
                "extract_sha256": "abcdef1234567890",
                "image_download_size": 524288000,
                "extract_size": 2147483648,
                "release_date": "2024-01-01",
                "subitems": [],
                "subitems_url": ""
            },
            {
                "name": "Other general-purpose OS",
                "description": "More operating systems",
                "icon": "",
                "url": "",
                "subitems": [
                    {
                        "name": "Ubuntu Desktop 24.04",
                        "description": "Ubuntu for Raspberry Pi",
                        "icon": "",
                        "url": "https://example.com/ubuntu.img.xz",
                        "extract_sha256": "fedcba0987654321",
                        "image_download_size": 1073741824,
                        "extract_size": 4294967296,
                        "release_date": "2024-04-25",
                        "subitems": [],
                        "subitems_url": ""
                    }
                ],
                "subitems_url": ""
            },
            {
                "name": "Category with URL",
                "description": "Loads a sub-list from URL",
                "icon": "",
                "url": "",
                "subitems": [],
                "subitems_url": "https://downloads.raspberrypi.com/sub_list.json"
            }
        ]
    }"#;

    #[test]
    fn test_parse_catalog() {
        let catalog = parse_catalog(SAMPLE_CATALOG).unwrap();
        assert_eq!(catalog.os_list.len(), 3);
        assert_eq!(catalog.os_list[0].name, "Raspberry Pi OS (64-bit)");
    }

    #[test]
    fn test_entry_is_downloadable() {
        let catalog = parse_catalog(SAMPLE_CATALOG).unwrap();
        assert!(catalog.os_list[0].is_downloadable());
        assert!(!catalog.os_list[1].is_downloadable()); // category
        assert!(!catalog.os_list[2].is_downloadable()); // category with URL
    }

    #[test]
    fn test_entry_is_category() {
        let catalog = parse_catalog(SAMPLE_CATALOG).unwrap();
        assert!(!catalog.os_list[0].is_category());
        assert!(catalog.os_list[1].is_category()); // has subitems
        assert!(catalog.os_list[2].is_category()); // has subitems_url
    }

    #[test]
    fn test_flatten_downloadable() {
        let catalog = parse_catalog(SAMPLE_CATALOG).unwrap();
        let flat = flatten_downloadable(&catalog.os_list);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].name, "Raspberry Pi OS (64-bit)");
        assert_eq!(flat[1].name, "Ubuntu Desktop 24.04");
    }

    #[test]
    fn test_humanize_bytes() {
        assert_eq!(humanize_bytes(0), "—");
        assert_eq!(humanize_bytes(1024), "1.0 KiB");
        assert_eq!(humanize_bytes(1048576), "1.0 MiB");
        assert_eq!(humanize_bytes(524288000), "500.0 MiB");
        assert_eq!(humanize_bytes(1073741824), "1.0 GiB");
    }

    #[test]
    fn test_size_human_methods() {
        let catalog = parse_catalog(SAMPLE_CATALOG).unwrap();
        assert_eq!(catalog.os_list[0].download_size_human(), "500.0 MiB");
        assert_eq!(catalog.os_list[0].extract_size_human(), "2.0 GiB");
    }

    #[test]
    fn test_format_catalog() {
        let catalog = parse_catalog(SAMPLE_CATALOG).unwrap();
        let output = format_catalog(&catalog);
        assert!(output.contains("Raspberry Pi OS (64-bit)"));
        assert!(output.contains("Ubuntu Desktop 24.04"));
        assert!(output.contains("category"));
    }

    #[test]
    fn test_parse_invalid_json() {
        assert!(parse_catalog("not json").is_err());
    }
}
