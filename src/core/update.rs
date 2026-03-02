// Self-update / version check module — check for new releases of abt.
//
// Inspired by Rufus's CheckForUpdatesThread which contacts a server on a
// configurable interval and compares server version to local version.
//
// Supports:
//   - GitHub Releases API version checking
//   - Configurable update check interval (default: once per 24 hours)
//   - Semantic version comparison
//   - Release notes preview
//   - Download URL extraction for the matching platform/architecture

use anyhow::{Context, Result};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Current version of abt (from Cargo.toml at compile time).
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default GitHub repository for update checks.
const DEFAULT_REPO: &str = "nervosys/AgenticBlockTransfer";

/// Default update check interval (24 hours in seconds).
const DEFAULT_CHECK_INTERVAL_SECS: u64 = 86400;

/// GitHub release information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    /// Release tag (e.g., "v1.2.0").
    pub tag_name: String,
    /// Release title / name.
    pub name: String,
    /// Release notes body (Markdown).
    pub body: String,
    /// Whether this is a pre-release.
    pub prerelease: bool,
    /// Whether this is a draft.
    pub draft: bool,
    /// ISO 8601 publish date.
    pub published_at: String,
    /// HTML URL to the release page.
    pub html_url: String,
    /// Downloadable assets.
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

/// A downloadable release asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAsset {
    /// Asset filename (e.g., "abt-1.2.0-x86_64-unknown-linux-musl.tar.gz").
    pub name: String,
    /// Direct download URL.
    pub browser_download_url: String,
    /// File size in bytes.
    pub size: u64,
    /// Content type.
    pub content_type: String,
    /// Download count.
    pub download_count: u64,
}

/// Result of a version check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckResult {
    /// Current local version.
    pub current_version: String,
    /// Latest available version (tag without 'v' prefix).
    pub latest_version: String,
    /// Whether an update is available.
    pub update_available: bool,
    /// Whether the update is a pre-release.
    pub is_prerelease: bool,
    /// Release notes excerpt (first 500 chars).
    pub release_notes: String,
    /// URL to the release page.
    pub release_url: String,
    /// Download URL for the current platform (if found).
    pub download_url: Option<String>,
    /// Timestamp of this check.
    pub checked_at: String,
}

/// Persistent state for update checking (stored in config dir).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCheckState {
    /// Timestamp of the last successful check (UNIX epoch seconds).
    pub last_check_at: u64,
    /// The latest version seen.
    pub latest_version: String,
    /// Whether the user dismissed the update notification for this version.
    pub dismissed_version: Option<String>,
}

impl Default for UpdateCheckState {
    fn default() -> Self {
        Self {
            last_check_at: 0,
            latest_version: CURRENT_VERSION.to_string(),
            dismissed_version: None,
        }
    }
}

/// Update check options.
#[derive(Debug, Clone)]
pub struct UpdateCheckOpts {
    /// GitHub repository (owner/repo).
    pub repo: String,
    /// Include pre-releases in the check.
    pub include_prerelease: bool,
    /// Minimum interval between automatic checks (seconds).
    pub check_interval_secs: u64,
    /// Force a check regardless of interval.
    pub force: bool,
    /// Path to the state file (default: ~/.config/abt/update-state.json).
    pub state_path: PathBuf,
}

impl Default for UpdateCheckOpts {
    fn default() -> Self {
        let state_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("abt")
            .join("update-state.json");

        Self {
            repo: DEFAULT_REPO.to_string(),
            include_prerelease: false,
            check_interval_secs: DEFAULT_CHECK_INTERVAL_SECS,
            force: false,
            state_path,
        }
    }
}

/// Load update check state from disk.
fn load_state(path: &Path) -> UpdateCheckState {
    if let Ok(data) = std::fs::read_to_string(path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        UpdateCheckState::default()
    }
}

/// Save update check state to disk.
fn save_state(path: &Path, state: &UpdateCheckState) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(state)?;
    std::fs::write(path, data)?;
    Ok(())
}

