// Safety module — pre-flight checks, dry-run, device fingerprinting
//
// This is what makes abt fundamentally safer than dd for both humans and AI agents.
// dd will silently destroy any target with zero validation. abt runs a comprehensive
// pre-flight safety analysis before any destructive operation, producing a structured
// machine-readable report that agents can parse and act on.

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

use super::device::{create_enumerator, DeviceInfo};
use super::image;
use super::types::ImageSource;

// ── Safety levels ──────────────────────────────────────────────────────────────

/// How strict should abt be? Higher levels add more checks and require more
/// explicit confirmation before proceeding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SafetyLevel {
    /// Minimal checks — equivalent to dd with a seatbelt.
    /// Still blocks system drive writes unless --force.
    Low,
    /// Recommended for agent use. All Low checks plus:
    /// - Requires removable media or explicit device confirmation token
    /// - Validates image fits on device
    /// - Checks source integrity if hash provided
    /// - Refuses unmounted-but-in-use devices
    Medium,
    /// Maximum safety. All Medium checks plus:
    /// - Backs up partition table before write
    /// - Requires device confirmation token (no interactive prompt fallback)
    /// - Refuses to write to any non-removable device
    /// - Logs full operation plan to file before executing
    High,
}

impl Default for SafetyLevel {
    fn default() -> Self {
        Self::Low
    }
}

impl std::fmt::Display for SafetyLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
        }
    }
}

impl std::str::FromStr for SafetyLevel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "low" | "normal" => Ok(Self::Low),
            "medium" | "cautious" | "agent" => Ok(Self::Medium),
            "high" | "paranoid" | "max" => Ok(Self::High),
            _ => Err(format!(
                "Unknown safety level: '{}'. Use: low, medium, high",
                s
            )),
        }
    }
}

// ── Device fingerprint ─────────────────────────────────────────────────────────

/// A cryptographic-ish identity token for a device, so agents can confirm they're
/// writing to the exact device they inspected. This prevents TOCTOU races between
/// `abt list` and `abt write`.
///
/// An agent workflow:
///   1. `abt list --json` → gets devices with fingerprints
///   2. Agent selects device, extracts fingerprint
///   3. `abt write -i image.iso -o /dev/sdb --confirm-token <fingerprint>`
///   4. abt verifies the device still matches the fingerprint before writing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceFingerprint {
    /// Device path at time of enumeration
    pub path: String,
    /// Device model/name
    pub name: String,
    /// Serial number (if available)
    pub serial: Option<String>,
    /// Device size in bytes
    pub size: u64,
    /// Whether device was removable at enumeration time
    pub removable: bool,
    /// Whether device was a system drive at enumeration time
    pub is_system: bool,
    /// Hex-encoded hash of the above fields
    pub token: String,
}

impl DeviceFingerprint {
    /// Create a fingerprint from a DeviceInfo.
    ///
    /// # FIPS Compliance
    /// In FIPS mode, uses SHA-256 (FIPS 180-4 approved) instead of BLAKE3.
    /// BLAKE3 is not NIST-approved and must not be used for integrity-relevant
    /// operations in federal / DoD environments.
    ///
    /// In default mode, BLAKE3 is used for performance (faster than SHA-256).
    pub fn from_device(dev: &DeviceInfo) -> Self {
        let token = if super::compliance::is_fips_mode() {
            // SP 800-131A / FIPS 180-4: Use SHA-256
            super::compliance::fips_device_token(
                &dev.path,
                &dev.name,
                dev.serial.as_deref(),
                dev.size,
                dev.removable,
                dev.is_system,
            )
        } else {
            // Default: BLAKE3 (fast, not FIPS-approved)
            let mut hasher = blake3::Hasher::new();
            hasher.update(dev.path.as_bytes());
            hasher.update(dev.name.as_bytes());
            if let Some(ref s) = dev.serial {
                hasher.update(s.as_bytes());
            }
            hasher.update(&dev.size.to_le_bytes());
            hasher.update(&[dev.removable as u8, dev.is_system as u8]);
            let hash = hasher.finalize();
            hex::encode(&hash.as_bytes()[..16]) // 128-bit token
        };

        Self {
            path: dev.path.clone(),
            name: dev.name.clone(),
            serial: dev.serial.clone(),
            size: dev.size,
            removable: dev.removable,
            is_system: dev.is_system,
            token,
        }
    }

