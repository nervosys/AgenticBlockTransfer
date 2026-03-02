// Security audit module — hardened input validation, path traversal prevention,
// symlink protection, privilege auditing, and TOCTOU race mitigation.
//
// This module consolidates all security-sensitive checks that go beyond the
// pre-flight safety system in safety.rs. While safety.rs prevents accidental
// data loss (wrong device, system drive), this module prevents exploitation:
// malicious paths, symlink attacks, privilege escalation, and race conditions.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

// ── Security finding severity ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Info => "INFO",
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
        }
    }

    pub fn is_blocking(&self) -> bool {
        matches!(self, Self::High | Self::Critical)
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ── Security finding ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityFinding {
    pub id: String,
    pub severity: Severity,
    pub category: SecurityCategory,
    pub title: String,
    pub description: String,
    pub remediation: String,
}

impl SecurityFinding {
    pub fn new(
        id: &str,
        severity: Severity,
        category: SecurityCategory,
        title: &str,
        description: &str,
        remediation: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            severity,
            category,
            title: title.to_string(),
            description: description.to_string(),
            remediation: remediation.to_string(),
        }
    }
}

impl std::fmt::Display for SecurityFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {}: {} — {}",
            self.severity, self.id, self.title, self.description
        )
    }
}

// ── Security categories ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityCategory {
    PathTraversal,
    SymlinkAttack,
    PrivilegeEscalation,
    InputValidation,
    RaceCondition,
    InformationDisclosure,
    IntegrityViolation,
    ResourceExhaustion,
}

impl SecurityCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PathTraversal => "Path Traversal",
            Self::SymlinkAttack => "Symlink Attack",
            Self::PrivilegeEscalation => "Privilege Escalation",
            Self::InputValidation => "Input Validation",
            Self::RaceCondition => "Race Condition",
            Self::InformationDisclosure => "Information Disclosure",
            Self::IntegrityViolation => "Integrity Violation",
            Self::ResourceExhaustion => "Resource Exhaustion",
        }
    }
}

impl std::fmt::Display for SecurityCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ── Security audit report ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAuditReport {
    pub timestamp: String,
    pub version: String,
    pub findings: Vec<SecurityFinding>,
    pub passed: bool,
    pub summary: HashMap<String, usize>,
}

impl SecurityAuditReport {
    pub fn new() -> Self {
        Self {
            timestamp: chrono::Utc::now().to_rfc3339(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            findings: Vec::new(),
            passed: true,
            summary: HashMap::new(),
        }
    }

    pub fn add(&mut self, finding: SecurityFinding) {
        if finding.severity.is_blocking() {
            self.passed = false;
        }
        let sev = finding.severity.label().to_string();
        *self.summary.entry(sev).or_insert(0) += 1;
        self.findings.push(finding);
    }

    pub fn has_blocking(&self) -> bool {
        self.findings.iter().any(|f| f.severity.is_blocking())
    }

    pub fn finding_count(&self) -> usize {
        self.findings.len()
    }

    pub fn to_json(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    pub fn format_text(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "Security Audit Report — abt v{}\n",
            self.version
        ));
        out.push_str(&format!("Timestamp: {}\n", self.timestamp));
        out.push_str(&format!(
            "Result: {}\n\n",
            if self.passed { "PASS" } else { "FAIL" }
        ));

        if self.findings.is_empty() {
            out.push_str("No security findings.\n");
            return out;
        }

        for f in &self.findings {
            out.push_str(&format!(
                "  [{:>8}] {} ({})\n",
                f.severity, f.title, f.category
            ));
            out.push_str(&format!("             {}\n", f.description));
            out.push_str(&format!("             Fix: {}\n\n", f.remediation));
        }

        out
    }
}

impl Default for SecurityAuditReport {
    fn default() -> Self {
        Self::new()
    }
}

// ── Path validation ────────────────────────────────────────────────────────────

