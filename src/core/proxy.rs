// Network proxy configuration for abt.
//
// Inspired by rpi-imager's CurlNetworkConfig: centralized proxy configuration
// that all HTTP clients (download, update check, catalog fetch, mirror probe)
// share. Supports HTTP/HTTPS/SOCKS5 proxies, no-proxy lists, and automatic
// detection from environment variables or config files.
//
// Corporate and education networks often require proxies — without this,
// abt's download features silently fail in those environments.

#![allow(dead_code)]

use anyhow::Result;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Proxy protocol type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyProtocol {
    Http,
    Https,
    Socks5,
    Socks5h,
    Direct,
}

impl std::fmt::Display for ProxyProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http => write!(f, "http"),
            Self::Https => write!(f, "https"),
            Self::Socks5 => write!(f, "socks5"),
            Self::Socks5h => write!(f, "socks5h"),
            Self::Direct => write!(f, "direct"),
        }
    }
}

/// Proxy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Proxy URL (e.g., "http://proxy.corp.com:8080")
    pub url: String,
    /// Proxy protocol
    pub protocol: ProxyProtocol,
    /// Proxy username (optional)
    pub username: Option<String>,
    /// Proxy password (optional)
    pub password: Option<String>,
    /// Hosts to bypass the proxy (e.g., "localhost,127.0.0.1,.local")
    pub no_proxy: Vec<String>,
}

impl ProxyConfig {
    /// Create a new proxy configuration from a URL string.
    pub fn from_url(url: &str) -> Result<Self> {
        let protocol = if url.starts_with("socks5h://") {
            ProxyProtocol::Socks5h
        } else if url.starts_with("socks5://") {
            ProxyProtocol::Socks5
        } else if url.starts_with("https://") {
            ProxyProtocol::Https
        } else if url.starts_with("http://") {
            ProxyProtocol::Http
        } else {
            anyhow::bail!("Unknown proxy protocol in URL: {}", url);
        };

        Ok(Self {
            url: url.to_string(),
            protocol,
            username: None,
            password: None,
            no_proxy: Vec::new(),
        })
    }

    /// Create a direct (no proxy) configuration.
    pub fn direct() -> Self {
        Self {
            url: String::new(),
            protocol: ProxyProtocol::Direct,
            username: None,
            password: None,
            no_proxy: Vec::new(),
        }
    }

    /// Set credentials.
    pub fn with_auth(mut self, username: &str, password: &str) -> Self {
        self.username = Some(username.to_string());
        self.password = Some(password.to_string());
        self
    }

    /// Set no-proxy list from a comma-separated string.
    pub fn with_no_proxy(mut self, no_proxy: &str) -> Self {
        self.no_proxy = no_proxy
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        self
    }

    /// Check if a host should bypass the proxy.
    pub fn should_bypass(&self, host: &str) -> bool {
        if self.protocol == ProxyProtocol::Direct {
            return true;
        }
        for pattern in &self.no_proxy {
            if pattern == "*" {
                return true;
            }
            if pattern.starts_with('.') && host.ends_with(pattern) {
                return true;
            }
            if host == pattern {
                return true;
            }
        }
        false
    }
}

/// Fetch profile — controls timeout/retry behavior for different request types.
/// Inspired by rpi-imager's Interactive vs FireAndForget profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchProfile {
    /// Interactive: user is waiting, shorter timeout, show progress.
    Interactive,
    /// Background: fire-and-forget, longer timeout, retry on failure.
    Background,
    /// Fast: quick probe (latency check, HEAD request), very short timeout.
    Fast,
}

impl FetchProfile {
    /// Connection timeout for this profile.
    pub fn connect_timeout(&self) -> Duration {
        match self {
            Self::Interactive => Duration::from_secs(30),
            Self::Background => Duration::from_secs(60),
            Self::Fast => Duration::from_secs(5),
        }
    }

    /// Total request timeout for this profile.
    pub fn request_timeout(&self) -> Duration {
        match self {
            Self::Interactive => Duration::from_secs(7200), // 2 hours for large downloads
            Self::Background => Duration::from_secs(3600),
            Self::Fast => Duration::from_secs(10),
        }
    }

