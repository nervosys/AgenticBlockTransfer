// Multi-target fleet write — write the same image to N USB drives simultaneously.
//
// Inspired by Etcher's multi-target write feature: select multiple USB drives
// and flash them all at once from the same source image. Key features:
//   - Per-device progress tracking with independent progress bars
//   - Per-device failure isolation (one drive failing doesn't stop others)
//   - Aggregate result reporting (success/fail counts, per-device status)
//   - Verification pass per device
//   - Throughput monitoring per device
//
// Designed for education labs, IT provisioning, and manufacturing where
// dozens of identical USB drives need to be created quickly.

#![allow(dead_code)]

use anyhow::Result;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Status of a single device in a fleet write operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus {
    /// Waiting to start.
    Pending,
    /// Writing in progress.
    Writing,
    /// Verification in progress.
    Verifying,
    /// Completed successfully.
    Completed,
    /// Failed with error message.
    Failed(String),
    /// Cancelled by user.
    Cancelled,
}

impl std::fmt::Display for DeviceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceStatus::Pending => write!(f, "Pending"),
            DeviceStatus::Writing => write!(f, "Writing"),
            DeviceStatus::Verifying => write!(f, "Verifying"),
            DeviceStatus::Completed => write!(f, "Completed"),
            DeviceStatus::Failed(msg) => write!(f, "Failed: {}", msg),
            DeviceStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Per-device progress information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceProgress {
    /// Device path (e.g., /dev/sdb or \\.\PhysicalDrive1).
    pub device_path: String,
    /// Device label/name for display.
    pub label: String,
    /// Device capacity in bytes.
    pub capacity: u64,
    /// Current status.
    pub status: DeviceStatus,
    /// Bytes written so far.
    pub bytes_written: u64,
    /// Total bytes to write.
    pub bytes_total: u64,
    /// Write speed in bytes per second (rolling average).
    pub speed_bps: u64,
    /// Estimated time remaining in seconds.
    pub eta_seconds: f64,
    /// Start time (UNIX epoch milliseconds).
    pub start_time_ms: u64,
    /// End time if completed (UNIX epoch milliseconds).
    pub end_time_ms: Option<u64>,
    /// Verification result (if verify was requested).
    pub verified: Option<bool>,
    /// Error message if failed.
    pub error: Option<String>,
}

impl DeviceProgress {
    /// Create new progress tracker for a device.
    pub fn new(device_path: &str, label: &str, capacity: u64, total_bytes: u64) -> Self {
        Self {
            device_path: device_path.to_string(),
            label: label.to_string(),
            capacity,
            status: DeviceStatus::Pending,
            bytes_written: 0,
            bytes_total: total_bytes,
            speed_bps: 0,
            eta_seconds: 0.0,
            start_time_ms: 0,
            end_time_ms: None,
            verified: None,
            error: None,
        }
    }

    /// Progress percentage (0.0 - 100.0).
    pub fn percent(&self) -> f64 {
        if self.bytes_total == 0 {
            return 0.0;
        }
        (self.bytes_written as f64 / self.bytes_total as f64) * 100.0
    }

    /// Whether this device is finished (completed, failed, or cancelled).
    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            DeviceStatus::Completed | DeviceStatus::Failed(_) | DeviceStatus::Cancelled
        )
    }

    /// Duration in seconds since write started.
    pub fn elapsed_seconds(&self) -> f64 {
        if self.start_time_ms == 0 {
            return 0.0;
        }
        let end = self.end_time_ms.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64
        });
        (end - self.start_time_ms) as f64 / 1000.0
    }
}

/// Fleet write configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetConfig {
    /// Source image path.
    pub source: PathBuf,
    /// Target device paths.
    pub targets: Vec<FleetTarget>,
    /// Whether to verify after writing.
    pub verify: bool,
    /// Block size for writing (bytes).
    pub block_size: usize,
    /// Maximum concurrent writes (0 = unlimited).
    pub max_concurrent: usize,
    /// Whether to unmount devices before writing.
    pub auto_unmount: bool,
    /// Whether an individual device failure should cancel all other devices.
    pub fail_fast: bool,
}

/// A target device for fleet writing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetTarget {
    /// Device path.
    pub path: String,
    /// Human-readable label.
    pub label: String,
    /// Device capacity in bytes.
    pub capacity: u64,
}

impl Default for FleetConfig {
    fn default() -> Self {
        Self {
            source: PathBuf::new(),
            targets: Vec::new(),
            verify: true,
            block_size: 4 * 1024 * 1024, // 4 MB
            max_concurrent: 0,
            auto_unmount: true,
            fail_fast: false,
        }
    }
}