/// Validate a path for traversal attacks. Returns findings for any issues.
pub fn validate_path(path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();
    let path_str = path.to_string_lossy();

    // Check for path traversal components (..)
    for component in path.components() {
        if matches!(component, Component::ParentDir) {
            findings.push(SecurityFinding::new(
                "SEC-001",
                Severity::High,
                SecurityCategory::PathTraversal,
                "Path traversal detected",
                &format!("Path contains '..' component: {}", path_str),
                "Use an absolute, canonical path without parent directory references",
            ));
            break;
        }
    }

    // Check for null bytes (can truncate paths in C-based syscalls)
    if path_str.contains('\0') {
        findings.push(SecurityFinding::new(
            "SEC-002",
            Severity::Critical,
            SecurityCategory::InputValidation,
            "Null byte in path",
            &format!("Path contains null byte: {}", path_str.replace('\0', "\\0")),
            "Remove null bytes from the path",
        ));
    }

    // Check for invalid/control characters
    for ch in path_str.chars() {
        if ch.is_control() && ch != '\0' {
            findings.push(SecurityFinding::new(
                "SEC-003",
                Severity::Medium,
                SecurityCategory::InputValidation,
                "Control character in path",
                &format!("Path contains control character U+{:04X}", ch as u32),
                "Remove control characters from file paths",
            ));
            break;
        }
    }

    // Check for excessively long paths (potential DoS / buffer overflow in OS APIs)
    if path_str.len() > 4096 {
        findings.push(SecurityFinding::new(
            "SEC-004",
            Severity::Medium,
            SecurityCategory::ResourceExhaustion,
            "Excessively long path",
            &format!("Path length {} exceeds 4096 byte limit", path_str.len()),
            "Use a shorter path",
        ));
    }

    // On Windows check for reserved device names (CON, PRN, AUX, NUL, COM1..9, LPT1..9)
    #[cfg(windows)]
    {
        if let Some(stem) = path.file_stem() {
            let stem_upper = stem.to_string_lossy().to_uppercase();
            let reserved = [
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
                "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
                "LPT7", "LPT8", "LPT9",
            ];
            if reserved.contains(&stem_upper.as_str()) {
                findings.push(SecurityFinding::new(
                    "SEC-005",
                    Severity::High,
                    SecurityCategory::InputValidation,
                    "Windows reserved device name",
                    &format!(
                        "Path uses reserved device name '{}' which could redirect I/O",
                        stem_upper
                    ),
                    "Choose a different filename that doesn't conflict with Windows device names",
                ));
            }
        }
    }

    findings
}

/// Canonicalize a path and verify it stays within the expected base directory.
/// Returns the canonical path if valid, or findings describing the violation.
pub fn validate_path_containment(
    path: &Path,
    base: &Path,
) -> Result<PathBuf, Vec<SecurityFinding>> {
    // First check for obvious traversal
    let traversal = validate_path(path);
    if traversal.iter().any(|f| f.severity.is_blocking()) {
        return Err(traversal);
    }

    // Attempt to canonicalize (resolves symlinks)
    let canonical = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            return Err(vec![SecurityFinding::new(
                "SEC-006",
                Severity::Medium,
                SecurityCategory::PathTraversal,
                "Path canonicalization failed",
                &format!("Cannot resolve path {}: {}", path.display(), e),
                "Ensure the path exists and is accessible",
            )]);
        }
    };

    let base_canonical = match std::fs::canonicalize(base) {
        Ok(p) => p,
        Err(e) => {
            return Err(vec![SecurityFinding::new(
                "SEC-006",
                Severity::Medium,
                SecurityCategory::PathTraversal,
                "Base path canonicalization failed",
                &format!("Cannot resolve base path {}: {}", base.display(), e),
                "Ensure the base path exists and is accessible",
            )]);
        }
    };

    if !canonical.starts_with(&base_canonical) {
        return Err(vec![SecurityFinding::new(
            "SEC-007",
            Severity::High,
            SecurityCategory::PathTraversal,
            "Path escapes base directory",
            &format!(
                "Resolved path {} is outside base {}",
                canonical.display(),
                base_canonical.display()
            ),
            "Use a path within the intended base directory",
        )]);
    }

    Ok(canonical)
}

// ── Symlink validation ─────────────────────────────────────────────────────────