    /// Max retries for this profile.
    pub fn max_retries(&self) -> u32 {
        match self {
            Self::Interactive => 2,
            Self::Background => 5,
            Self::Fast => 0,
        }
    }
}

/// Shared network configuration — singleton-like, thread-safe.
/// All HTTP operations should share this to get consistent proxy/timeout behavior.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    inner: Arc<RwLock<NetworkConfigInner>>,
}

#[derive(Debug)]
struct NetworkConfigInner {
    proxy: Option<ProxyConfig>,
    user_agent: String,
    max_concurrent_downloads: usize,
    ca_bundle_path: Option<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl NetworkConfig {
    /// Create a new default network configuration.
    ///
    /// Automatically detects proxy from environment variables:
    /// `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`.
    pub fn new() -> Self {
        let proxy = detect_proxy_from_env();
        if let Some(ref p) = proxy {
            info!("Proxy detected from environment: {} ({})", p.url, p.protocol);
        }

        Self {
            inner: Arc::new(RwLock::new(NetworkConfigInner {
                proxy,
                user_agent: format!("abt/{}", env!("CARGO_PKG_VERSION")),
                max_concurrent_downloads: 4,
                ca_bundle_path: None,
            })),
        }
    }

    /// Set an explicit proxy configuration.
    pub fn set_proxy(&self, proxy: ProxyConfig) {
        if let Ok(mut inner) = self.inner.write() {
            info!("Proxy configured: {} ({})", proxy.url, proxy.protocol);
            inner.proxy = Some(proxy);
        }
    }

    /// Clear proxy (use direct connections).
    pub fn clear_proxy(&self) {
        if let Ok(mut inner) = self.inner.write() {
            inner.proxy = None;
        }
    }

    /// Set a custom CA certificate bundle path.
    pub fn set_ca_bundle(&self, path: &str) {
        if let Ok(mut inner) = self.inner.write() {
            inner.ca_bundle_path = Some(path.to_string());
        }
    }

    /// Set maximum concurrent downloads.
    pub fn set_max_concurrent(&self, max: usize) {
        if let Ok(mut inner) = self.inner.write() {
            inner.max_concurrent_downloads = max;
        }
    }

    /// Get current proxy configuration, if any.
    pub fn proxy(&self) -> Option<ProxyConfig> {
        self.inner.read().ok().and_then(|inner| inner.proxy.clone())
    }

    /// Build a reqwest client with this network configuration applied.
    pub fn build_client(&self, profile: FetchProfile) -> Result<reqwest::Client> {
        let inner = self.inner.read().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;

        let mut builder = reqwest::Client::builder()
            .user_agent(&inner.user_agent)
            .connect_timeout(profile.connect_timeout())
            .timeout(profile.request_timeout());

        // Apply proxy configuration
        if let Some(ref proxy_config) = inner.proxy {
            if proxy_config.protocol != ProxyProtocol::Direct {
                debug!("Configuring proxy: {}", proxy_config.url);
                let mut proxy = reqwest::Proxy::all(&proxy_config.url)?;
                if let (Some(ref user), Some(ref pass)) = (&proxy_config.username, &proxy_config.password) {
                    proxy = proxy.basic_auth(user, pass);
                }
                builder = builder.proxy(proxy);

                // Configure no-proxy
                if !proxy_config.no_proxy.is_empty() {
                    std::env::set_var("NO_PROXY", &proxy_config.no_proxy.join(","));
                    builder = builder.no_proxy();
                }
            } else {
                builder = builder.no_proxy();
            }
        }

        Ok(builder.build()?)
    }

    /// Get the user agent string.
    pub fn user_agent(&self) -> String {
        self.inner.read().map(|inner| inner.user_agent.clone()).unwrap_or_default()
    }

    /// Get max concurrent downloads.
    pub fn max_concurrent(&self) -> usize {
        self.inner.read().map(|inner| inner.max_concurrent_downloads).unwrap_or(4)
    }

    /// Export current configuration as TOML-compatible structure.
    pub fn to_config_section(&self) -> ProxyConfigSection {
        let inner = self.inner.read().unwrap();
        ProxyConfigSection {
            proxy_url: inner.proxy.as_ref().map(|p| p.url.clone()),
            no_proxy: inner.proxy.as_ref().map(|p| p.no_proxy.join(",")),
            max_concurrent_downloads: Some(inner.max_concurrent_downloads),
            ca_bundle: inner.ca_bundle_path.clone(),
        }
    }
}

/// Config file section for network/proxy settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyConfigSection {
    pub proxy_url: Option<String>,
    pub no_proxy: Option<String>,
    pub max_concurrent_downloads: Option<usize>,
    pub ca_bundle: Option<String>,
}

