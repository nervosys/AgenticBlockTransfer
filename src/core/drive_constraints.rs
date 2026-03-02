// Drive constraints — validation rules for target drive compatibility.
// Checks whether a drive is suitable as a write target, enforcing safety rules
// like system drive protection, minimum/recommended size, source drive exclusion,
// and locked/read-only detection. Inspired by Etcher's drive-constraints module.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

use super::device::DeviceInfo;

/// Result of evaluating a single constraint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintResult {
    /// Name of the constraint.
    pub name: String,
    /// Whether the constraint passed.
    pub passed: bool,
    /// Severity if the constraint failed.
    pub severity: ConstraintSeverity,
    /// Human-readable message.
    pub message: String,
}

impl fmt::Display for ConstraintResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.passed { "PASS" } else { "FAIL" };
        write!(f, "[{}] {}: {}", status, self.name, self.message)
    }
}

/// Severity of a constraint failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConstraintSeverity {
    /// Informational only — does not block the operation.
    Info,
    /// Warning — operation can proceed but user should be aware.
    Warning,
    /// Error — operation should not proceed.
    Error,
}

impl fmt::Display for ConstraintSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Warning => write!(f, "warning"),
            Self::Error => write!(f, "error"),
        }
    }
}

/// Compatibility status for a drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DriveStatus {
    /// Drive is fully compatible and recommended.
    Compatible,
    /// Drive is compatible but with warnings.
    CompatibleWithWarnings,
    /// Drive is not compatible.
    Incompatible,
}

impl fmt::Display for DriveStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Compatible => write!(f, "compatible"),
            Self::CompatibleWithWarnings => write!(f, "compatible (with warnings)"),
            Self::Incompatible => write!(f, "incompatible"),
        }
    }
}

/// Full validation report for a drive candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Device path.
    pub device_path: String,
    /// Overall status.
    pub status: DriveStatus,
    /// Individual constraint results.
    pub constraints: Vec<ConstraintResult>,
    /// Image path/name used for source check (if applicable).
    pub image_source: Option<String>,
    /// Required size in bytes (for size check).
    pub required_size: Option<u64>,
    /// Recommended size in bytes.
    pub recommended_size: Option<u64>,
}

impl fmt::Display for ValidationReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.device_path, self.status)?;
        for c in &self.constraints {
            if !c.passed {
                write!(f, "\n  {}", c)?;
            }
        }
        Ok(())
    }
}

impl ValidationReport {
    /// Whether all constraints passed.
    pub fn is_valid(&self) -> bool {
        self.status != DriveStatus::Incompatible
    }

    /// Get only the failed constraints.
    pub fn failures(&self) -> Vec<&ConstraintResult> {
        self.constraints.iter().filter(|c| !c.passed).collect()
    }

    /// Get constraints by severity.
    pub fn by_severity(&self, severity: ConstraintSeverity) -> Vec<&ConstraintResult> {
        self.constraints
            .iter()
            .filter(|c| !c.passed && c.severity == severity)
            .collect()
    }
}

/// Drive constraint configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstraintConfig {
    /// Block system drives from being write targets.
    pub protect_system_drives: bool,
    /// Block read-only drives.
    pub block_read_only: bool,
    /// Minimum size the target must have (bytes). 0 = no minimum.
    pub minimum_size: u64,
    /// Recommended size (generates warning if smaller). 0 = no recommendation.
    pub recommended_size: u64,
    /// Path to the source image (to prevent writing to source device).
    pub source_path: Option<String>,
    /// Warn if the drive is much larger than needed (ratio threshold).
    pub large_drive_warning_ratio: f64,
    /// Minimum number of bytes that must remain free on the drive beyond the image.
    pub min_overhead_bytes: u64,
    /// Warn about non-removable drives.
    pub warn_non_removable: bool,
    /// Warn about drives with active mount points.
    pub warn_mounted: bool,
}

impl Default for ConstraintConfig {
    fn default() -> Self {
        Self {
            protect_system_drives: true,
            block_read_only: true,
            minimum_size: 0,
            recommended_size: 0,
            source_path: None,
            large_drive_warning_ratio: 16.0,
            min_overhead_bytes: 0,
            warn_non_removable: true,
            warn_mounted: true,
        }
    }
}

/// Check if a device is a system drive.
pub fn is_system_drive(device: &DeviceInfo) -> bool {
    if device.is_system {
        return true;
    }

    // Check mount points for system paths
    for mp in &device.mount_points {
        let mp_lower = mp.to_lowercase();
        // Windows system drive
        if mp_lower == "c:\\" || mp_lower == "c:" {
            return true;
        }
        // Unix root
        if mp == "/" {
            return true;
        }
        // macOS system volume
        if mp_lower.starts_with("/system") {
            return true;
        }
    }

    false
}