/// Check if a path is a symlink and report accordingly.
pub fn validate_symlink(path: &Path) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                // Resolve the symlink target
                match std::fs::read_link(path) {
                    Ok(target) => {
                        findings.push(SecurityFinding::new(
                            "SEC-010",
                            Severity::Medium,
                            SecurityCategory::SymlinkAttack,
                            "Symbolic link detected",
                            &format!(
                                "Path {} is a symlink pointing to {}",
                                path.display(),
                                target.display()
                            ),
                            "Use the real path directly or verify the symlink target is trusted",
                        ));

                        // Check if symlink target is an absolute path (could escape sandbox)
                        if target.is_absolute() {
                            findings.push(SecurityFinding::new(
                                "SEC-011",
                                Severity::High,
                                SecurityCategory::SymlinkAttack,
                                "Absolute symlink target",
                                &format!(
                                    "Symlink {} points to absolute path {} which could escape containment",
                                    path.display(), target.display()
                                ),
                                "Use relative symlinks or verify the absolute target is safe",
                            ));
                        }
                    }
                    Err(e) => {
                        findings.push(SecurityFinding::new(
                            "SEC-012",
                            Severity::High,
                            SecurityCategory::SymlinkAttack,
                            "Unresolvable symlink",
                            &format!("Cannot read symlink target for {}: {}", path.display(), e),
                            "Remove or fix the broken symlink",
                        ));
                    }
                }
            }
        }
        Err(_) => {
            // Path doesn't exist — not a symlink issue
        }
    }

    findings
}

// ── Device path validation ─────────────────────────────────────────────────────

/// Validate that a device path looks like a real block device, not a file masquerading as one.
pub fn validate_device_path(path: &str) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    #[cfg(unix)]
    {
        // Unix device paths should be under /dev/
        if !path.starts_with("/dev/") {
            findings.push(SecurityFinding::new(
                "SEC-020",
                Severity::High,
                SecurityCategory::InputValidation,
                "Device path outside /dev",
                &format!(
                    "Device path '{}' is not under /dev/ — could be a regular file",
                    path
                ),
                "Use a proper device path like /dev/sdX or /dev/loopN",
            ));
        }

        // Check for path traversal in device path
        if path.contains("..") {
            findings.push(SecurityFinding::new(
                "SEC-021",
                Severity::Critical,
                SecurityCategory::PathTraversal,
                "Path traversal in device path",
                &format!("Device path '{}' contains '..' traversal", path),
                "Use a canonical device path without directory traversal",
            ));
        }
    }

    #[cfg(windows)]
    {
        // Windows device paths should start with \\.\
        let valid_prefix = path.starts_with("\\\\.\\PhysicalDrive")
            || path.starts_with("\\\\.\\Harddisk")
            || path.starts_with("\\\\.\\CdRom");
        if !valid_prefix && !path.contains("PhysicalDrive") {
            findings.push(SecurityFinding::new(
                "SEC-020",
                Severity::Medium,
                SecurityCategory::InputValidation,
                "Non-standard Windows device path",
                &format!(
                    "Device path '{}' doesn't match expected \\\\.\\PhysicalDriveN pattern",
                    path
                ),
                "Use a standard Windows device path like \\\\.\\PhysicalDrive1",
            ));
        }
    }

    // Check for shell metacharacters that could enable injection
    let dangerous_chars = ['|', ';', '&', '$', '`', '(', ')', '{', '}', '<', '>', '!', '~'];
    for ch in dangerous_chars {
        if path.contains(ch) {
            findings.push(SecurityFinding::new(
                "SEC-022",
                Severity::High,
                SecurityCategory::InputValidation,
                "Shell metacharacter in device path",
                &format!(
                    "Device path contains '{}' which could enable command injection",
                    ch
                ),
                "Remove shell metacharacters from the device path",
            ));
            break;
        }
    }

    findings
}

// ── Privilege audit ────────────────────────────────────────────────────────────