/// Detect proxy from environment variables.
///
/// Checks (in order): `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`,
/// and their lowercase variants. `NO_PROXY` is applied as the bypass list.
pub fn detect_proxy_from_env() -> Option<ProxyConfig> {
    let proxy_url = std::env::var("HTTPS_PROXY")
        .or_else(|_| std::env::var("https_proxy"))
        .or_else(|_| std::env::var("HTTP_PROXY"))
        .or_else(|_| std::env::var("http_proxy"))
        .or_else(|_| std::env::var("ALL_PROXY"))
        .or_else(|_| std::env::var("all_proxy"))
        .ok()?;

    if proxy_url.is_empty() {
        return None;
    }

    let mut config = ProxyConfig::from_url(&proxy_url).ok()?;

    // Apply NO_PROXY
    if let Ok(no_proxy) = std::env::var("NO_PROXY").or_else(|_| std::env::var("no_proxy")) {
        config = config.with_no_proxy(&no_proxy);
    }

    Some(config)
}

/// Validate that a proxy URL is syntactically correct and reachable.
pub async fn validate_proxy(proxy_url: &str) -> Result<ProxyValidation> {
    let config = ProxyConfig::from_url(proxy_url)?;
    let net = NetworkConfig::new();
    net.set_proxy(config);

    let client = net.build_client(FetchProfile::Fast)?;

    let start = std::time::Instant::now();
    match client.get("https://httpbin.org/ip").send().await {
        Ok(resp) => {
            let latency = start.elapsed();
            Ok(ProxyValidation {
                valid: resp.status().is_success(),
                latency_ms: latency.as_millis() as u64,
                external_ip: resp.text().await.ok(),
                error: None,
            })
        }
        Err(e) => {
            warn!("Proxy validation failed: {}", e);
            Ok(ProxyValidation {
                valid: false,
                latency_ms: 0,
                external_ip: None,
                error: Some(e.to_string()),
            })
        }
    }
}