    /// Verify that a live device still matches this fingerprint.
    #[allow(dead_code)]
    pub fn matches(&self, dev: &DeviceInfo) -> bool {
        let live = Self::from_device(dev);
        self.token == live.token
    }

    /// Encode to a compact string suitable for CLI --confirm-token.
    #[allow(dead_code)]
    pub fn to_token_string(&self) -> String {
        self.token.clone()
    }
}

// ── Safety check results ───────────────────────────────────────────────────────

/// Individual safety check result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyCheck {
    /// Machine-readable check identifier
    pub id: String,
    /// Human-readable description
    pub description: String,
    /// Whether this check passed
    pub passed: bool,
    /// Severity if failed: "error" blocks the operation, "warning" does not
    pub severity: CheckSeverity,
    /// Detail message (especially useful when failed)
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckSeverity {
    /// Informational — always passes
    Info,
    /// Warning — does not block, but agent/user should acknowledge
    Warning,
    /// Error — blocks the operation
    Error,
}

impl std::fmt::Display for CheckSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Info => write!(f, "INFO"),
            Self::Warning => write!(f, "WARN"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// Complete pre-flight safety report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyReport {
    /// Overall go/no-go decision
    pub safe_to_proceed: bool,
    /// Safety level that was applied
    pub safety_level: SafetyLevel,
    /// All individual checks that were run
    pub checks: Vec<SafetyCheck>,
    /// Device fingerprint (if device was found)
    pub device_fingerprint: Option<DeviceFingerprint>,
    /// Summary counts
    pub errors: usize,
    pub warnings: usize,
    /// Was this a dry-run?
    pub dry_run: bool,
}

impl SafetyReport {
    /// Emit the report as structured JSON (for agents).
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "safe_to_proceed": self.safe_to_proceed,
            "safety_level": format!("{}", self.safety_level),
            "dry_run": self.dry_run,
            "errors": self.errors,
            "warnings": self.warnings,
            "checks": self.checks.iter().map(|c| json!({
                "id": c.id,
                "description": c.description,
                "passed": c.passed,
                "severity": format!("{}", c.severity),
                "detail": c.detail,
            })).collect::<Vec<_>>(),
            "device_fingerprint": self.device_fingerprint.as_ref().map(|fp| json!({
                "path": fp.path,
                "name": fp.name,
                "serial": fp.serial,
                "size": fp.size,
                "removable": fp.removable,
                "is_system": fp.is_system,
                "token": fp.token,
            })),
        })
    }

    /// Print a human-readable summary to stderr.
    pub fn print_human(&self) {
        eprintln!();
        eprintln!("  ── Pre-flight Safety Report ──");
        eprintln!("  Safety level: {}", self.safety_level);
        eprintln!();

        for check in &self.checks {
            let icon = if check.passed {
                "✓"
            } else {
                match check.severity {
                    CheckSeverity::Error => "✗",
                    CheckSeverity::Warning => "⚠",
                    CheckSeverity::Info => "·",
                }
            };
            let severity = if check.passed {
                "OK".to_string()
            } else {
                format!("{}", check.severity)
            };
            eprintln!(
                "  {} [{}] {} — {}",
                icon, severity, check.description, check.detail
            );
        }

        eprintln!();
        if self.safe_to_proceed {
            eprintln!("  Result: SAFE TO PROCEED ({} warnings)", self.warnings);
        } else {
            eprintln!(
                "  Result: BLOCKED ({} errors, {} warnings)",
                self.errors, self.warnings
            );
        }

        if let Some(ref fp) = self.device_fingerprint {
            eprintln!("  Device token: {}", fp.token);
        }
        eprintln!();
    }
}

// ── Pre-flight check engine ────────────────────────────────────────────────────