/// Audit the current process privileges and report security-relevant findings.
pub fn audit_privileges() -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    #[cfg(unix)]
    {
        let uid = unsafe { libc::getuid() };
        let euid = unsafe { libc::geteuid() };

        if uid == 0 {
            findings.push(SecurityFinding::new(
                "SEC-030",
                Severity::Info,
                SecurityCategory::PrivilegeEscalation,
                "Running as root",
                "Process is running with UID 0 (root privileges)",
                "Consider using minimal privileges and elevating only when needed",
            ));
        }

        // Check for setuid — running as different effective user
        if uid != euid {
            findings.push(SecurityFinding::new(
                "SEC-031",
                Severity::High,
                SecurityCategory::PrivilegeEscalation,
                "SUID bit detected",
                &format!(
                    "Real UID ({}) differs from effective UID ({}) — setuid binary",
                    uid, euid
                ),
                "Avoid running abt as a setuid binary; use sudo instead",
            ));
        }
    }

    #[cfg(windows)]
    {
        // Check if running as administrator
        let is_admin = std::env::var("USERNAME")
            .map(|u| u.to_uppercase())
            .unwrap_or_default();
        if is_admin == "SYSTEM" || is_admin == "ADMINISTRATOR" {
            findings.push(SecurityFinding::new(
                "SEC-030",
                Severity::Info,
                SecurityCategory::PrivilegeEscalation,
                "Running with elevated privileges",
                &format!("Process is running as {}", is_admin),
                "Consider using minimal privileges where possible",
            ));
        }
    }

    // Check for suspicious environment variables
    let suspicious_vars = [
        ("LD_PRELOAD", "Could inject malicious shared libraries"),
        ("LD_LIBRARY_PATH", "Could redirect library loading"),
        ("DYLD_INSERT_LIBRARIES", "macOS library injection"),
        ("DYLD_LIBRARY_PATH", "macOS library path override"),
    ];

    for (var, desc) in &suspicious_vars {
        if std::env::var(var).is_ok() {
            findings.push(SecurityFinding::new(
                "SEC-032",
                Severity::Medium,
                SecurityCategory::PrivilegeEscalation,
                "Suspicious environment variable",
                &format!("{} is set: {}", var, desc),
                &format!("Unset {} unless intentionally required", var),
            ));
        }
    }

    findings
}

// ── Input sanitization ─────────────────────────────────────────────────────────

/// Sanitize a string for use in log messages and output. Strips control characters
/// and truncates to a maximum length to prevent log injection and DoS.
pub fn sanitize_for_display(input: &str, max_len: usize) -> String {
    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(max_len)
        .collect();
    if input.len() > max_len {
        format!("{}...(truncated)", cleaned)
    } else {
        cleaned
    }
}

/// Validate a URL for safe use as an image source. Checks for internal network
/// addresses, file:// scheme abuse, and credential leakage.
pub fn validate_url(url: &str) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    // Block file:// URIs (could access local files)
    if url.starts_with("file://") || url.starts_with("file:///") {
        findings.push(SecurityFinding::new(
            "SEC-040",
            Severity::High,
            SecurityCategory::PathTraversal,
            "file:// URL scheme",
            "file:// URLs access the local filesystem and bypass network security controls",
            "Use a local file path directly instead of file:// URLs",
        ));
    }

    // Check for credentials in URL
    if url.contains('@') && (url.starts_with("http://") || url.starts_with("https://")) {
        let authority = url
            .split("://")
            .nth(1)
            .unwrap_or("")
            .split('/')
            .next()
            .unwrap_or("");
        if authority.contains('@') {
            findings.push(SecurityFinding::new(
                "SEC-041",
                Severity::Medium,
                SecurityCategory::InformationDisclosure,
                "Credentials in URL",
                "URL contains embedded credentials (user:pass@host) which may be logged",
                "Use a URL without embedded credentials; provide authentication separately",
            ));
        }
    }

    // Check for private/internal network addresses (SSRF prevention)
    let lower = url.to_lowercase();
    let internal_patterns = [
        "://localhost",
        "://127.",
        "://0.0.0.0",
        "://[::1]",
        "://10.",
        "://172.16.",
        "://172.17.",
        "://172.18.",
        "://172.19.",
        "://172.20.",
        "://172.21.",
        "://172.22.",
        "://172.23.",
        "://172.24.",
        "://172.25.",
        "://172.26.",
        "://172.27.",
        "://172.28.",
        "://172.29.",
        "://172.30.",
        "://172.31.",
        "://192.168.",
        "://169.254.",
    ];
    for pat in &internal_patterns {
        if lower.contains(pat) {
            findings.push(SecurityFinding::new(
                "SEC-042",
                Severity::Low,
                SecurityCategory::InputValidation,
                "Internal network URL",
                &format!(
                    "URL points to an internal/private network address (matches {})",
                    pat
                ),
                "Verify you intend to access this internal resource",
            ));
            break;
        }
    }

    findings
}

// ── TOCTOU race detection ──────────────────────────────────────────────────────