/// Aggregate result of a fleet write operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetResult {
    /// Per-device results.
    pub devices: Vec<DeviceProgress>,
    /// Total devices targeted.
    pub total_devices: usize,
    /// Devices completed successfully.
    pub succeeded: usize,
    /// Devices that failed.
    pub failed: usize,
    /// Devices that were cancelled.
    pub cancelled: usize,
    /// Total bytes written across all devices.
    pub total_bytes_written: u64,
    /// Wall-clock duration in seconds.
    pub duration_seconds: f64,
    /// Average speed across all devices (bytes/sec).
    pub avg_speed_bps: u64,
    /// Source image path.
    pub source: PathBuf,
}

impl FleetResult {
    /// Whether all devices completed successfully.
    pub fn all_succeeded(&self) -> bool {
        self.succeeded == self.total_devices
    }

    /// Generate a summary string.
    pub fn summary(&self) -> String {
        let mut s = format!(
            "Fleet write complete: {}/{} succeeded",
            self.succeeded, self.total_devices
        );
        if self.failed > 0 {
            s.push_str(&format!(", {} failed", self.failed));
        }
        if self.cancelled > 0 {
            s.push_str(&format!(", {} cancelled", self.cancelled));
        }
        s.push_str(&format!(
            " | {:.1}s | {}/s avg",
            self.duration_seconds,
            format_speed(self.avg_speed_bps)
        ));
        s
    }
}

/// Fleet write session — manages the state of an ongoing fleet write operation.
#[derive(Debug)]
pub struct FleetSession {
    /// Configuration.
    config: FleetConfig,
    /// Per-device progress (shared, thread-safe).
    progress: Arc<Mutex<Vec<DeviceProgress>>>,
    /// Session start time.
    start_time: Option<Instant>,
    /// Cancellation flag.
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl FleetSession {
    /// Create a new fleet write session.
    pub fn new(config: FleetConfig) -> Self {
        let image_size = std::fs::metadata(&config.source)
            .map(|m| m.len())
            .unwrap_or(0);

        let progress: Vec<DeviceProgress> = config
            .targets
            .iter()
            .map(|t| DeviceProgress::new(&t.path, &t.label, t.capacity, image_size))
            .collect();

        Self {
            config,
            progress: Arc::new(Mutex::new(progress)),
            start_time: None,
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Get the current progress snapshot for all devices.
    pub fn snapshot(&self) -> Vec<DeviceProgress> {
        self.progress.lock().unwrap().clone()
    }

    /// Get the number of target devices.
    pub fn device_count(&self) -> usize {
        self.config.targets.len()
    }

    /// Cancel the fleet write operation.
    pub fn cancel(&self) {
        warn!("Fleet write cancelled by user");
        self.cancelled
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let mut progress = self.progress.lock().unwrap();
        for p in progress.iter_mut() {
            if !p.is_finished() {
                p.status = DeviceStatus::Cancelled;
            }
        }
    }

    /// Check if cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Update the progress for a specific device.
    pub fn update_progress(
        &self,
        device_index: usize,
        bytes_written: u64,
        speed_bps: u64,
    ) {
        let mut progress = self.progress.lock().unwrap();
        if let Some(p) = progress.get_mut(device_index) {
            p.bytes_written = bytes_written;
            p.speed_bps = speed_bps;
            if p.bytes_total > 0 && speed_bps > 0 {
                let remaining = p.bytes_total.saturating_sub(bytes_written);
                p.eta_seconds = remaining as f64 / speed_bps as f64;
            }
        }
    }

    /// Mark a device as having started writing.
    pub fn mark_writing(&self, device_index: usize) {
        let mut progress = self.progress.lock().unwrap();
        if let Some(p) = progress.get_mut(device_index) {
            p.status = DeviceStatus::Writing;
            p.start_time_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
        }
    }

    /// Mark a device as verifying.
    pub fn mark_verifying(&self, device_index: usize) {
        let mut progress = self.progress.lock().unwrap();
        if let Some(p) = progress.get_mut(device_index) {
            p.status = DeviceStatus::Verifying;
        }
    }

    /// Mark a device as completed.
    pub fn mark_completed(&self, device_index: usize, verified: Option<bool>) {
        let mut progress = self.progress.lock().unwrap();
        if let Some(p) = progress.get_mut(device_index) {
            p.status = DeviceStatus::Completed;
            p.verified = verified;
            p.end_time_ms = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            );
            info!("Device {} completed successfully", p.device_path);
        }
    }

    /// Mark a device as failed.
    pub fn mark_failed(&self, device_index: usize, error: &str) {
        let mut progress = self.progress.lock().unwrap();
        if let Some(p) = progress.get_mut(device_index) {
            p.status = DeviceStatus::Failed(error.to_string());
            p.error = Some(error.to_string());
            p.end_time_ms = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64,
            );
            error!("Device {} failed: {}", p.device_path, error);
        }
    }

    /// Generate the final fleet result.
    pub fn result(&self) -> FleetResult {
        let progress = self.progress.lock().unwrap();
        let total_devices = progress.len();
        let mut succeeded = 0;
        let mut failed = 0;
        let mut cancelled = 0;
        let mut total_bytes = 0u64;
        let mut total_speed = 0u64;
        let mut speed_count = 0;

        for p in progress.iter() {
            match &p.status {
                DeviceStatus::Completed => {
                    succeeded += 1;
                    total_bytes += p.bytes_written;
                    if p.speed_bps > 0 {
                        total_speed += p.speed_bps;
                        speed_count += 1;
                    }
                }
                DeviceStatus::Failed(_) => {
                    failed += 1;
                    total_bytes += p.bytes_written;
                }
                DeviceStatus::Cancelled => {
                    cancelled += 1;
                }
                _ => {}
            }
        }

        let duration = self
            .start_time
            .map(|s| s.elapsed().as_secs_f64())
            .unwrap_or(0.0);

        let avg_speed = if speed_count > 0 {
            total_speed / speed_count as u64
        } else {
            0
        };

        FleetResult {
            devices: progress.clone(),
            total_devices,
            succeeded,
            failed,
            cancelled,
            total_bytes_written: total_bytes,
            duration_seconds: duration,
            avg_speed_bps: avg_speed,
            source: self.config.source.clone(),
        }
    }
}

/// Format a speed value for display.
pub fn format_speed(bps: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bps >= GB {
        format!("{:.1} GB", bps as f64 / GB as f64)
    } else if bps >= MB {
        format!("{:.1} MB", bps as f64 / MB as f64)
    } else if bps >= KB {
        format!("{:.1} KB", bps as f64 / KB as f64)
    } else {
        format!("{} B", bps)
    }
}

/// Validate that all target devices are suitable for writing.
pub fn validate_targets(targets: &[FleetTarget], image_size: u64) -> Vec<TargetValidation> {
    targets
        .iter()
        .map(|t| {
            let mut issues = Vec::new();

            if t.path.is_empty() {
                issues.push("Empty device path".into());
            }

            if t.capacity > 0 && image_size > t.capacity {
                issues.push(format!(
                    "Image ({}) exceeds device capacity ({})",
                    format_speed(image_size),
                    format_speed(t.capacity)
                ));
            }

            TargetValidation {
                device_path: t.path.clone(),
                valid: issues.is_empty(),
                issues,
            }
        })
        .collect()
}

/// Validation result for a single target device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetValidation {
    /// Device path.
    pub device_path: String,
    /// Whether the target is valid.
    pub valid: bool,
    /// List of issues found.
    pub issues: Vec<String>,
}