/// Run all pre-flight safety checks for a write operation.
pub async fn preflight_check(
    source: &ImageSource,
    target: &str,
    safety_level: SafetyLevel,
    confirm_token: Option<&str>,
    dry_run: bool,
) -> anyhow::Result<SafetyReport> {
    let mut checks: Vec<SafetyCheck> = Vec::new();

    // ── 1. Source exists and is readable ────────────────────────────────────
    match source {
        ImageSource::File(path) => {
            let exists = path.exists();
            checks.push(SafetyCheck {
                id: "source_exists".into(),
                description: "Source image exists".into(),
                passed: exists,
                severity: CheckSeverity::Error,
                detail: if exists {
                    format!("{}", path.display())
                } else {
                    format!("File not found: {}", path.display())
                },
            });

            if exists {
                // Check readable
                let readable = std::fs::File::open(path).is_ok();
                checks.push(SafetyCheck {
                    id: "source_readable".into(),
                    description: "Source image is readable".into(),
                    passed: readable,
                    severity: CheckSeverity::Error,
                    detail: if readable {
                        "OK".into()
                    } else {
                        "Permission denied or file locked".into()
                    },
                });

                // Detect format
                if let Ok(format) = image::detect_format(path) {
                    checks.push(SafetyCheck {
                        id: "source_format".into(),
                        description: "Image format detected".into(),
                        passed: true,
                        severity: CheckSeverity::Info,
                        detail: format!(
                            "{} ({})",
                            format,
                            if format.is_compressed() {
                                "compressed, will auto-decompress"
                            } else {
                                "uncompressed"
                            }
                        ),
                    });
                }

                // Source size
                if let Ok(meta) = std::fs::metadata(path) {
                    checks.push(SafetyCheck {
                        id: "source_size".into(),
                        description: "Source image size".into(),
                        passed: true,
                        severity: CheckSeverity::Info,
                        detail: humansize::format_size(meta.len(), humansize::BINARY),
                    });
                }
            }
        }
        ImageSource::Url(url) => {
            checks.push(SafetyCheck {
                id: "source_url".into(),
                description: "Source is a URL".into(),
                passed: url.starts_with("https://"),
                severity: if url.starts_with("https://") {
                    CheckSeverity::Info
                } else {
                    CheckSeverity::Warning
                },
                detail: if url.starts_with("https://") {
                    format!("{} (HTTPS)", url)
                } else if url.starts_with("http://") {
                    format!("{} (WARNING: unencrypted HTTP)", url)
                } else {
                    format!("{} (unknown protocol)", url)
                },
            });
        }
        ImageSource::Stdin => {
            checks.push(SafetyCheck {
                id: "source_stdin".into(),
                description: "Source is stdin".into(),
                passed: true,
                severity: CheckSeverity::Warning,
                detail: "Reading from stdin — cannot verify size or integrity before write".into(),
            });
        }
    }

    // ── 2. Target device checks ────────────────────────────────────────────
    let enumerator = create_enumerator();
    let device_result = enumerator.get_device(target).await;

    let device = match device_result {
        Ok(dev) => {
            checks.push(SafetyCheck {
                id: "target_exists".into(),
                description: "Target device exists".into(),
                passed: true,
                severity: CheckSeverity::Info,
                detail: format!(
                    "{} ({}, {})",
                    dev.name,
                    humansize::format_size(dev.size, humansize::BINARY),
                    dev.device_type
                ),
            });
            Some(dev)
        }
        Err(e) => {
            // Check if it's a regular file path (non-device target)
            let is_file_path = Path::new(target).parent().map_or(false, |p| p.exists());
            if is_file_path {
                checks.push(SafetyCheck {
                    id: "target_exists".into(),
                    description: "Target is a file path".into(),
                    passed: true,
                    severity: CheckSeverity::Warning,
                    detail: format!("Writing to file (not a block device): {}", target),
                });
            } else {
                checks.push(SafetyCheck {
                    id: "target_exists".into(),
                    description: "Target device exists".into(),
                    passed: false,
                    severity: CheckSeverity::Error,
                    detail: format!("Device not found: {} ({})", target, e),
                });
            }
            None
        }
    };

    if let Some(ref dev) = device {
        // ── 2a. System drive check ──────────────────────────────────────
        checks.push(SafetyCheck {
            id: "not_system_drive".into(),
            description: "Target is not a system/boot drive".into(),
            passed: !dev.is_system,
            severity: CheckSeverity::Error,
            detail: if dev.is_system {
                format!(
                    "DANGER: {} is a system drive! Writing here will destroy your OS.",
                    dev.path
                )
            } else {
                "Not a system drive".into()
            },
        });

        // ── 2b. Read-only check ─────────────────────────────────────────
        checks.push(SafetyCheck {
            id: "not_read_only".into(),
            description: "Target is writable".into(),
            passed: !dev.read_only,
            severity: CheckSeverity::Error,
            detail: if dev.read_only {
                format!("{} is read-only (write-protect switch or locked)", dev.path)
            } else {
                "Writable".into()
            },
        });

        // ── 2c. Removable media check ───────────────────────────────────
        let removable_required = matches!(safety_level, SafetyLevel::High);
        let removable_warning = matches!(safety_level, SafetyLevel::Medium);
        if !dev.is_removable_media() {
            checks.push(SafetyCheck {
                id: "removable_media".into(),
                description: "Target is removable media".into(),
                passed: !removable_required,
                severity: if removable_required {
                    CheckSeverity::Error
                } else if removable_warning {
                    CheckSeverity::Warning
                } else {
                    CheckSeverity::Info
                },
                detail: format!(
                    "{} is not removable media (type: {}). {}",
                    dev.path,
                    dev.device_type,
                    if removable_required {
                        "High safety level requires removable media."
                    } else {
                        "Ensure this is the intended target."
                    }
                ),
            });
        } else {
            checks.push(SafetyCheck {
                id: "removable_media".into(),
                description: "Target is removable media".into(),
                passed: true,
                severity: CheckSeverity::Info,
                detail: format!("{} ({})", dev.device_type, dev.path),
            });
        }

        // ── 2d. Mounted filesystem check ────────────────────────────────
        if !dev.mount_points.is_empty() {
            checks.push(SafetyCheck {
                id: "not_mounted".into(),
                description: "Target has no mounted filesystems".into(),
                passed: false,
                severity: CheckSeverity::Warning,
                detail: format!(
                    "Mounted at: {}. Will attempt to unmount before writing.",
                    dev.mount_points.join(", ")
                ),
            });
        } else {
            checks.push(SafetyCheck {
                id: "not_mounted".into(),
                description: "Target has no mounted filesystems".into(),
                passed: true,
                severity: CheckSeverity::Info,
                detail: "No mounted filesystems".into(),
            });
        }

        // ── 2e. Size check (image fits on device) ───────────────────────
        if dev.size > 0 {
            if let ImageSource::File(path) = source {
                if let Ok(meta) = std::fs::metadata(path) {
                    let image_size = meta.len();
                    let fits = image_size <= dev.size;
                    checks.push(SafetyCheck {
                        id: "image_fits".into(),
                        description: "Image fits on target device".into(),
                        passed: fits,
                        severity: if fits {
                            CheckSeverity::Info
                        } else {
                            CheckSeverity::Error
                        },
                        detail: if fits {
                            format!(
                                "Image {} ≤ Device {} ({}% of device)",
                                humansize::format_size(image_size, humansize::BINARY),
                                humansize::format_size(dev.size, humansize::BINARY),
                                if dev.size > 0 {
                                    image_size * 100 / dev.size
                                } else {
                                    0
                                }
                            )
                        } else {
                            format!(
                                "Image {} > Device {} — image will not fit!",
                                humansize::format_size(image_size, humansize::BINARY),
                                humansize::format_size(dev.size, humansize::BINARY)
                            )
                        },
                    });
                }
            }
        }

        // ── 2f. Self-write protection ───────────────────────────────────
        if let ImageSource::File(path) = source {
            let source_str = path.to_string_lossy();
            let self_write = source_str.contains(target)
                || dev.mount_points.iter().any(|mp| source_str.starts_with(mp));
            if self_write {
                checks.push(SafetyCheck {
                    id: "no_self_write".into(),
                    description: "Source is not on target device".into(),
                    passed: false,
                    severity: CheckSeverity::Error,
                    detail: "Source image appears to be on the target device! This would destroy the source.".into(),
                });
            } else {
                checks.push(SafetyCheck {
                    id: "no_self_write".into(),
                    description: "Source is not on target device".into(),
                    passed: true,
                    severity: CheckSeverity::Info,
                    detail: "OK".into(),
                });
            }
        }

        // ── 2g. Device confirmation token (agent safety) ────────────────
        if let Some(token) = confirm_token {
            let fingerprint = DeviceFingerprint::from_device(dev);
            let token_matches = fingerprint.token == token;
            checks.push(SafetyCheck {
                id: "confirm_token".into(),
                description: "Device confirmation token matches".into(),
                passed: token_matches,
                severity: if matches!(safety_level, SafetyLevel::High) {
                    CheckSeverity::Error
                } else {
                    CheckSeverity::Warning
                },
                detail: if token_matches {
                    "Device identity confirmed — matches token from enumeration".into()
                } else {
                    format!(
                        "Token mismatch! Device may have changed since enumeration. \
                         Expected: {}, Got: {}",
                        token, fingerprint.token
                    )
                },
            });
        } else if matches!(safety_level, SafetyLevel::High) {
            checks.push(SafetyCheck {
                id: "confirm_token".into(),
                description: "Device confirmation token provided".into(),
                passed: false,
                severity: CheckSeverity::Error,
                detail: "High safety level requires --confirm-token. \
                         Use 'abt list --json' to get device tokens."
                    .into(),
            });
        }
    }

    // ── 3. Privilege check ─────────────────────────────────────────────────
    let elevated = crate::platform::is_elevated();
    checks.push(SafetyCheck {
        id: "elevated_privileges".into(),
        description: "Running with elevated privileges".into(),
        passed: elevated,
        severity: if elevated { CheckSeverity::Info } else { CheckSeverity::Warning },
        detail: if elevated {
            "Running as root/administrator".into()
        } else {
            "Not elevated — write to block devices may fail. Use sudo (Linux/macOS) or Run as Administrator (Windows).".into()
        },
    });

    // ── Build report ───────────────────────────────────────────────────────
    let errors = checks
        .iter()
        .filter(|c| !c.passed && c.severity == CheckSeverity::Error)
        .count();
    let warnings = checks
        .iter()
        .filter(|c| !c.passed && c.severity == CheckSeverity::Warning)
        .count();

    let fingerprint = device.as_ref().map(|d| DeviceFingerprint::from_device(d));

    let report = SafetyReport {
        safe_to_proceed: errors == 0,
        safety_level,
        checks,
        device_fingerprint: fingerprint,
        errors,
        warnings,
        dry_run,
    };

    // SI-1/SI-10: Postcondition — safe_to_proceed ↔ errors == 0
    debug_assert!(
        report.safe_to_proceed == (report.errors == 0),
        "POSTCONDITION VIOLATED: safe_to_proceed must be true iff errors == 0"
    );
    // SI-10: Postcondition — error/warning counts match checks
    debug_assert!(
        report.errors
            == report
                .checks
                .iter()
                .filter(|c| !c.passed && c.severity == CheckSeverity::Error)
                .count(),
        "POSTCONDITION VIOLATED: error count must match failed error-severity checks"
    );
    debug_assert!(
        report.warnings
            == report
                .checks
                .iter()
                .filter(|c| !c.passed && c.severity == CheckSeverity::Warning)
                .count(),
        "POSTCONDITION VIOLATED: warning count must match failed warning-severity checks"
    );

    Ok(report)
}

