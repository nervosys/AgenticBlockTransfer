// Mirror selection and failover module — download from the fastest/most reliable mirror.
//
// Inspired by MediaWriter's DownloadManager which fetches a mirror list from a
// service URL and falls back to alternative mirrors on connection failure or stall.
//
// Supports:
//   - Mirror list fetching from a URL (JSON array of mirrors)
//   - Manual mirror list configuration
//   - Automatic failover on download failure
//   - Mirror latency probing
//   - Metalink (RFC 5854) parsing for mirrors and checksums

use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// A download mirror with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mirror {
    /// Base URL of the mirror (e.g., "https://mirror.example.com/releases/").
    pub url: String,
    /// Mirror location / country code (e.g., "US", "DE").
    #[serde(default)]
    pub location: String,
    /// Optional display name.
    #[serde(default)]
    pub name: String,
    /// Priority (lower = higher priority). Default 100.
    #[serde(default = "default_priority")]
    pub priority: u32,
    /// Measured latency in milliseconds (0 = not measured).
    #[serde(skip)]
    pub latency_ms: u64,
    /// Whether this mirror failed on the last attempt.
    #[serde(skip)]
    pub failed: bool,
}

fn default_priority() -> u32 {
    100
}

/// A mirror list that can be queried for the best available mirror.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirrorList {
    /// Available mirrors, sorted by preference.
    pub mirrors: Vec<Mirror>,
    /// Maximum number of mirrors to probe for latency.
    #[serde(default = "default_max_probe")]
    pub max_probe: usize,
    /// Timeout for latency probes in milliseconds.
    #[serde(default = "default_probe_timeout_ms")]
    pub probe_timeout_ms: u64,
}

fn default_max_probe() -> usize {
    8
}

fn default_probe_timeout_ms() -> u64 {
    5000
}

impl Default for MirrorList {
    fn default() -> Self {
        Self {
            mirrors: Vec::new(),
            max_probe: 8,
            probe_timeout_ms: 5000,
        }
    }
}

impl MirrorList {
    /// Create a mirror list from a list of URLs.
    pub fn from_urls(urls: &[&str]) -> Self {
        Self {
            mirrors: urls
                .iter()
                .enumerate()
                .map(|(i, url)| Mirror {
                    url: url.to_string(),
                    location: String::new(),
                    name: format!("Mirror {}", i + 1),
                    priority: (i as u32) + 1,
                    latency_ms: 0,
                    failed: false,
                })
                .collect(),
            ..Default::default()
        }
    }

    /// Number of available (non-failed) mirrors.
    pub fn available_count(&self) -> usize {
        self.mirrors.iter().filter(|m| !m.failed).count()
    }

    /// Get the best available mirror URL (lowest latency among non-failed, or
    /// lowest priority if latency not measured).
    pub fn best_mirror(&self) -> Option<&Mirror> {
        self.mirrors
            .iter()
            .filter(|m| !m.failed)
            .min_by_key(|m| {
                if m.latency_ms > 0 {
                    m.latency_ms
                } else {
                    m.priority as u64 * 1000
                }
            })
    }

    /// Mark a mirror as failed (it will be skipped on subsequent calls to `best_mirror`).
    pub fn mark_failed(&mut self, url: &str) {
        if let Some(m) = self.mirrors.iter_mut().find(|m| m.url == url) {
            m.failed = true;
            warn!("Mirror marked as failed: {}", url);
        }
    }

    /// Reset all failure flags (e.g., for a retry cycle).
    pub fn reset_failures(&mut self) {
        for m in &mut self.mirrors {
            m.failed = false;
        }
    }

    /// Construct a full download URL by combining a mirror base URL with a relative path.
    pub fn mirror_url(mirror: &Mirror, relative_path: &str) -> String {
        let base = mirror.url.trim_end_matches('/');
        let path = relative_path.trim_start_matches('/');
        format!("{}/{}", base, path)
    }
}