/// Snapshot of a file's metadata for TOCTOU race detection.
/// Take a snapshot before validation, then verify it hasn't changed before use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: String,
    pub size: u64,
    pub modified: String,
    pub is_symlink: bool,
    pub is_file: bool,
    pub is_dir: bool,
    #[cfg(unix)]
    pub inode: u64,
    #[cfg(unix)]
    pub device: u64,
}

impl FileSnapshot {
    /// Take a metadata snapshot of a path.
    pub fn capture(path: &Path) -> anyhow::Result<Self> {
        let sym_meta = std::fs::symlink_metadata(path)?;
        let meta = std::fs::metadata(path)?;

        Ok(Self {
            path: path.to_string_lossy().to_string(),
            size: meta.len(),
            modified: meta
                .modified()
                .map(|t| {
                    chrono::DateTime::<chrono::Utc>::from(t)
                        .to_rfc3339()
                })
                .unwrap_or_else(|_| "unknown".to_string()),
            is_symlink: sym_meta.file_type().is_symlink(),
            is_file: meta.is_file(),
            is_dir: meta.is_dir(),
            #[cfg(unix)]
            inode: {
                use std::os::unix::fs::MetadataExt;
                meta.ino()
            },
            #[cfg(unix)]
            device: {
                use std::os::unix::fs::MetadataExt;
                meta.dev()
            },
        })
    }

    /// Verify that the file hasn't changed since the snapshot was taken.
    /// Returns findings for any detected changes.
    pub fn verify(&self, path: &Path) -> Vec<SecurityFinding> {
        let mut findings = Vec::new();

        match Self::capture(path) {
            Ok(current) => {
                if current.size != self.size {
                    findings.push(SecurityFinding::new(
                        "SEC-050",
                        Severity::High,
                        SecurityCategory::RaceCondition,
                        "File size changed (TOCTOU)",
                        &format!(
                            "File {} changed size from {} to {} between check and use",
                            path.display(),
                            self.size,
                            current.size
                        ),
                        "Re-validate the file before proceeding",
                    ));
                }

                if current.modified != self.modified {
                    findings.push(SecurityFinding::new(
                        "SEC-051",
                        Severity::High,
                        SecurityCategory::RaceCondition,
                        "File modified (TOCTOU)",
                        &format!(
                            "File {} was modified between check and use",
                            path.display()
                        ),
                        "Re-validate the file before proceeding",
                    ));
                }

                if current.is_symlink != self.is_symlink {
                    findings.push(SecurityFinding::new(
                        "SEC-052",
                        Severity::Critical,
                        SecurityCategory::RaceCondition,
                        "Symlink status changed (TOCTOU)",
                        &format!(
                            "File {} symlink status changed between check and use",
                            path.display()
                        ),
                        "The file may have been swapped with a symlink — abort the operation",
                    ));
                }

                #[cfg(unix)]
                {
                    if current.inode != self.inode || current.device != self.device {
                        findings.push(SecurityFinding::new(
                            "SEC-053",
                            Severity::Critical,
                            SecurityCategory::RaceCondition,
                            "Inode/device changed (TOCTOU)",
                            &format!(
                                "File {} inode/device changed — file was replaced",
                                path.display()
                            ),
                            "The file was replaced between check and use — abort",
                        ));
                    }
                }
            }
            Err(e) => {
                findings.push(SecurityFinding::new(
                    "SEC-054",
                    Severity::High,
                    SecurityCategory::RaceCondition,
                    "File disappeared (TOCTOU)",
                    &format!(
                        "File {} no longer accessible: {}",
                        path.display(),
                        e
                    ),
                    "The file was removed between check and use",
                ));
            }
        }

        findings
    }
}

// ── Hash integrity validation ──────────────────────────────────────────────────