/// Check if a device contains the source image.
pub fn is_source_drive(device: &DeviceInfo, source_path: &str) -> bool {
    let source = Path::new(source_path);

    // If source path has no parent, it can't be on a drive
    let source_str = source.to_string_lossy().to_lowercase();

    // Check if any mount point is a prefix of the source path
    for mp in &device.mount_points {
        let mp_norm = mp.to_lowercase().replace('\\', "/");
        let src_norm = source_str.replace('\\', "/");
        if src_norm.starts_with(&mp_norm) {
            return true;
        }
    }

    // Check if device path might contain the source
    let dev_norm = device.path.to_lowercase();
    if source_str.contains(&dev_norm) {
        return true;
    }

    false
}

/// Check if a device is large enough for the image.
pub fn is_drive_large_enough(device: &DeviceInfo, required_size: u64) -> bool {
    if required_size == 0 {
        return true;
    }
    device.size >= required_size
}

/// Check if a device meets the recommended size.
pub fn is_drive_size_recommended(device: &DeviceInfo, recommended_size: u64) -> bool {
    if recommended_size == 0 {
        return true;
    }
    device.size >= recommended_size
}

/// Check if a device is suspiciously large for the image (might be wrong target).
pub fn is_drive_size_large(device: &DeviceInfo, image_size: u64, ratio: f64) -> bool {
    if image_size == 0 || ratio <= 0.0 {
        return false;
    }
    device.size as f64 > image_size as f64 * ratio
}

/// Check if a drive is locked or has active mount points.
pub fn is_drive_locked(device: &DeviceInfo) -> bool {
    device.read_only
}

/// Validate a drive against all configured constraints.
pub fn validate_drive(device: &DeviceInfo, config: &ConstraintConfig) -> ValidationReport {
    let mut constraints = Vec::new();
    let mut has_error = false;
    let mut has_warning = false;

    // 1. System drive protection
    if config.protect_system_drives {
        let is_sys = is_system_drive(device);
        constraints.push(ConstraintResult {
            name: "system-drive".to_string(),
            passed: !is_sys,
            severity: ConstraintSeverity::Error,
            message: if is_sys {
                "Drive appears to be a system drive — writing here could destroy your OS".to_string()
            } else {
                "Not a system drive".to_string()
            },
        });
        if is_sys {
            has_error = true;
        }
    }

    // 2. Read-only check
    if config.block_read_only {
        let is_ro = device.read_only;
        constraints.push(ConstraintResult {
            name: "read-only".to_string(),
            passed: !is_ro,
            severity: ConstraintSeverity::Error,
            message: if is_ro {
                "Drive is read-only or write-protected".to_string()
            } else {
                "Drive is writable".to_string()
            },
        });
        if is_ro {
            has_error = true;
        }
    }

    // 3. Source drive check
    if let Some(ref source) = config.source_path {
        let is_src = is_source_drive(device, source);
        constraints.push(ConstraintResult {
            name: "source-drive".to_string(),
            passed: !is_src,
            severity: ConstraintSeverity::Error,
            message: if is_src {
                format!(
                    "Drive contains the source image '{}' — cannot write to source",
                    source
                )
            } else {
                "Not the source drive".to_string()
            },
        });
        if is_src {
            has_error = true;
        }
    }

    // 4. Minimum size check
    if config.minimum_size > 0 {
        let large_enough = is_drive_large_enough(device, config.minimum_size);
        constraints.push(ConstraintResult {
            name: "minimum-size".to_string(),
            passed: large_enough,
            severity: ConstraintSeverity::Error,
            message: if large_enough {
                format!(
                    "Drive size ({}) meets minimum ({})",
                    humansize::format_size(device.size, humansize::BINARY),
                    humansize::format_size(config.minimum_size, humansize::BINARY),
                )
            } else {
                format!(
                    "Drive size ({}) is below minimum required ({})",
                    humansize::format_size(device.size, humansize::BINARY),
                    humansize::format_size(config.minimum_size, humansize::BINARY),
                )
            },
        });
        if !large_enough {
            has_error = true;
        }
    }

    // 5. Recommended size check
    if config.recommended_size > 0 {
        let recommended = is_drive_size_recommended(device, config.recommended_size);
        constraints.push(ConstraintResult {
            name: "recommended-size".to_string(),
            passed: recommended,
            severity: ConstraintSeverity::Warning,
            message: if recommended {
                format!(
                    "Drive size meets recommended ({})",
                    humansize::format_size(config.recommended_size, humansize::BINARY),
                )
            } else {
                format!(
                    "Drive size ({}) is below recommended ({})",
                    humansize::format_size(device.size, humansize::BINARY),
                    humansize::format_size(config.recommended_size, humansize::BINARY),
                )
            },
        });
        if !recommended {
            has_warning = true;
        }
    }

    // 6. Large drive warning
    if config.minimum_size > 0 && config.large_drive_warning_ratio > 0.0 {
        let too_large = is_drive_size_large(device, config.minimum_size, config.large_drive_warning_ratio);
        if too_large {
            constraints.push(ConstraintResult {
                name: "large-drive".to_string(),
                passed: false,
                severity: ConstraintSeverity::Warning,
                message: format!(
                    "Drive ({}) is much larger than needed ({}) — verify this is the correct target",
                    humansize::format_size(device.size, humansize::BINARY),
                    humansize::format_size(config.minimum_size, humansize::BINARY),
                ),
            });
            has_warning = true;
        }
    }

    // 7. Non-removable warning
    if config.warn_non_removable && !device.removable {
        constraints.push(ConstraintResult {
            name: "non-removable".to_string(),
            passed: false,
            severity: ConstraintSeverity::Warning,
            message: "Drive is non-removable (internal drive) — verify this is the correct target"
                .to_string(),
        });
        has_warning = true;
    }

    // 8. Mounted warning
    if config.warn_mounted && !device.mount_points.is_empty() {
        constraints.push(ConstraintResult {
            name: "mounted".to_string(),
            passed: false,
            severity: ConstraintSeverity::Warning,
            message: format!(
                "Drive has {} active mount point(s): {} — will be unmounted before writing",
                device.mount_points.len(),
                device.mount_points.join(", "),
            ),
        });
        has_warning = true;
    }

    let status = if has_error {
        DriveStatus::Incompatible
    } else if has_warning {
        DriveStatus::CompatibleWithWarnings
    } else {
        DriveStatus::Compatible
    };

    ValidationReport {
        device_path: device.path.clone(),
        status,
        constraints,
        image_source: config.source_path.clone(),
        required_size: if config.minimum_size > 0 {
            Some(config.minimum_size)
        } else {
            None
        },
        recommended_size: if config.recommended_size > 0 {
            Some(config.recommended_size)
        } else {
            None
        },
    }
}