/// Parse a semantic version string (with optional 'v' prefix) into (major, minor, patch).
pub fn parse_semver(version: &str) -> Option<(u32, u32, u32)> {
    let v = version.strip_prefix('v').unwrap_or(version);
    let core = v.split('-').next().unwrap_or(v);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let major = parts[0].parse().ok()?;
    let minor = parts[1].parse().ok()?;
    let patch = parts[2].parse().ok()?;
    Some((major, minor, patch))
}

/// Compare two semantic version strings. Returns true if `remote` is newer than `local`.
pub fn is_newer(local: &str, remote: &str) -> bool {
    match (parse_semver(local), parse_semver(remote)) {
        (Some((lmaj, lmin, lpat)), Some((rmaj, rmin, rpat))) => {
            (rmaj, rmin, rpat) > (lmaj, lmin, lpat)
        }
        _ => false,
    }
}

/// Check for updates by querying the GitHub Releases API.
pub async fn check_for_updates(opts: &UpdateCheckOpts) -> Result<UpdateCheckResult> {
    // Check interval (skip if checked recently, unless forced)
    if !opts.force {
        let state = load_state(&opts.state_path);
        let now = chrono::Utc::now().timestamp() as u64;
        if now - state.last_check_at < opts.check_interval_secs {
            info!(
                "Update check skipped: last checked {}s ago (interval: {}s)",
                now - state.last_check_at,
                opts.check_interval_secs
            );
            return Ok(UpdateCheckResult {
                current_version: CURRENT_VERSION.to_string(),
                latest_version: state.latest_version.clone(),
                update_available: is_newer(CURRENT_VERSION, &state.latest_version),
                is_prerelease: false,
                release_notes: String::new(),
                release_url: String::new(),
                download_url: None,
                checked_at: chrono::Utc::now().to_rfc3339(),
            });
        }
    }

    info!("Checking for updates: {}", opts.repo);

    let api_url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        opts.repo
    );

    let client = reqwest::Client::builder()
        .user_agent(format!("abt/{}", CURRENT_VERSION))
        .timeout(Duration::from_secs(15))
        .build()?;

    let resp = client
        .get(&api_url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await?;

    if !resp.status().is_success() {
        anyhow::bail!(
            "GitHub API request failed: HTTP {}",
            resp.status().as_u16()
        );
    }

    let release: ReleaseInfo = resp
        .json::<ReleaseInfo>()
        .await
        .context("Failed to parse GitHub release JSON")?;

    // Skip pre-releases unless opted in
    if release.prerelease && !opts.include_prerelease {
        info!("Latest release {} is a pre-release, skipping", release.tag_name);
        return Ok(UpdateCheckResult {
            current_version: CURRENT_VERSION.to_string(),
            latest_version: CURRENT_VERSION.to_string(),
            update_available: false,
            is_prerelease: true,
            release_notes: String::new(),
            release_url: release.html_url,
            download_url: None,
            checked_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    let remote_version = release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name);
    let update_available = is_newer(CURRENT_VERSION, remote_version);

    // Find platform-specific download URL
    let download_url = find_platform_asset(&release.assets);

    // Truncate release notes for display
    let release_notes = if release.body.len() > 500 {
        format!("{}...", &release.body[..500])
    } else {
        release.body.clone()
    };

    // Update state
    let state = UpdateCheckState {
        last_check_at: chrono::Utc::now().timestamp() as u64,
        latest_version: remote_version.to_string(),
        dismissed_version: load_state(&opts.state_path).dismissed_version,
    };
    if let Err(e) = save_state(&opts.state_path, &state) {
        warn!("Failed to save update state: {}", e);
    }

    let result = UpdateCheckResult {
        current_version: CURRENT_VERSION.to_string(),
        latest_version: remote_version.to_string(),
        update_available,
        is_prerelease: release.prerelease,
        release_notes,
        release_url: release.html_url,
        download_url,
        checked_at: chrono::Utc::now().to_rfc3339(),
    };

    if update_available {
        info!(
            "Update available: {} -> {}",
            CURRENT_VERSION, remote_version
        );
    } else {
        info!("abt is up to date ({})", CURRENT_VERSION);
    }

    Ok(result)
}

/// Find a release asset matching the current platform and architecture.
fn find_platform_asset(assets: &[ReleaseAsset]) -> Option<String> {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "freebsd") {
        "freebsd"
    } else {
        return None;
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86") {
        "i686"
    } else if cfg!(target_arch = "arm") {
        "armv7"
    } else {
        return None;
    };

    // Try exact match first, then partial matches
    let name_lower: Vec<String> = assets.iter().map(|a| a.name.to_lowercase()).collect();

    // Exact platform+arch (e.g., "abt-1.2.0-x86_64-unknown-linux-musl.tar.gz")
    for (i, name) in name_lower.iter().enumerate() {
        if name.contains(os) && name.contains(arch) {
            return Some(assets[i].browser_download_url.clone());
        }
    }

    // OS-only match (e.g., "abt-1.2.0-windows.zip")
    for (i, name) in name_lower.iter().enumerate() {
        if name.contains(os) {
            return Some(assets[i].browser_download_url.clone());
        }
    }

    None
}

/// Dismiss the update notification for a specific version.
pub fn dismiss_update(version: &str, state_path: &Path) -> Result<()> {
    let mut state = load_state(state_path);
    state.dismissed_version = Some(version.to_string());
    save_state(state_path, &state)
}

/// Check if an update was dismissed for a specific version.
pub fn is_update_dismissed(version: &str, state_path: &Path) -> bool {
    let state = load_state(state_path);
    state.dismissed_version.as_deref() == Some(version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_semver() {
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.0.1"), Some((0, 0, 1)));
        assert_eq!(parse_semver("v10.20.30"), Some((10, 20, 30)));
    }

    #[test]
    fn test_parse_semver_with_prerelease() {
        assert_eq!(parse_semver("1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_semver("v2.0.0-rc.1"), Some((2, 0, 0)));
    }

    #[test]
    fn test_parse_semver_invalid() {
        assert_eq!(parse_semver("not-a-version"), None);
        assert_eq!(parse_semver("1.2"), None);
        assert_eq!(parse_semver(""), None);
        assert_eq!(parse_semver("1.2.3.4"), None);
    }

    #[test]
    fn test_is_newer() {
        assert!(is_newer("1.0.0", "1.0.1"));
        assert!(is_newer("1.0.0", "1.1.0"));
        assert!(is_newer("1.0.0", "2.0.0"));
        assert!(is_newer("1.1.0", "1.2.0"));
        assert!(!is_newer("1.2.0", "1.2.0")); // same
        assert!(!is_newer("1.2.0", "1.1.0")); // older
        assert!(!is_newer("2.0.0", "1.9.9")); // older
    }

    #[test]
    fn test_is_newer_with_v_prefix() {
        assert!(is_newer("v1.0.0", "v1.0.1"));
        assert!(is_newer("1.0.0", "v2.0.0"));
        assert!(is_newer("v1.0.0", "2.0.0"));
    }

    #[test]
    fn test_is_newer_invalid() {
        assert!(!is_newer("invalid", "1.0.0"));
        assert!(!is_newer("1.0.0", "invalid"));
    }

    #[test]
    fn test_update_check_state_default() {
        let state = UpdateCheckState::default();
        assert_eq!(state.last_check_at, 0);
        assert_eq!(state.latest_version, CURRENT_VERSION);
        assert!(state.dismissed_version.is_none());
    }

    #[test]
    fn test_update_check_state_serde() {
        let state = UpdateCheckState {
            last_check_at: 1700000000,
            latest_version: "1.5.0".into(),
            dismissed_version: Some("1.4.0".into()),
        };
        let json = serde_json::to_string(&state).unwrap();
        let parsed: UpdateCheckState = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.last_check_at, 1700000000);
        assert_eq!(parsed.latest_version, "1.5.0");
        assert_eq!(parsed.dismissed_version, Some("1.4.0".into()));
    }

    #[test]
    fn test_save_and_load_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-state.json");
        let state = UpdateCheckState {
            last_check_at: 42,
            latest_version: "9.9.9".into(),
            dismissed_version: None,
        };
        save_state(&path, &state).unwrap();
        let loaded = load_state(&path);
        assert_eq!(loaded.last_check_at, 42);
        assert_eq!(loaded.latest_version, "9.9.9");
    }

    #[test]
    fn test_load_state_missing_file() {
        let state = load_state(Path::new("/nonexistent/path/update.json"));
        assert_eq!(state.last_check_at, 0);
    }

    #[test]
    fn test_update_check_opts_default() {
        let opts = UpdateCheckOpts::default();
        assert_eq!(opts.repo, "nervosys/AgenticBlockTransfer");
        assert_eq!(opts.check_interval_secs, 86400);
        assert!(!opts.include_prerelease);
        assert!(!opts.force);
    }

    #[test]
    fn test_release_info_serde() {
        let json = r#"{
            "tag_name": "v1.2.0",
            "name": "Version 1.2.0",
            "body": "Changelog: Feature A, Fix B",
            "prerelease": false,
            "draft": false,
            "published_at": "2025-01-01T00:00:00Z",
            "html_url": "https://github.com/test/repo/releases/tag/v1.2.0",
            "assets": [{
                "name": "abt-1.2.0-x86_64-unknown-linux-musl.tar.gz",
                "browser_download_url": "https://github.com/test/repo/releases/download/v1.2.0/abt.tar.gz",
                "size": 5000000,
                "content_type": "application/gzip",
                "download_count": 100
            }]
        }"#;

        let release: ReleaseInfo = serde_json::from_str(json).unwrap();
        assert_eq!(release.tag_name, "v1.2.0");
        assert!(!release.prerelease);
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].size, 5000000);
    }

    #[test]
    fn test_find_platform_asset() {
        let assets = vec![
            ReleaseAsset {
                name: "abt-1.2.0-x86_64-unknown-linux-musl.tar.gz".into(),
                browser_download_url: "https://example.com/linux.tar.gz".into(),
                size: 1000,
                content_type: "application/gzip".into(),
                download_count: 0,
            },
            ReleaseAsset {
                name: "abt-1.2.0-windows-x86_64.zip".into(),
                browser_download_url: "https://example.com/windows.zip".into(),
                size: 2000,
                content_type: "application/zip".into(),
                download_count: 0,
            },
            ReleaseAsset {
                name: "abt-1.2.0-aarch64-apple-macos.tar.gz".into(),
                browser_download_url: "https://example.com/macos.tar.gz".into(),
                size: 1500,
                content_type: "application/gzip".into(),
                download_count: 0,
            },
        ];

        let result = find_platform_asset(&assets);
        // Result depends on compile-time target
        assert!(result.is_some());
    }

    #[test]
    fn test_dismiss_and_check_update() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("update-state.json");

        assert!(!is_update_dismissed("1.5.0", &path));
        dismiss_update("1.5.0", &path).unwrap();
        assert!(is_update_dismissed("1.5.0", &path));
        assert!(!is_update_dismissed("1.6.0", &path));
    }

    #[test]
    fn test_update_check_result_serde() {
        let result = UpdateCheckResult {
            current_version: "1.0.0".into(),
            latest_version: "1.1.0".into(),
            update_available: true,
            is_prerelease: false,
            release_notes: "Bug fixes".into(),
            release_url: "https://github.com/test/releases/v1.1.0".into(),
            download_url: Some("https://example.com/abt.tar.gz".into()),
            checked_at: "2025-01-01T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&result).unwrap();
        let parsed: UpdateCheckResult = serde_json::from_str(&json).unwrap();
        assert!(parsed.update_available);
        assert_eq!(parsed.latest_version, "1.1.0");
    }
}