/// Fetch a mirror list from a JSON URL (expects an array of mirror objects or strings).
pub async fn fetch_mirror_list(list_url: &str) -> Result<MirrorList> {
    info!("Fetching mirror list from: {}", list_url);

    let client = reqwest::Client::builder()
        .user_agent(format!("abt/{}", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .build()?;

    let resp = client.get(list_url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Mirror list fetch failed: HTTP {}", resp.status().as_u16());
    }

    let body = resp.text().await?;

    // Try parsing as array of Mirror objects first, then as array of strings
    if let Ok(mirrors) = serde_json::from_str::<Vec<Mirror>>(&body) {
        info!("Loaded {} mirrors from JSON object list", mirrors.len());
        return Ok(MirrorList {
            mirrors,
            ..Default::default()
        });
    }

    if let Ok(urls) = serde_json::from_str::<Vec<String>>(&body) {
        info!("Loaded {} mirrors from URL list", urls.len());
        let url_refs: Vec<&str> = urls.iter().map(|s| s.as_str()).collect();
        return Ok(MirrorList::from_urls(&url_refs));
    }

    anyhow::bail!("Could not parse mirror list JSON (expected array of mirrors or URLs)")
}

/// Probe a single mirror for latency by sending a HEAD request.
pub async fn probe_mirror_latency(url: &str, timeout: Duration) -> Result<u64> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .connect_timeout(timeout)
        .build()?;

    let start = Instant::now();
    let resp = client.head(url).send().await?;
    let latency = start.elapsed().as_millis() as u64;

    if resp.status().is_success() || resp.status().is_redirection() {
        Ok(latency)
    } else {
        anyhow::bail!("Mirror probe failed: HTTP {}", resp.status().as_u16())
    }
}

/// Probe all mirrors for latency and sort by fastest.
pub async fn probe_and_sort(mirror_list: &mut MirrorList) {
    let timeout = Duration::from_millis(mirror_list.probe_timeout_ms);
    let max_probe = mirror_list.max_probe.min(mirror_list.mirrors.len());

    info!("Probing {} mirrors for latency...", max_probe);

    for i in 0..max_probe {
        let url = mirror_list.mirrors[i].url.clone();
        match probe_mirror_latency(&url, timeout).await {
            Ok(latency) => {
                mirror_list.mirrors[i].latency_ms = latency;
                info!("  {} — {}ms", url, latency);
            }
            Err(e) => {
                mirror_list.mirrors[i].latency_ms = u64::MAX;
                mirror_list.mirrors[i].failed = true;
                warn!("  {} — probe failed: {}", url, e);
            }
        }
    }

    // Sort by latency (non-probed mirrors at the end)
    mirror_list.mirrors.sort_by_key(|m| {
        if m.failed {
            u64::MAX
        } else if m.latency_ms > 0 {
            m.latency_ms
        } else {
            u64::MAX - 1
        }
    });
}

/// Simple metalink parser — extract mirror URLs and optional checksums from
/// a Metalink v4 (RFC 5854) XML document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetalinkInfo {
    /// Extracted mirror URLs with their priorities.
    pub mirrors: Vec<Mirror>,
    /// File name.
    pub filename: String,
    /// File size in bytes (0 if not specified).
    pub size: u64,
    /// SHA-256 hash (if specified in the metalink).
    pub sha256: Option<String>,
    /// MD5 hash (if specified).
    pub md5: Option<String>,
}

/// Parse a simple metalink-like document from text content.
/// This is a lightweight parser that handles common patterns without
/// requiring a full XML parser dependency.
pub fn parse_metalink_urls(content: &str) -> Result<MetalinkInfo> {
    let mut mirrors = Vec::new();
    let mut filename = String::new();
    let mut size: u64 = 0;
    let mut sha256 = None;
    let mut md5 = None;

    for line in content.lines() {
        let line = line.trim();

        // Extract URLs (common in .meta4 and .metalink files)
        if line.contains("<url") && line.contains("</url>") {
            let priority = extract_attr(line, "priority")
                .and_then(|p| p.parse::<u32>().ok())
                .unwrap_or(100);
            let location = extract_attr(line, "location").unwrap_or_default();

            if let Some(url) = extract_tag_content(line, "url") {
                mirrors.push(Mirror {
                    url,
                    location,
                    name: String::new(),
                    priority,
                    latency_ms: 0,
                    failed: false,
                });
            }
        }

        // Extract filename
        if line.contains("<file") {
            if let Some(name) = extract_attr(line, "name") {
                filename = name;
            }
        }

        // Extract size
        if line.contains("<size>") {
            if let Some(s) = extract_tag_content(line, "size") {
                size = s.parse().unwrap_or(0);
            }
        }

        // Extract hashes
        if line.contains("<hash") && line.contains("sha-256") {
            if let Some(h) = extract_tag_content(line, "hash") {
                sha256 = Some(h);
            }
        }
        if line.contains("<hash") && line.contains("md5") {
            if let Some(h) = extract_tag_content(line, "hash") {
                md5 = Some(h);
            }
        }
    }

    mirrors.sort_by_key(|m| m.priority);

    Ok(MetalinkInfo {
        mirrors,
        filename,
        size,
        sha256,
        md5,
    })
}