/// Validate a hash string format (algorithm:hex).
pub fn validate_hash_format(hash_str: &str) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    if !hash_str.contains(':') {
        findings.push(SecurityFinding::new(
            "SEC-060",
            Severity::Low,
            SecurityCategory::IntegrityViolation,
            "No hash algorithm prefix",
            &format!(
                "Hash '{}' has no algorithm prefix (expected algo:hex)",
                sanitize_for_display(hash_str, 80)
            ),
            "Prefix the hash with the algorithm, e.g., sha256:abcdef...",
        ));
        return findings;
    }

    let parts: Vec<&str> = hash_str.splitn(2, ':').collect();
    let algo = parts[0].to_lowercase();
    let hex_value = parts[1];

    let valid_algos = ["sha256", "sha512", "sha1", "md5", "blake3", "crc32"];
    if !valid_algos.contains(&algo.as_str()) {
        findings.push(SecurityFinding::new(
            "SEC-061",
            Severity::Medium,
            SecurityCategory::IntegrityViolation,
            "Unknown hash algorithm",
            &format!(
                "Hash algorithm '{}' is not recognized; supported: {:?}",
                algo, valid_algos
            ),
            "Use a supported hash algorithm",
        ));
    }

    // Validate hex characters
    if !hex_value.chars().all(|c| c.is_ascii_hexdigit()) {
        findings.push(SecurityFinding::new(
            "SEC-062",
            Severity::Medium,
            SecurityCategory::IntegrityViolation,
            "Invalid hex in hash",
            "Hash value contains non-hexadecimal characters",
            "Provide a valid hex-encoded hash value",
        ));
    }

    // Check expected lengths
    let expected_len = match algo.as_str() {
        "sha256" => Some(64),
        "sha512" => Some(128),
        "sha1" => Some(40),
        "md5" => Some(32),
        "blake3" => Some(64),
        "crc32" => Some(8),
        _ => None,
    };

    if let Some(expected) = expected_len {
        if hex_value.len() != expected {
            findings.push(SecurityFinding::new(
                "SEC-063",
                Severity::Medium,
                SecurityCategory::IntegrityViolation,
                "Hash length mismatch",
                &format!(
                    "{} hash should be {} hex chars, got {}",
                    algo,
                    expected,
                    hex_value.len()
                ),
                "Provide a complete hash value of the correct length",
            ));
        }
    }

    findings
}

// ── Full security audit ────────────────────────────────────────────────────────