/// Result of proxy validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyValidation {
    pub valid: bool,
    pub latency_ms: u64,
    pub external_ip: Option<String>,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_from_url_http() {
        let config = ProxyConfig::from_url("http://proxy.corp.com:8080").unwrap();
        assert_eq!(config.protocol, ProxyProtocol::Http);
        assert_eq!(config.url, "http://proxy.corp.com:8080");
    }

    #[test]
    fn test_proxy_from_url_socks5() {
        let config = ProxyConfig::from_url("socks5://localhost:1080").unwrap();
        assert_eq!(config.protocol, ProxyProtocol::Socks5);
    }

    #[test]
    fn test_proxy_from_url_socks5h() {
        let config = ProxyConfig::from_url("socks5h://proxy.local:1080").unwrap();
        assert_eq!(config.protocol, ProxyProtocol::Socks5h);
    }

    #[test]
    fn test_proxy_from_url_invalid() {
        assert!(ProxyConfig::from_url("ftp://proxy.com:21").is_err());
    }

    #[test]
    fn test_proxy_direct() {
        let config = ProxyConfig::direct();
        assert_eq!(config.protocol, ProxyProtocol::Direct);
        assert!(config.url.is_empty());
    }

    #[test]
    fn test_proxy_with_auth() {
        let config = ProxyConfig::from_url("http://proxy:8080")
            .unwrap()
            .with_auth("user", "pass");
        assert_eq!(config.username.as_deref(), Some("user"));
        assert_eq!(config.password.as_deref(), Some("pass"));
    }

    #[test]
    fn test_no_proxy_list() {
        let config = ProxyConfig::from_url("http://proxy:8080")
            .unwrap()
            .with_no_proxy("localhost, 127.0.0.1, .local, .corp.com");
        assert_eq!(config.no_proxy.len(), 4);
        assert!(config.no_proxy.contains(&"localhost".to_string()));
        assert!(config.no_proxy.contains(&".corp.com".to_string()));
    }

    #[test]
    fn test_should_bypass_exact() {
        let config = ProxyConfig::from_url("http://proxy:8080")
            .unwrap()
            .with_no_proxy("localhost,192.168.1.1");
        assert!(config.should_bypass("localhost"));
        assert!(config.should_bypass("192.168.1.1"));
        assert!(!config.should_bypass("example.com"));
    }

    #[test]
    fn test_should_bypass_domain_suffix() {
        let config = ProxyConfig::from_url("http://proxy:8080")
            .unwrap()
            .with_no_proxy(".local,.corp.com");
        assert!(config.should_bypass("myhost.local"));
        assert!(config.should_bypass("git.corp.com"));
        assert!(!config.should_bypass("example.com"));
    }

    #[test]
    fn test_should_bypass_wildcard() {
        let config = ProxyConfig::from_url("http://proxy:8080")
            .unwrap()
            .with_no_proxy("*");
        assert!(config.should_bypass("anything.example.com"));
    }

    #[test]
    fn test_direct_always_bypasses() {
        let config = ProxyConfig::direct();
        assert!(config.should_bypass("anything.example.com"));
    }

    #[test]
    fn test_fetch_profile_interactive() {
        let p = FetchProfile::Interactive;
        assert_eq!(p.connect_timeout(), Duration::from_secs(30));
        assert_eq!(p.request_timeout(), Duration::from_secs(7200));
        assert_eq!(p.max_retries(), 2);
    }

    #[test]
    fn test_fetch_profile_background() {
        let p = FetchProfile::Background;
        assert_eq!(p.connect_timeout(), Duration::from_secs(60));
        assert_eq!(p.max_retries(), 5);
    }

    #[test]
    fn test_fetch_profile_fast() {
        let p = FetchProfile::Fast;
        assert_eq!(p.connect_timeout(), Duration::from_secs(5));
        assert_eq!(p.request_timeout(), Duration::from_secs(10));
        assert_eq!(p.max_retries(), 0);
    }

    #[test]
    fn test_network_config_default() {
        let config = NetworkConfig::new();
        assert_eq!(config.max_concurrent(), 4);
        assert!(config.user_agent().starts_with("abt/"));
    }

    #[test]
    fn test_network_config_set_proxy() {
        let config = NetworkConfig::new();
        config.set_proxy(ProxyConfig::from_url("http://test:8080").unwrap());
        let proxy = config.proxy().unwrap();
        assert_eq!(proxy.url, "http://test:8080");
    }

    #[test]
    fn test_network_config_clear_proxy() {
        let config = NetworkConfig::new();
        config.set_proxy(ProxyConfig::from_url("http://test:8080").unwrap());
        config.clear_proxy();
        assert!(config.proxy().is_none());
    }

    #[test]
    fn test_network_config_build_client() {
        let config = NetworkConfig::new();
        let client = config.build_client(FetchProfile::Interactive);
        assert!(client.is_ok());
    }

    #[test]
    fn test_network_config_build_client_with_proxy() {
        let config = NetworkConfig::new();
        config.set_proxy(ProxyConfig::from_url("http://test:8080").unwrap());
        let client = config.build_client(FetchProfile::Fast);
        assert!(client.is_ok());
    }

    #[test]
    fn test_config_section_roundtrip() {
        let config = NetworkConfig::new();
        config.set_proxy(
            ProxyConfig::from_url("http://proxy:3128")
                .unwrap()
                .with_no_proxy("localhost,.local"),
        );
        let section = config.to_config_section();
        assert_eq!(section.proxy_url.as_deref(), Some("http://proxy:3128"));
        assert_eq!(section.no_proxy.as_deref(), Some("localhost,.local"));
        assert_eq!(section.max_concurrent_downloads, Some(4));
    }

    #[test]
    fn test_proxy_protocol_display() {
        assert_eq!(ProxyProtocol::Http.to_string(), "http");
        assert_eq!(ProxyProtocol::Socks5h.to_string(), "socks5h");
        assert_eq!(ProxyProtocol::Direct.to_string(), "direct");
    }
}