/// Extract an XML attribute value (simple regex-free approach).
fn extract_attr(line: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = line.find(&pattern)?;
    let value_start = start + pattern.len();
    let rest = &line[value_start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Extract text content between XML tags.
fn extract_tag_content(line: &str, tag: &str) -> Option<String> {
    let open_end = line.find('>')?;
    let close_pattern = format!("</{}>", tag);
    let close_start = line.find(&close_pattern)?;
    if open_end + 1 <= close_start {
        Some(line[open_end + 1..close_start].to_string())
    } else {
        None
    }
}

/// Download a file with mirror failover.
///
/// Tries each mirror in order of preference. If a download fails, moves to
/// the next mirror automatically.
///
/// # Arguments
/// * `mirror_list` — List of mirrors to try.
/// * `relative_path` — Relative path to the file on each mirror.
/// * `output_dir` — Directory to save the downloaded file.
/// * `progress` — Progress handle for reporting and cancellation.
///
/// # Returns
/// Path to the downloaded file, or error if all mirrors failed.
pub async fn download_with_failover(
    mirror_list: &mut MirrorList,
    relative_path: &str,
    output_dir: &std::path::Path,
    progress: &super::progress::Progress,
) -> Result<std::path::PathBuf> {
    use super::download_resume::{download_with_resume, ResumeDownloadOpts};

    let filename = relative_path.rsplit('/').next().unwrap_or("download");

    let candidates: Vec<(String, String)> = mirror_list
        .mirrors
        .iter()
        .filter(|m| !m.failed)
        .map(|m| (m.url.clone(), m.name.clone()))
        .collect();

    for (mirror_url, mirror_name) in &candidates {
        let url = format!("{}{}{}", mirror_url.trim_end_matches('/'), "/", relative_path.trim_start_matches('/'));
        info!("Trying mirror: {} ({})", mirror_name, url);

        let opts = ResumeDownloadOpts {
            url: url.clone(),
            output_dir: output_dir.to_path_buf(),
            filename: Some(filename.to_string()),
            max_retries: 1, // Per-mirror: only 1 retry, then failover
            retry_delay_secs: 1,
            force_fresh: false,
        };

        match download_with_resume(&opts, progress).await {
            Ok(result) => {
                info!("Download succeeded from mirror: {}", mirror_name);
                return Ok(result.path);
            }
            Err(e) => {
                if progress.is_cancelled() {
                    anyhow::bail!("Download cancelled by user");
                }
                warn!("Mirror {} failed: {}", mirror_name, e);
                mirror_list.mark_failed(mirror_url);
            }
        }
    }

    anyhow::bail!(
        "All {} mirrors failed for {}",
        mirror_list.mirrors.len(),
        relative_path
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mirror_list_from_urls() {
        let list = MirrorList::from_urls(&[
            "https://mirror1.example.com/",
            "https://mirror2.example.com/",
            "https://mirror3.example.com/",
        ]);
        assert_eq!(list.mirrors.len(), 3);
        assert_eq!(list.mirrors[0].priority, 1);
        assert_eq!(list.mirrors[2].priority, 3);
    }

    #[test]
    fn test_best_mirror_by_priority() {
        let list = MirrorList::from_urls(&[
            "https://slow.example.com/",
            "https://fast.example.com/",
        ]);
        let best = list.best_mirror().unwrap();
        assert_eq!(best.url, "https://slow.example.com/"); // Priority 1 wins
    }

    #[test]
    fn test_best_mirror_by_latency() {
        let mut list = MirrorList::from_urls(&[
            "https://slow.example.com/",
            "https://fast.example.com/",
        ]);
        list.mirrors[0].latency_ms = 500;
        list.mirrors[1].latency_ms = 50;
        let best = list.best_mirror().unwrap();
        assert_eq!(best.url, "https://fast.example.com/"); // 50ms wins
    }

    #[test]
    fn test_mark_failed() {
        let mut list = MirrorList::from_urls(&[
            "https://a.example.com/",
            "https://b.example.com/",
        ]);
        assert_eq!(list.available_count(), 2);
        list.mark_failed("https://a.example.com/");
        assert_eq!(list.available_count(), 1);
        let best = list.best_mirror().unwrap();
        assert_eq!(best.url, "https://b.example.com/");
    }

    #[test]
    fn test_reset_failures() {
        let mut list = MirrorList::from_urls(&[
            "https://a.example.com/",
            "https://b.example.com/",
        ]);
        list.mark_failed("https://a.example.com/");
        list.mark_failed("https://b.example.com/");
        assert_eq!(list.available_count(), 0);
        list.reset_failures();
        assert_eq!(list.available_count(), 2);
    }

    #[test]
    fn test_mirror_url() {
        let mirror = Mirror {
            url: "https://mirror.example.com/releases/".into(),
            location: "US".into(),
            name: "Test".into(),
            priority: 1,
            latency_ms: 0,
            failed: false,
        };
        assert_eq!(
            MirrorList::mirror_url(&mirror, "ubuntu/22.04/ubuntu.iso"),
            "https://mirror.example.com/releases/ubuntu/22.04/ubuntu.iso"
        );
        // Double-slash prevention
        assert_eq!(
            MirrorList::mirror_url(&mirror, "/ubuntu.iso"),
            "https://mirror.example.com/releases/ubuntu.iso"
        );
    }

    #[test]
    fn test_mirror_list_serde() {
        let list = MirrorList::from_urls(&["https://a.example.com/"]);
        let json = serde_json::to_string(&list).unwrap();
        let parsed: MirrorList = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.mirrors.len(), 1);
        assert_eq!(parsed.mirrors[0].url, "https://a.example.com/");
    }

    #[test]
    fn test_parse_metalink_urls() {
        let metalink = r#"<?xml version="1.0" encoding="UTF-8"?>
<metalink xmlns="urn:ietf:params:xml:ns:metalink">
  <file name="ubuntu-24.04.iso">
    <size>5000000000</size>
    <hash type="sha-256">abcdef1234567890</hash>
    <url priority="1" location="US">https://us.mirror.com/ubuntu-24.04.iso</url>
    <url priority="2" location="DE">https://de.mirror.com/ubuntu-24.04.iso</url>
    <url priority="3" location="JP">https://jp.mirror.com/ubuntu-24.04.iso</url>
  </file>
</metalink>"#;

        let info = parse_metalink_urls(metalink).unwrap();
        assert_eq!(info.filename, "ubuntu-24.04.iso");
        assert_eq!(info.size, 5000000000);
        assert_eq!(info.sha256, Some("abcdef1234567890".into()));
        assert_eq!(info.mirrors.len(), 3);
        assert_eq!(info.mirrors[0].priority, 1);
        assert_eq!(info.mirrors[0].location, "US");
    }

    #[test]
    fn test_extract_attr() {
        assert_eq!(
            extract_attr(r#"<url priority="5" location="US">x</url>"#, "priority"),
            Some("5".into())
        );
        assert_eq!(
            extract_attr(r#"<url priority="5" location="US">x</url>"#, "location"),
            Some("US".into())
        );
        assert_eq!(extract_attr("<url>x</url>", "priority"), None);
    }

    #[test]
    fn test_extract_tag_content() {
        assert_eq!(
            extract_tag_content("<size>12345</size>", "size"),
            Some("12345".into())
        );
        assert_eq!(
            extract_tag_content("<hash type=\"sha-256\">abc</hash>", "hash"),
            Some("abc".into())
        );
    }

    #[test]
    fn test_default_mirror_list() {
        let list = MirrorList::default();
        assert!(list.mirrors.is_empty());
        assert_eq!(list.max_probe, 8);
        assert_eq!(list.probe_timeout_ms, 5000);
    }

    #[test]
    fn test_no_mirrors_best_returns_none() {
        let list = MirrorList::default();
        assert!(list.best_mirror().is_none());
    }

    #[test]
    fn test_all_failed_best_returns_none() {
        let mut list = MirrorList::from_urls(&["https://a.example.com/"]);
        list.mark_failed("https://a.example.com/");
        assert!(list.best_mirror().is_none());
    }
}