/// Validate multiple drives and rank them by suitability.
pub fn validate_drives(
    devices: &[DeviceInfo],
    config: &ConstraintConfig,
) -> Vec<ValidationReport> {
    let mut reports: Vec<ValidationReport> = devices
        .iter()
        .map(|d| validate_drive(d, config))
        .collect();

    // Sort: compatible first, then by size (smallest suitable first)
    reports.sort_by(|a, b| {
        let a_score = match a.status {
            DriveStatus::Compatible => 0,
            DriveStatus::CompatibleWithWarnings => 1,
            DriveStatus::Incompatible => 2,
        };
        let b_score = match b.status {
            DriveStatus::Compatible => 0,
            DriveStatus::CompatibleWithWarnings => 1,
            DriveStatus::Incompatible => 2,
        };
        a_score.cmp(&b_score)
    });

    reports
}

/// Auto-select the best compatible drive from a list.
pub fn auto_select_drive(
    devices: &[DeviceInfo],
    config: &ConstraintConfig,
) -> Option<(DeviceInfo, ValidationReport)> {
    for device in devices {
        let report = validate_drive(device, config);
        if report.status == DriveStatus::Compatible {
            return Some((device.clone(), report));
        }
    }
    None
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::DeviceType;

    fn test_device(path: &str, size: u64) -> DeviceInfo {
        DeviceInfo {
            path: path.to_string(),
            name: "Test Drive".to_string(),
            vendor: "TestVendor".to_string(),
            serial: Some("SN123".to_string()),
            size,
            sector_size: 512,
            physical_sector_size: 512,
            removable: true,
            read_only: false,
            is_system: false,
            device_type: DeviceType::Usb,
            mount_points: vec![],
            transport: "USB".to_string(),
        }
    }

    #[test]
    fn test_constraint_severity_display() {
        assert_eq!(ConstraintSeverity::Info.to_string(), "info");
        assert_eq!(ConstraintSeverity::Warning.to_string(), "warning");
        assert_eq!(ConstraintSeverity::Error.to_string(), "error");
    }

    #[test]
    fn test_drive_status_display() {
        assert_eq!(DriveStatus::Compatible.to_string(), "compatible");
        assert_eq!(
            DriveStatus::CompatibleWithWarnings.to_string(),
            "compatible (with warnings)"
        );
        assert_eq!(DriveStatus::Incompatible.to_string(), "incompatible");
    }

    #[test]
    fn test_is_system_drive_flag() {
        let mut dev = test_device("/dev/sda", 500_000_000_000);
        assert!(!is_system_drive(&dev));
        dev.is_system = true;
        assert!(is_system_drive(&dev));
    }

    #[test]
    fn test_is_system_drive_mount_point() {
        let mut dev = test_device("/dev/sda", 500_000_000_000);
        dev.mount_points = vec!["/".to_string()];
        assert!(is_system_drive(&dev));

        dev.mount_points = vec!["C:\\".to_string()];
        assert!(is_system_drive(&dev));

        dev.mount_points = vec!["/mnt/usb".to_string()];
        assert!(!is_system_drive(&dev));
    }

    #[test]
    fn test_is_source_drive() {
        let mut dev = test_device("/dev/sdb", 8_000_000_000);
        dev.mount_points = vec!["/media/user/usb".to_string()];
        assert!(is_source_drive(
            &dev,
            "/media/user/usb/ubuntu.iso"
        ));
        assert!(!is_source_drive(&dev, "/home/user/ubuntu.iso"));
    }

    #[test]
    fn test_is_drive_large_enough() {
        let dev = test_device("/dev/sdb", 8_000_000_000);
        assert!(is_drive_large_enough(&dev, 4_000_000_000));
        assert!(is_drive_large_enough(&dev, 8_000_000_000));
        assert!(!is_drive_large_enough(&dev, 16_000_000_000));
        assert!(is_drive_large_enough(&dev, 0)); // 0 = no minimum
    }

    #[test]
    fn test_is_drive_size_recommended() {
        let dev = test_device("/dev/sdb", 8_000_000_000);
        assert!(is_drive_size_recommended(&dev, 4_000_000_000));
        assert!(!is_drive_size_recommended(&dev, 16_000_000_000));
    }

    #[test]
    fn test_is_drive_size_large() {
        let dev = test_device("/dev/sdb", 500_000_000_000); // 500 GB
        assert!(is_drive_size_large(&dev, 4_000_000_000, 16.0)); // 500GB >> 4GB * 16
        assert!(!is_drive_size_large(&dev, 100_000_000_000, 16.0)); // 500GB < 100GB * 16
    }

    #[test]
    fn test_is_drive_locked() {
        let mut dev = test_device("/dev/sdb", 8_000_000_000);
        assert!(!is_drive_locked(&dev));
        dev.read_only = true;
        assert!(is_drive_locked(&dev));
    }

    #[test]
    fn test_validate_drive_compatible() {
        let dev = test_device("/dev/sdb", 8_000_000_000);
        let config = ConstraintConfig {
            minimum_size: 4_000_000_000,
            ..Default::default()
        };
        let report = validate_drive(&dev, &config);
        assert_eq!(report.status, DriveStatus::Compatible);
        assert!(report.is_valid());
    }

    #[test]
    fn test_validate_drive_system_blocked() {
        let mut dev = test_device("/dev/sda", 500_000_000_000);
        dev.is_system = true;
        let config = ConstraintConfig::default();
        let report = validate_drive(&dev, &config);
        assert_eq!(report.status, DriveStatus::Incompatible);
        assert!(!report.is_valid());
        let failures = report.failures();
        assert!(failures.iter().any(|c| c.name == "system-drive"));
    }

    #[test]
    fn test_validate_drive_read_only() {
        let mut dev = test_device("/dev/sdb", 8_000_000_000);
        dev.read_only = true;
        let config = ConstraintConfig::default();
        let report = validate_drive(&dev, &config);
        assert_eq!(report.status, DriveStatus::Incompatible);
        let failures = report.failures();
        assert!(failures.iter().any(|c| c.name == "read-only"));
    }

    #[test]
    fn test_validate_drive_too_small() {
        let dev = test_device("/dev/sdb", 2_000_000_000); // 2 GB
        let config = ConstraintConfig {
            minimum_size: 4_000_000_000, // needs 4 GB
            ..Default::default()
        };
        let report = validate_drive(&dev, &config);
        assert_eq!(report.status, DriveStatus::Incompatible);
    }

    #[test]
    fn test_validate_drive_source_conflict() {
        let mut dev = test_device("/dev/sdb", 8_000_000_000);
        dev.mount_points = vec!["/media/usb".to_string()];
        let config = ConstraintConfig {
            source_path: Some("/media/usb/image.iso".to_string()),
            ..Default::default()
        };
        let report = validate_drive(&dev, &config);
        assert_eq!(report.status, DriveStatus::Incompatible);
    }

    #[test]
    fn test_validate_drive_warning_mounted() {
        let mut dev = test_device("/dev/sdb", 8_000_000_000);
        dev.mount_points = vec!["/mnt/usb".to_string()];
        let config = ConstraintConfig::default();
        let report = validate_drive(&dev, &config);
        assert_eq!(report.status, DriveStatus::CompatibleWithWarnings);
    }

    #[test]
    fn test_validate_drive_warning_non_removable() {
        let mut dev = test_device("/dev/sdb", 8_000_000_000);
        dev.removable = false;
        let config = ConstraintConfig::default();
        let report = validate_drive(&dev, &config);
        assert_eq!(report.status, DriveStatus::CompatibleWithWarnings);
    }

    #[test]
    fn test_validate_drives_sorted() {
        let dev1 = {
            let mut d = test_device("/dev/sdb", 8_000_000_000);
            d.removable = false; // will get warning
            d
        };
        let dev2 = test_device("/dev/sdc", 16_000_000_000); // fully compatible
        let mut dev3 = test_device("/dev/sdd", 8_000_000_000);
        dev3.read_only = true; // incompatible

        let config = ConstraintConfig::default();
        let reports = validate_drives(&[dev1, dev2, dev3], &config);
        assert_eq!(reports[0].status, DriveStatus::Compatible);
        assert_eq!(reports[1].status, DriveStatus::CompatibleWithWarnings);
        assert_eq!(reports[2].status, DriveStatus::Incompatible);
    }

    #[test]
    fn test_auto_select_drive() {
        let mut dev1 = test_device("/dev/sdb", 8_000_000_000);
        dev1.removable = false; // warning
        let dev2 = test_device("/dev/sdc", 16_000_000_000); // compatible

        let config = ConstraintConfig::default();
        let result = auto_select_drive(&[dev1, dev2], &config);
        assert!(result.is_some());
        let (selected, _) = result.unwrap();
        assert_eq!(selected.path, "/dev/sdc");
    }

    #[test]
    fn test_auto_select_no_compatible() {
        let mut dev1 = test_device("/dev/sda", 500_000_000_000);
        dev1.is_system = true;
        let mut dev2 = test_device("/dev/sdb", 8_000_000_000);
        dev2.read_only = true;

        let config = ConstraintConfig::default();
        let result = auto_select_drive(&[dev1, dev2], &config);
        assert!(result.is_none());
    }

    #[test]
    fn test_validation_report_display() {
        let dev = test_device("/dev/sdb", 8_000_000_000);
        let config = ConstraintConfig::default();
        let report = validate_drive(&dev, &config);
        let s = report.to_string();
        assert!(s.contains("/dev/sdb"));
    }

    #[test]
    fn test_constraint_result_display() {
        let cr = ConstraintResult {
            name: "test".to_string(),
            passed: true,
            severity: ConstraintSeverity::Info,
            message: "All good".to_string(),
        };
        assert!(cr.to_string().contains("PASS"));

        let cr_fail = ConstraintResult {
            name: "test".to_string(),
            passed: false,
            severity: ConstraintSeverity::Error,
            message: "Bad".to_string(),
        };
        assert!(cr_fail.to_string().contains("FAIL"));
    }

    #[test]
    fn test_report_by_severity() {
        let mut dev = test_device("/dev/sdb", 2_000_000_000);
        dev.removable = false;
        let config = ConstraintConfig {
            minimum_size: 4_000_000_000,
            ..Default::default()
        };
        let report = validate_drive(&dev, &config);
        let errors = report.by_severity(ConstraintSeverity::Error);
        let warnings = report.by_severity(ConstraintSeverity::Warning);
        assert!(!errors.is_empty());
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_default_config() {
        let config = ConstraintConfig::default();
        assert!(config.protect_system_drives);
        assert!(config.block_read_only);
        assert_eq!(config.minimum_size, 0);
        assert_eq!(config.recommended_size, 0);
        assert!(config.source_path.is_none());
        assert_eq!(config.large_drive_warning_ratio, 16.0);
        assert!(config.warn_non_removable);
        assert!(config.warn_mounted);
    }
}