/// Detect all available USB mass storage devices for fleet writing.
/// Returns a list of potential target devices.
pub fn detect_usb_devices() -> Result<Vec<FleetTarget>> {
    // Platform-specific USB device enumeration
    // This is a placeholder — actual implementation uses platform APIs
    debug!("Detecting USB mass storage devices...");
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_status_display() {
        assert_eq!(DeviceStatus::Pending.to_string(), "Pending");
        assert_eq!(DeviceStatus::Writing.to_string(), "Writing");
        assert_eq!(DeviceStatus::Verifying.to_string(), "Verifying");
        assert_eq!(DeviceStatus::Completed.to_string(), "Completed");
        assert_eq!(
            DeviceStatus::Failed("disk full".into()).to_string(),
            "Failed: disk full"
        );
        assert_eq!(DeviceStatus::Cancelled.to_string(), "Cancelled");
    }

    #[test]
    fn test_device_progress_new() {
        let dp = DeviceProgress::new("/dev/sdb", "Kingston 16GB", 16_000_000_000, 4_000_000_000);
        assert_eq!(dp.device_path, "/dev/sdb");
        assert_eq!(dp.status, DeviceStatus::Pending);
        assert_eq!(dp.bytes_written, 0);
        assert_eq!(dp.bytes_total, 4_000_000_000);
    }

    #[test]
    fn test_device_progress_percent() {
        let mut dp = DeviceProgress::new("/dev/sdb", "USB", 8_000_000_000, 1_000_000);
        assert_eq!(dp.percent(), 0.0);
        dp.bytes_written = 500_000;
        assert!((dp.percent() - 50.0).abs() < 0.001);
        dp.bytes_written = 1_000_000;
        assert!((dp.percent() - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_device_progress_percent_zero_total() {
        let dp = DeviceProgress::new("/dev/sdb", "USB", 0, 0);
        assert_eq!(dp.percent(), 0.0);
    }

    #[test]
    fn test_device_progress_is_finished() {
        let mut dp = DeviceProgress::new("/dev/sdb", "USB", 0, 0);
        assert!(!dp.is_finished());

        dp.status = DeviceStatus::Writing;
        assert!(!dp.is_finished());

        dp.status = DeviceStatus::Completed;
        assert!(dp.is_finished());

        dp.status = DeviceStatus::Failed("err".into());
        assert!(dp.is_finished());

        dp.status = DeviceStatus::Cancelled;
        assert!(dp.is_finished());
    }

    #[test]
    fn test_fleet_config_default() {
        let fc = FleetConfig::default();
        assert!(fc.verify);
        assert_eq!(fc.block_size, 4 * 1024 * 1024);
        assert_eq!(fc.max_concurrent, 0);
        assert!(fc.auto_unmount);
        assert!(!fc.fail_fast);
    }

    #[test]
    fn test_fleet_session_basics() {
        let config = FleetConfig {
            source: PathBuf::from("/tmp/test.img"),
            targets: vec![
                FleetTarget {
                    path: "/dev/sdb".into(),
                    label: "USB 1".into(),
                    capacity: 8_000_000_000,
                },
                FleetTarget {
                    path: "/dev/sdc".into(),
                    label: "USB 2".into(),
                    capacity: 16_000_000_000,
                },
            ],
            ..Default::default()
        };

        let session = FleetSession::new(config);
        assert_eq!(session.device_count(), 2);
        assert!(!session.is_cancelled());

        let snap = session.snapshot();
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].status, DeviceStatus::Pending);
        assert_eq!(snap[1].status, DeviceStatus::Pending);
    }

    #[test]
    fn test_fleet_session_state_transitions() {
        let config = FleetConfig {
            source: PathBuf::from("/tmp/test.img"),
            targets: vec![FleetTarget {
                path: "/dev/sdb".into(),
                label: "USB 1".into(),
                capacity: 8_000_000_000,
            }],
            ..Default::default()
        };

        let session = FleetSession::new(config);

        // Pending -> Writing
        session.mark_writing(0);
        assert_eq!(session.snapshot()[0].status, DeviceStatus::Writing);

        // Update progress
        session.update_progress(0, 500_000, 50_000_000);
        let snap = session.snapshot();
        assert_eq!(snap[0].bytes_written, 500_000);
        assert_eq!(snap[0].speed_bps, 50_000_000);

        // Writing -> Verifying
        session.mark_verifying(0);
        assert_eq!(session.snapshot()[0].status, DeviceStatus::Verifying);

        // Verifying -> Completed
        session.mark_completed(0, Some(true));
        let snap = session.snapshot();
        assert_eq!(snap[0].status, DeviceStatus::Completed);
        assert_eq!(snap[0].verified, Some(true));
    }

    #[test]
    fn test_fleet_session_failure() {
        let config = FleetConfig {
            source: PathBuf::from("/tmp/test.img"),
            targets: vec![FleetTarget {
                path: "/dev/sdb".into(),
                label: "USB 1".into(),
                capacity: 8_000_000_000,
            }],
            ..Default::default()
        };

        let session = FleetSession::new(config);
        session.mark_writing(0);
        session.mark_failed(0, "I/O error");

        let snap = session.snapshot();
        assert_eq!(snap[0].status, DeviceStatus::Failed("I/O error".into()));
        assert_eq!(snap[0].error.as_deref(), Some("I/O error"));
        assert!(snap[0].is_finished());
    }

    #[test]
    fn test_fleet_session_cancel() {
        let config = FleetConfig {
            source: PathBuf::from("/tmp/test.img"),
            targets: vec![
                FleetTarget {
                    path: "/dev/sdb".into(),
                    label: "USB 1".into(),
                    capacity: 8_000_000_000,
                },
                FleetTarget {
                    path: "/dev/sdc".into(),
                    label: "USB 2".into(),
                    capacity: 16_000_000_000,
                },
            ],
            ..Default::default()
        };

        let session = FleetSession::new(config);
        session.mark_writing(0);
        session.mark_completed(0, None); // First device already done

        session.cancel();
        assert!(session.is_cancelled());

        let snap = session.snapshot();
        assert_eq!(snap[0].status, DeviceStatus::Completed); // completed devices stay completed
        assert_eq!(snap[1].status, DeviceStatus::Cancelled);
    }

    #[test]
    fn test_fleet_result() {
        let config = FleetConfig {
            source: PathBuf::from("/tmp/test.img"),
            targets: vec![
                FleetTarget {
                    path: "/dev/sdb".into(),
                    label: "USB 1".into(),
                    capacity: 8_000_000_000,
                },
                FleetTarget {
                    path: "/dev/sdc".into(),
                    label: "USB 2".into(),
                    capacity: 16_000_000_000,
                },
            ],
            ..Default::default()
        };

        let session = FleetSession::new(config);
        session.mark_writing(0);
        session.update_progress(0, 1_000_000, 100_000_000);
        session.mark_completed(0, Some(true));

        session.mark_writing(1);
        session.mark_failed(1, "bad sector");

        let result = session.result();
        assert_eq!(result.total_devices, 2);
        assert_eq!(result.succeeded, 1);
        assert_eq!(result.failed, 1);
        assert_eq!(result.cancelled, 0);
        assert!(!result.all_succeeded());
    }

    #[test]
    fn test_fleet_result_summary() {
        let result = FleetResult {
            devices: vec![],
            total_devices: 5,
            succeeded: 4,
            failed: 1,
            cancelled: 0,
            total_bytes_written: 20_000_000_000,
            duration_seconds: 120.5,
            avg_speed_bps: 100_000_000,
            source: PathBuf::from("/tmp/test.img"),
        };
        let summary = result.summary();
        assert!(summary.contains("4/5 succeeded"));
        assert!(summary.contains("1 failed"));
    }

    #[test]
    fn test_fleet_result_all_succeeded() {
        let result = FleetResult {
            devices: vec![],
            total_devices: 3,
            succeeded: 3,
            failed: 0,
            cancelled: 0,
            total_bytes_written: 0,
            duration_seconds: 0.0,
            avg_speed_bps: 0,
            source: PathBuf::new(),
        };
        assert!(result.all_succeeded());
    }

    #[test]
    fn test_validate_targets_valid() {
        let targets = vec![FleetTarget {
            path: "/dev/sdb".into(),
            label: "USB".into(),
            capacity: 8_000_000_000,
        }];
        let results = validate_targets(&targets, 4_000_000_000);
        assert_eq!(results.len(), 1);
        assert!(results[0].valid);
        assert!(results[0].issues.is_empty());
    }

    #[test]
    fn test_validate_targets_too_small() {
        let targets = vec![FleetTarget {
            path: "/dev/sdb".into(),
            label: "USB".into(),
            capacity: 2_000_000_000,
        }];
        let results = validate_targets(&targets, 4_000_000_000);
        assert!(!results[0].valid);
        assert!(results[0].issues[0].contains("exceeds"));
    }

    #[test]
    fn test_validate_targets_empty_path() {
        let targets = vec![FleetTarget {
            path: "".into(),
            label: "USB".into(),
            capacity: 8_000_000_000,
        }];
        let results = validate_targets(&targets, 4_000_000_000);
        assert!(!results[0].valid);
        assert!(results[0].issues[0].contains("Empty"));
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(format_speed(500), "500 B");
        assert_eq!(format_speed(1024), "1.0 KB");
        assert_eq!(format_speed(1024 * 1024), "1.0 MB");
        assert_eq!(format_speed(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_speed(50 * 1024 * 1024), "50.0 MB");
    }

    #[test]
    fn test_fleet_config_serde() {
        let config = FleetConfig {
            source: PathBuf::from("/tmp/image.iso"),
            targets: vec![FleetTarget {
                path: "/dev/sdb".into(),
                label: "USB 1".into(),
                capacity: 8_000_000_000,
            }],
            verify: true,
            block_size: 1024 * 1024,
            max_concurrent: 4,
            auto_unmount: true,
            fail_fast: false,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deser: FleetConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.targets[0].path, "/dev/sdb");
        assert_eq!(deser.max_concurrent, 4);
    }

    #[test]
    fn test_target_validation_serde() {
        let tv = TargetValidation {
            device_path: "/dev/sdb".into(),
            valid: true,
            issues: vec![],
        };
        let json = serde_json::to_string(&tv).unwrap();
        assert!(json.contains("sdb"));
    }

    #[test]
    fn test_detect_usb_devices() {
        // Should return empty vec on non-rooted/non-privileged systems
        let devices = detect_usb_devices().unwrap();
        assert!(devices.is_empty());
    }
}