/// Back up the first 1 MiB of a device (contains MBR/GPT) before writing.
/// Returns the backup path on success. Uses spawn_blocking to avoid blocking
/// the async runtime with synchronous file I/O.
pub async fn backup_partition_table(device_path: &str) -> anyhow::Result<std::path::PathBuf> {
    let device_path = device_path.to_string();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;

        let backup_dir = std::env::temp_dir().join("abt_backups");
        std::fs::create_dir_all(&backup_dir)?;

        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        let safe_name = device_path
            .replace(['/', '\\', '.'], "_")
            .trim_matches('_')
            .to_string();
        let backup_path = backup_dir.join(format!("pt_{}_{}.bin", safe_name, timestamp));

        let mut device = std::fs::File::open(&device_path)?;
        let mut buf = vec![0u8; 1024 * 1024]; // 1 MiB — covers both MBR and GPT
        let n = device.read(&mut buf)?;
        buf.truncate(n);

        std::fs::write(&backup_path, &buf)?;
        log::info!(
            "Partition table backup saved: {} ({} bytes)",
            backup_path.display(),
            n
        );

        Ok(backup_path)
    })
    .await?
}

// ── Structured exit codes ──────────────────────────────────────────────────────

/// Exit codes with well-defined semantics for agent consumption.
/// These correspond to std::process::exit() codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
#[allow(dead_code)]
pub enum ExitCode {
    /// Operation completed successfully
    Success = 0,
    /// General / unspecified error
    GeneralError = 1,
    /// Pre-flight safety check failed (blocked by safety system)
    SafetyCheckFailed = 2,
    /// Verification failed (data mismatch after write)
    VerificationFailed = 3,
    /// Permission denied / insufficient privileges
    PermissionDenied = 4,
    /// Source image not found or unreadable
    SourceError = 5,
    /// Target device not found, read-only, or unavailable
    TargetError = 6,
    /// Image too large for target device
    SizeMismatch = 7,
    /// Device changed between enumeration and write (token mismatch)
    DeviceChanged = 8,
    /// Operation cancelled by user or agent
    Cancelled = 130,
}

