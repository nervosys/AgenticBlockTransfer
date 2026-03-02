//! Configuration file support for abt.
//!
//! Loads user preferences from `~/.config/abt/config.toml` (Linux/macOS)
//! or `%APPDATA%\abt\config.toml` (Windows). All settings are optional
//! and serve as defaults that can be overridden by CLI flags.
//!
//! # Example config.toml
//!
//! ```toml
//! [write]
//! block_size = "4M"
//! verify = true
//! hash_algorithm = "sha256"
//! direct_io = true
//! sync = true
//! sparse = false
//!
//! [safety]
//! level = "low"              # "low", "medium", "high"
//! backup_partition_table = true
//! dry_run = false
//!
//! [output]
//! format = "text"           # "text", "json", "json-ld"
//! verbose = 2
//!
//! [logging]
//! log_file = ""             # empty = disabled
//! ```

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Top-level configuration structure.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub write: WriteConfig,
    pub safety: SafetyConfig,
    pub output: OutputConfig,
    pub logging: LoggingConfig,
}

/// Write operation defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WriteConfig {
    /// Default block size (e.g., "4M", "1M", "512K").
    pub block_size: String,
    /// Whether to verify after writing.
    pub verify: bool,
    /// Hash algorithm to use for verification.
    pub hash_algorithm: String,
    /// Use direct I/O (O_DIRECT / FILE_FLAG_NO_BUFFERING).
    pub direct_io: bool,
    /// Sync to device after writing.
    pub sync: bool,
    /// Enable sparse write (skip zero blocks).
    pub sparse: bool,
}

impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            block_size: "4M".to_string(),
            verify: true,
            hash_algorithm: "sha256".to_string(),
            direct_io: true,
            sync: true,
            sparse: false,
        }
    }
}

/// Safety defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SafetyConfig {
    /// Safety level: "low", "medium", "high".
    pub level: String,
    /// Whether to back up the partition table before writing.
    pub backup_partition_table: bool,
    /// Default to dry-run mode.
    pub dry_run: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            level: "low".to_string(),
            backup_partition_table: true,
            dry_run: false,
        }
    }
}

/// Output format defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OutputConfig {
    /// Default output format: "text", "json", "json-ld".
    pub format: String,
    /// Default verbosity level (0-4).
    pub verbose: u8,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            format: "text".to_string(),
            verbose: 2,
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Path to log file (empty = disabled).
    pub log_file: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            log_file: String::new(),
        }
    }
}

impl Config {
    /// Load configuration from the default platform-specific path.
    /// Returns `Config::default()` if the file does not exist.
    pub fn load() -> Result<Self> {
        if let Some(path) = config_path() {
            if path.exists() {
                return Self::load_from(&path);
            }
        }
        Ok(Self::default())
    }

    /// Load configuration from a specific file path.
    pub fn load_from(path: &Path) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
        log::info!("Loaded config from {}", path.display());
        Ok(config)
    }

    /// Write the default configuration to the platform-specific path.
    /// Creates the parent directory if needed.
    pub fn write_default() -> Result<PathBuf> {
        let path = config_path()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {}", parent.display()))?;
        }

        let default = Config::default();
        let toml_str = toml::to_string_pretty(&default)
            .context("Failed to serialize default config")?;

        fs::write(&path, &toml_str)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(path)
    }
}

/// Get the platform-specific configuration file path.
///
/// - Linux/macOS: `~/.config/abt/config.toml`
/// - Windows: `%APPDATA%\abt\config.toml`
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("abt").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid_toml() {
        let cfg = Config::default();
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.write.block_size, "4M");
        assert!(parsed.write.verify);
        assert_eq!(parsed.safety.level, "low");
    }

    #[test]
    fn partial_config_fills_defaults() {
        let partial = r#"
[write]
block_size = "1M"
sparse = true
"#;
        let cfg: Config = toml::from_str(partial).unwrap();
        assert_eq!(cfg.write.block_size, "1M");
        assert!(cfg.write.sparse);
        // Other fields should have defaults
        assert!(cfg.write.verify);
        assert!(cfg.write.direct_io);
        assert_eq!(cfg.safety.level, "low");
    }

    #[test]
    fn empty_config_is_all_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.write.block_size, "4M");
        assert!(cfg.write.verify);
        assert!(!cfg.write.sparse);
    }

    #[test]
    fn config_path_is_deterministic() {
        // Just verify it returns Some on all major platforms
        let path = config_path();
        // On CI or unusual environments this might be None, so we just test the join
        if let Some(p) = path {
            assert!(p.to_string_lossy().contains("config.toml"));
        }
    }

    #[test]
    fn roundtrip_serialization() {
        let cfg = Config {
            write: WriteConfig {
                block_size: "8M".to_string(),
                verify: false,
                hash_algorithm: "blake3".to_string(),
                direct_io: false,
                sync: false,
                sparse: true,
            },
            safety: SafetyConfig {
                level: "high".to_string(),
                backup_partition_table: false,
                dry_run: true,
            },
            output: OutputConfig {
                format: "json".to_string(),
                verbose: 4,
            },
            logging: LoggingConfig {
                log_file: "/tmp/abt.log".to_string(),
            },
        };
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.write.block_size, "8M");
        assert!(!parsed.write.verify);
        assert!(parsed.write.sparse);
        assert_eq!(parsed.safety.level, "high");
        assert!(parsed.safety.dry_run);
        assert_eq!(parsed.output.verbose, 4);
    }
}