/// Run a comprehensive security audit on the given operation parameters.
pub fn run_audit(
    source_path: Option<&str>,
    target_path: Option<&str>,
    url: Option<&str>,
    hash: Option<&str>,
) -> SecurityAuditReport {
    let mut report = SecurityAuditReport::new();

    // Audit privileges
    for f in audit_privileges() {
        report.add(f);
    }

    // Validate source path
    if let Some(src) = source_path {
        let path = Path::new(src);
        for f in validate_path(path) {
            report.add(f);
        }
        for f in validate_symlink(path) {
            report.add(f);
        }
    }

    // Validate target/device path
    if let Some(tgt) = target_path {
        for f in validate_device_path(tgt) {
            report.add(f);
        }
        let path = Path::new(tgt);
        for f in validate_symlink(path) {
            report.add(f);
        }
    }

    // Validate URL
    if let Some(u) = url {
        for f in validate_url(u) {
            report.add(f);
        }
    }

    // Validate hash
    if let Some(h) = hash {
        for f in validate_hash_format(h) {
            report.add(f);
        }
    }

    report
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering() {
        assert!(Severity::Info < Severity::Low);
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::High);
        assert!(Severity::High < Severity::Critical);
    }

    #[test]
    fn severity_blocking() {
        assert!(!Severity::Info.is_blocking());
        assert!(!Severity::Low.is_blocking());
        assert!(!Severity::Medium.is_blocking());
        assert!(Severity::High.is_blocking());
        assert!(Severity::Critical.is_blocking());
    }

    #[test]
    fn severity_labels() {
        assert_eq!(Severity::Info.label(), "INFO");
        assert_eq!(Severity::Critical.label(), "CRITICAL");
    }

    #[test]
    fn finding_display() {
        let f = SecurityFinding::new(
            "TEST-001",
            Severity::High,
            SecurityCategory::PathTraversal,
            "Test finding",
            "Description here",
            "Fix it",
        );
        let s = f.to_string();
        assert!(s.contains("HIGH"));
        assert!(s.contains("TEST-001"));
        assert!(s.contains("Test finding"));
    }

    #[test]
    fn category_labels() {
        assert_eq!(SecurityCategory::PathTraversal.label(), "Path Traversal");
        assert_eq!(SecurityCategory::SymlinkAttack.label(), "Symlink Attack");
        assert_eq!(
            SecurityCategory::ResourceExhaustion.label(),
            "Resource Exhaustion"
        );
    }

    #[test]
    fn report_default_passes() {
        let report = SecurityAuditReport::new();
        assert!(report.passed);
        assert_eq!(report.finding_count(), 0);
        assert!(!report.has_blocking());
    }

    #[test]
    fn report_blocking_finding_fails() {
        let mut report = SecurityAuditReport::new();
        report.add(SecurityFinding::new(
            "T-1",
            Severity::High,
            SecurityCategory::PathTraversal,
            "test",
            "test",
            "test",
        ));
        assert!(!report.passed);
        assert!(report.has_blocking());
        assert_eq!(report.finding_count(), 1);
    }

    #[test]
    fn report_non_blocking_stays_passed() {
        let mut report = SecurityAuditReport::new();
        report.add(SecurityFinding::new(
            "T-1",
            Severity::Low,
            SecurityCategory::InputValidation,
            "info",
            "info",
            "info",
        ));
        assert!(report.passed);
        assert!(!report.has_blocking());
    }

    #[test]
    fn report_json() {
        let report = SecurityAuditReport::new();
        let json = report.to_json().unwrap();
        assert!(json.contains("\"passed\": true"));
        assert!(json.contains("\"findings\""));
    }

    #[test]
    fn report_text_format() {
        let mut report = SecurityAuditReport::new();
        report.add(SecurityFinding::new(
            "T-1",
            Severity::Medium,
            SecurityCategory::InputValidation,
            "Test",
            "Desc",
            "Fix",
        ));
        let text = report.format_text();
        assert!(text.contains("PASS") || !text.contains("FAIL"));
        assert!(text.contains("Test"));
    }

    #[test]
    fn validate_clean_path() {
        let findings = validate_path(Path::new("/tmp/image.iso"));
        assert!(
            findings.is_empty(),
            "Clean path should have no findings: {:?}",
            findings
        );
    }

    #[test]
    fn validate_traversal_path() {
        let findings = validate_path(Path::new("/tmp/../etc/passwd"));
        assert!(!findings.is_empty());
        assert_eq!(findings[0].id, "SEC-001");
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn validate_null_byte_path() {
        let findings = validate_path(Path::new("/tmp/image\0.iso"));
        let has_null = findings.iter().any(|f| f.id == "SEC-002");
        assert!(has_null, "Should detect null byte");
    }

    #[test]
    fn validate_long_path() {
        let long = "a".repeat(5000);
        let findings = validate_path(Path::new(&long));
        let has_long = findings.iter().any(|f| f.id == "SEC-004");
        assert!(has_long, "Should detect long path");
    }

    #[test]
    fn validate_device_path_shell_meta() {
        let findings = validate_device_path("/dev/sdb; rm -rf /");
        let has_meta = findings.iter().any(|f| f.id == "SEC-022");
        assert!(has_meta, "Should detect shell metacharacters");
    }

    #[test]
    fn validate_device_path_traversal() {
        #[cfg(unix)]
        {
            let findings = validate_device_path("/dev/../etc/passwd");
            let has_trav = findings.iter().any(|f| f.id == "SEC-021");
            assert!(has_trav, "Should detect traversal in device path");
        }
    }

    #[test]
    fn sanitize_strips_control_chars() {
        let input = "hello\x07world\x1b[31m";
        let sanitized = sanitize_for_display(input, 100);
        assert!(!sanitized.contains('\x07'));
        assert!(sanitized.contains("helloworld"));
    }

    #[test]
    fn sanitize_truncates() {
        let input = "a".repeat(200);
        let sanitized = sanitize_for_display(&input, 50);
        assert!(sanitized.len() < 200);
        assert!(sanitized.contains("truncated"));
    }

    #[test]
    fn validate_url_file_scheme() {
        let findings = validate_url("file:///etc/passwd");
        assert!(!findings.is_empty());
        assert_eq!(findings[0].id, "SEC-040");
    }

    #[test]
    fn validate_url_credentials() {
        let findings = validate_url("https://user:pass@example.com/image.iso");
        let has_creds = findings.iter().any(|f| f.id == "SEC-041");
        assert!(has_creds);
    }

    #[test]
    fn validate_url_internal() {
        let findings = validate_url("http://192.168.1.1/image.iso");
        let has_internal = findings.iter().any(|f| f.id == "SEC-042");
        assert!(has_internal);
    }

    #[test]
    fn validate_url_clean() {
        let findings = validate_url("https://releases.ubuntu.com/24.04/ubuntu.iso");
        assert!(findings.is_empty());
    }

    #[test]
    fn validate_hash_valid_sha256() {
        let hash = format!("sha256:{}", "a".repeat(64));
        let findings = validate_hash_format(&hash);
        assert!(findings.is_empty(), "Valid SHA-256 should pass: {:?}", findings);
    }

    #[test]
    fn validate_hash_wrong_length() {
        let findings = validate_hash_format("sha256:abcdef");
        let has_len = findings.iter().any(|f| f.id == "SEC-063");
        assert!(has_len);
    }

    #[test]
    fn validate_hash_no_prefix() {
        let findings = validate_hash_format("abcdef1234567890");
        let has_no_prefix = findings.iter().any(|f| f.id == "SEC-060");
        assert!(has_no_prefix);
    }

    #[test]
    fn validate_hash_bad_hex() {
        let findings = validate_hash_format("sha256:xyz123");
        let has_hex = findings.iter().any(|f| f.id == "SEC-062");
        assert!(has_hex);
    }

    #[test]
    fn validate_hash_unknown_algo() {
        let findings = validate_hash_format("sm3:abcdef1234567890");
        let has_algo = findings.iter().any(|f| f.id == "SEC-061");
        assert!(has_algo);
    }

    #[test]
    fn privilege_audit_runs() {
        let findings = audit_privileges();
        // Should at least run without panic — findings depend on environment
        let _ = findings;
    }

    #[test]
    fn file_snapshot_capture() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let snap = FileSnapshot::capture(&file_path).unwrap();
        assert_eq!(snap.size, 5);
        assert!(snap.is_file);
        assert!(!snap.is_dir);
        assert!(!snap.is_symlink);
    }

    #[test]
    fn file_snapshot_verify_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let snap = FileSnapshot::capture(&file_path).unwrap();
        let findings = snap.verify(&file_path);
        assert!(findings.is_empty(), "Unchanged file should pass: {:?}", findings);
    }

    #[test]
    fn file_snapshot_verify_changed() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let snap = FileSnapshot::capture(&file_path).unwrap();

        // Modify the file
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(&file_path, "hello world, changed").unwrap();

        let findings = snap.verify(&file_path);
        assert!(!findings.is_empty(), "Changed file should produce findings");
    }

    #[test]
    fn file_snapshot_verify_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello").unwrap();

        let snap = FileSnapshot::capture(&file_path).unwrap();
        std::fs::remove_file(&file_path).unwrap();

        let findings = snap.verify(&file_path);
        assert!(!findings.is_empty(), "Deleted file should produce findings");
        assert_eq!(findings[0].id, "SEC-054");
    }

    #[test]
    fn run_audit_clean() {
        let report = run_audit(None, None, None, None);
        // May have privilege findings but no path/url/hash issues
        assert!(report.findings.iter().all(|f| {
            f.category == SecurityCategory::PrivilegeEscalation
                || f.severity <= Severity::Low
        }) || report.findings.is_empty());
    }

    #[test]
    fn run_audit_with_bad_source() {
        let report = run_audit(Some("/tmp/../etc/passwd"), None, None, None);
        assert!(report.has_blocking());
    }

    #[test]
    fn run_audit_with_bad_url() {
        let report = run_audit(None, None, Some("file:///etc/shadow"), None);
        assert!(report.has_blocking());
    }

    #[test]
    fn summary_counts() {
        let mut report = SecurityAuditReport::new();
        report.add(SecurityFinding::new(
            "T-1",
            Severity::Low,
            SecurityCategory::InputValidation,
            "a",
            "b",
            "c",
        ));
        report.add(SecurityFinding::new(
            "T-2",
            Severity::Low,
            SecurityCategory::InputValidation,
            "d",
            "e",
            "f",
        ));
        report.add(SecurityFinding::new(
            "T-3",
            Severity::High,
            SecurityCategory::PathTraversal,
            "g",
            "h",
            "i",
        ));
        assert_eq!(*report.summary.get("LOW").unwrap(), 2);
        assert_eq!(*report.summary.get("HIGH").unwrap(), 1);
    }
}