impl ExitCode {
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// Map an anyhow::Error to a structured exit code.
pub fn error_to_exit_code(err: &anyhow::Error) -> ExitCode {
    if let Some(abt_err) = err.downcast_ref::<super::error::AbtError>() {
        match abt_err {
            super::error::AbtError::Io(_) => ExitCode::GeneralError,
            super::error::AbtError::DeviceNotFound(_) => ExitCode::TargetError,
            super::error::AbtError::SystemDrive(_) => ExitCode::SafetyCheckFailed,
            super::error::AbtError::ReadOnly(_) => ExitCode::TargetError,
            super::error::AbtError::ImageNotFound(_) => ExitCode::SourceError,
            super::error::AbtError::UnsupportedFormat(_) => ExitCode::SourceError,
            super::error::AbtError::ImageTooLarge { .. } => ExitCode::SizeMismatch,
            super::error::AbtError::VerificationFailed { .. } => ExitCode::VerificationFailed,
            super::error::AbtError::ChecksumMismatch { .. } => ExitCode::VerificationFailed,
            super::error::AbtError::Aborted => ExitCode::Cancelled,
            super::error::AbtError::PermissionDenied => ExitCode::PermissionDenied,
            super::error::AbtError::Decompression(_) => ExitCode::SourceError,
            super::error::AbtError::FormatError(_) => ExitCode::GeneralError,
            super::error::AbtError::PlatformError(_) => ExitCode::GeneralError,
            super::error::AbtError::ConfigError(_) => ExitCode::GeneralError,
            super::error::AbtError::Timeout { .. } => ExitCode::GeneralError,
            super::error::AbtError::CancelledByUser => ExitCode::Cancelled,
            super::error::AbtError::BackupFailed(_) => ExitCode::GeneralError,
            super::error::AbtError::TokenMismatch { .. } => ExitCode::DeviceChanged,
            super::error::AbtError::RetryExhausted { .. } => ExitCode::GeneralError,
            super::error::AbtError::DeviceChanged(_) => ExitCode::DeviceChanged,
        }
    } else {
        ExitCode::GeneralError
    }
}

/// Produce a structured JSON error response for agent consumption.
pub fn structured_error(err: &anyhow::Error, exit_code: ExitCode) -> serde_json::Value {
    json!({
        "success": false,
        "exit_code": exit_code.code(),
        "error": format!("{}", err),
        "error_chain": err.chain().map(|e| format!("{}", e)).collect::<Vec<_>>(),
    })
}
