// Write watchdog — stall detection and automatic recovery for long-running I/O.
//
// Inspired by rpi-imager's WriteProgressWatchdog which monitors write progress
// and takes corrective action when stalls are detected:
//   - Heartbeat monitoring: checks that bytes_written is advancing
//   - Stall detection: configurable timeout before declaring a stall
//   - Queue depth reduction: reduce async I/O queue depth on stall
//   - Async-to-sync fallback: fall back to synchronous I/O if async stalls
//   - Progress watchdog thread: runs in background, checks periodically
//
// The watchdog tracks the progress of any I/O operation and can trigger
// recovery actions when progress stalls. This is critical for USB drives
// which may have unpredictable write latency spikes.

#![allow(dead_code)]

use anyhow::{bail, Result};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Recovery action to take when a stall is detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryAction {
    /// Log a warning but continue waiting.
    Warn,
    /// Reduce async I/O queue depth.
    ReduceQueueDepth,
    /// Switch from async to synchronous I/O.
    FallbackToSync,
    /// Cancel the operation.
    Cancel,
}

impl std::fmt::Display for RecoveryAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecoveryAction::Warn => write!(f, "Warn"),
            RecoveryAction::ReduceQueueDepth => write!(f, "ReduceQueueDepth"),
            RecoveryAction::FallbackToSync => write!(f, "FallbackToSync"),
            RecoveryAction::Cancel => write!(f, "Cancel"),
        }
    }
}

/// Severity level for stall events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StallSeverity {
    /// Minor stall (< 2x threshold).
    Minor,
    /// Major stall (2x-5x threshold).
    Major,
    /// Critical stall (> 5x threshold).
    Critical,
}

impl std::fmt::Display for StallSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StallSeverity::Minor => write!(f, "Minor"),
            StallSeverity::Major => write!(f, "Major"),
            StallSeverity::Critical => write!(f, "Critical"),
        }
    }
}

/// Watchdog configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchdogConfig {
    /// How often to check for progress (default: 2 seconds).
    pub check_interval: Duration,
    /// Time without progress before a stall is declared (default: 10 seconds).
    pub stall_timeout: Duration,
    /// Maximum allowed stalls before cancelling (0 = unlimited).
    pub max_stalls: u32,
    /// Escalation chain: first stall → Warn, then ReduceQueueDepth, etc.
    pub escalation: Vec<RecoveryAction>,
    /// Minimum queue depth after reduction (default: 1).
    pub min_queue_depth: u32,
    /// Queue depth reduction factor (halve each time).
    pub queue_depth_factor: f64,
    /// Whether the watchdog is enabled.
    pub enabled: bool,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            check_interval: Duration::from_secs(2),
            stall_timeout: Duration::from_secs(10),
            max_stalls: 10,
            escalation: vec![
                RecoveryAction::Warn,
                RecoveryAction::ReduceQueueDepth,
                RecoveryAction::ReduceQueueDepth,
                RecoveryAction::FallbackToSync,
                RecoveryAction::Cancel,
            ],
            min_queue_depth: 1,
            queue_depth_factor: 0.5,
            enabled: true,
        }
    }
}

impl WatchdogConfig {
    /// Create a lenient config (longer timeouts, more retries).
    pub fn lenient() -> Self {
        Self {
            stall_timeout: Duration::from_secs(30),
            max_stalls: 20,
            ..Default::default()
        }
    }

    /// Create a strict config (shorter timeouts, fewer retries).
    pub fn strict() -> Self {
        Self {
            stall_timeout: Duration::from_secs(5),
            max_stalls: 5,
            check_interval: Duration::from_secs(1),
            ..Default::default()
        }
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.check_interval.as_millis() < 100 {
            bail!("Check interval too short (minimum 100ms)");
        }
        if self.stall_timeout < self.check_interval {
            bail!("Stall timeout must be >= check interval");
        }
        if self.escalation.is_empty() {
            bail!("Escalation chain must not be empty");
        }
        if self.queue_depth_factor <= 0.0 || self.queue_depth_factor >= 1.0 {
            bail!("Queue depth factor must be between 0 and 1 (exclusive)");
        }
        Ok(())
    }
}

/// A recorded stall event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StallEvent {
    /// When the stall started (seconds since operation start).
    pub started_at_secs: f64,
    /// How long the stall lasted (seconds).
    pub duration_secs: f64,
    /// Stall severity.
    pub severity: StallSeverity,
    /// Recovery action taken.
    pub action: RecoveryAction,
    /// Queue depth at time of stall.
    pub queue_depth: u32,
    /// Bytes written at time of stall.
    pub bytes_at_stall: u64,
}

/// Watchdog state — shared between the monitor and the I/O thread.
#[derive(Debug)]
pub struct WatchdogState {
    /// Bytes written (updated by the I/O thread).
    pub bytes_written: AtomicU64,
    /// Total bytes expected.
    pub bytes_total: AtomicU64,
    /// Current queue depth.
    pub queue_depth: AtomicU64,
    /// Whether to use sync I/O (set by watchdog on fallback).
    pub use_sync_io: AtomicBool,
    /// Whether the operation is cancelled.
    pub cancelled: AtomicBool,
    /// Whether the operation is complete.
    pub completed: AtomicBool,
}

impl WatchdogState {
    pub fn new(total_bytes: u64, initial_queue_depth: u32) -> Self {
        Self {
            bytes_written: AtomicU64::new(0),
            bytes_total: AtomicU64::new(total_bytes),
            queue_depth: AtomicU64::new(initial_queue_depth as u64),
            use_sync_io: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            completed: AtomicBool::new(false),
        }
    }

    /// Update bytes written (called by I/O thread).
    pub fn update_progress(&self, bytes: u64) {
        self.bytes_written.store(bytes, Ordering::Relaxed);
    }

    /// Add bytes to the counter.
    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_written.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Get current bytes written.
    pub fn get_bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }

    /// Get current queue depth.
    pub fn get_queue_depth(&self) -> u32 {
        self.queue_depth.load(Ordering::Relaxed) as u32
    }

    /// Set queue depth (called by watchdog).
    pub fn set_queue_depth(&self, depth: u32) {
        self.queue_depth.store(depth as u64, Ordering::Relaxed);
    }

    /// Check if sync I/O should be used.
    pub fn should_use_sync(&self) -> bool {
        self.use_sync_io.load(Ordering::Relaxed)
    }

    /// Mark that sync I/O should be used.
    pub fn set_sync_io(&self) {
        self.use_sync_io.store(true, Ordering::Relaxed);
    }

    /// Cancel the operation.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// Check if cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Mark as completed.
    pub fn complete(&self) {
        self.completed.store(true, Ordering::Relaxed);
    }

    /// Check if completed.
    pub fn is_completed(&self) -> bool {
        self.completed.load(Ordering::Relaxed)
    }

    /// Progress percentage (0.0 to 100.0).
    pub fn progress_pct(&self) -> f64 {
        let total = self.bytes_total.load(Ordering::Relaxed);
        if total == 0 {
            return 0.0;
        }
        let written = self.bytes_written.load(Ordering::Relaxed);
        (written as f64 / total as f64) * 100.0
    }
}

/// The watchdog monitor — runs in a background task.
#[derive(Debug)]
pub struct WriteWatchdog {
    config: WatchdogConfig,
    state: Arc<WatchdogState>,
    stalls: Vec<StallEvent>,
    stall_count: u32,
    escalation_index: usize,
    operation_start: Instant,
}

impl WriteWatchdog {
    /// Create a new watchdog.
    pub fn new(config: WatchdogConfig, state: Arc<WatchdogState>) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            state,
            stalls: Vec::new(),
            stall_count: 0,
            escalation_index: 0,
            operation_start: Instant::now(),
        })
    }

    /// Run a single check cycle. Returns the recovery action if a stall is detected.
    pub fn check(&mut self, last_bytes: u64) -> Option<RecoveryAction> {
        if self.state.is_completed() || self.state.is_cancelled() {
            return None;
        }

        let current_bytes = self.state.get_bytes_written();

        if current_bytes > last_bytes {
            // Progress is being made, reset escalation.
            if self.escalation_index > 0 {
                debug!("Watchdog: progress resumed, resetting escalation");
                self.escalation_index = 0;
            }
            return None;
        }

        // No progress — this is a potential stall.
        self.stall_count += 1;
        let severity = self.classify_severity();

        warn!(
            "Watchdog: stall #{} detected ({} severity) at {:.1}% — no progress for check cycle",
            self.stall_count, severity, self.state.progress_pct()
        );

        // Determine action from escalation chain.
        let action = if self.escalation_index < self.config.escalation.len() {
            let a = self.config.escalation[self.escalation_index];
            self.escalation_index += 1;
            a
        } else {
            // Past the end of the chain — use the last action.
            *self.config.escalation.last().unwrap_or(&RecoveryAction::Cancel)
        };

        // Record the stall event.
        self.stalls.push(StallEvent {
            started_at_secs: self.operation_start.elapsed().as_secs_f64(),
            duration_secs: self.config.check_interval.as_secs_f64(),
            severity,
            action,
            queue_depth: self.state.get_queue_depth(),
            bytes_at_stall: current_bytes,
        });

        // Execute the action.
        self.execute_action(action);

        // Check max stalls.
        if self.config.max_stalls > 0 && self.stall_count >= self.config.max_stalls {
            warn!("Watchdog: max stall count ({}) reached — cancelling", self.config.max_stalls);
            self.state.cancel();
            return Some(RecoveryAction::Cancel);
        }

        Some(action)
    }

    /// Execute a recovery action.
    fn execute_action(&self, action: RecoveryAction) {
        match action {
            RecoveryAction::Warn => {
                warn!("Watchdog: write stall detected, continuing to monitor");
            }
            RecoveryAction::ReduceQueueDepth => {
                let current = self.state.get_queue_depth();
                let new_depth = (current as f64 * self.config.queue_depth_factor) as u32;
                let new_depth = new_depth.max(self.config.min_queue_depth);
                if new_depth < current {
                    info!("Watchdog: reducing queue depth {} → {}", current, new_depth);
                    self.state.set_queue_depth(new_depth);
                }
            }
            RecoveryAction::FallbackToSync => {
                warn!("Watchdog: falling back to synchronous I/O");
                self.state.set_sync_io();
                self.state.set_queue_depth(1);
            }
            RecoveryAction::Cancel => {
                error!("Watchdog: cancelling operation due to persistent stall");
                self.state.cancel();
            }
        }
    }

    /// Classify stall severity based on consecutive stall count.
    fn classify_severity(&self) -> StallSeverity {
        if self.stall_count <= 2 {
            StallSeverity::Minor
        } else if self.stall_count <= 5 {
            StallSeverity::Major
        } else {
            StallSeverity::Critical
        }
    }

    /// Get all recorded stall events.
    pub fn stalls(&self) -> &[StallEvent] {
        &self.stalls
    }

    /// Get total stall count.
    pub fn stall_count(&self) -> u32 {
        self.stall_count
    }

    /// Check if the operation was cancelled by the watchdog.
    pub fn was_cancelled(&self) -> bool {
        self.state.is_cancelled()
    }

    /// Get a summary of watchdog activity.
    pub fn summary(&self) -> WatchdogSummary {
        WatchdogSummary {
            stall_count: self.stall_count,
            stalls: self.stalls.clone(),
            was_cancelled: self.state.is_cancelled(),
            final_queue_depth: self.state.get_queue_depth(),
            used_sync_io: self.state.should_use_sync(),
            elapsed_secs: self.operation_start.elapsed().as_secs_f64(),
        }
    }
}

/// Summary of watchdog activity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchdogSummary {
    pub stall_count: u32,
    pub stalls: Vec<StallEvent>,
    pub was_cancelled: bool,
    pub final_queue_depth: u32,
    pub used_sync_io: bool,
    pub elapsed_secs: f64,
}

/// Format a watchdog summary as a human-readable string.
pub fn format_summary(summary: &WatchdogSummary) -> String {
    let mut lines = vec![];
    lines.push(format!("Watchdog Summary ({:.1}s elapsed)", summary.elapsed_secs));
    lines.push(format!("  Stalls detected: {}", summary.stall_count));
    lines.push(format!("  Final queue depth: {}", summary.final_queue_depth));
    lines.push(format!("  Used sync I/O: {}", summary.used_sync_io));
    lines.push(format!("  Was cancelled: {}", summary.was_cancelled));

    if !summary.stalls.is_empty() {
        lines.push("  Stall events:".into());
        for (i, stall) in summary.stalls.iter().enumerate() {
            lines.push(format!(
                "    #{}: {:.1}s, {} severity, action={}, qd={}",
                i + 1,
                stall.started_at_secs,
                stall.severity,
                stall.action,
                stall.queue_depth,
            ));
        }
    }

    lines.join("\n")
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state() -> Arc<WatchdogState> {
        Arc::new(WatchdogState::new(1_000_000, 8))
    }

    #[test]
    fn test_recovery_action_display() {
        assert_eq!(RecoveryAction::Warn.to_string(), "Warn");
        assert_eq!(RecoveryAction::ReduceQueueDepth.to_string(), "ReduceQueueDepth");
        assert_eq!(RecoveryAction::FallbackToSync.to_string(), "FallbackToSync");
        assert_eq!(RecoveryAction::Cancel.to_string(), "Cancel");
    }

    #[test]
    fn test_stall_severity_display() {
        assert_eq!(StallSeverity::Minor.to_string(), "Minor");
        assert_eq!(StallSeverity::Major.to_string(), "Major");
        assert_eq!(StallSeverity::Critical.to_string(), "Critical");
    }

    #[test]
    fn test_stall_severity_ordering() {
        assert!(StallSeverity::Minor < StallSeverity::Major);
        assert!(StallSeverity::Major < StallSeverity::Critical);
    }

    #[test]
    fn test_default_config() {
        let cfg = WatchdogConfig::default();
        assert_eq!(cfg.check_interval, Duration::from_secs(2));
        assert_eq!(cfg.stall_timeout, Duration::from_secs(10));
        assert_eq!(cfg.max_stalls, 10);
        assert!(cfg.enabled);
        assert!(!cfg.escalation.is_empty());
    }

    #[test]
    fn test_lenient_config() {
        let cfg = WatchdogConfig::lenient();
        assert_eq!(cfg.stall_timeout, Duration::from_secs(30));
        assert_eq!(cfg.max_stalls, 20);
    }

    #[test]
    fn test_strict_config() {
        let cfg = WatchdogConfig::strict();
        assert_eq!(cfg.stall_timeout, Duration::from_secs(5));
        assert_eq!(cfg.max_stalls, 5);
        assert_eq!(cfg.check_interval, Duration::from_secs(1));
    }

    #[test]
    fn test_config_validation_ok() {
        let cfg = WatchdogConfig::default();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_config_validation_short_interval() {
        let cfg = WatchdogConfig {
            check_interval: Duration::from_millis(50),
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validation_timeout_less_than_interval() {
        let cfg = WatchdogConfig {
            check_interval: Duration::from_secs(5),
            stall_timeout: Duration::from_secs(2),
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validation_empty_escalation() {
        let cfg = WatchdogConfig {
            escalation: vec![],
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_config_validation_bad_factor() {
        let cfg = WatchdogConfig {
            queue_depth_factor: 1.5,
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn test_watchdog_state_new() {
        let state = WatchdogState::new(1_000_000, 8);
        assert_eq!(state.get_bytes_written(), 0);
        assert_eq!(state.get_queue_depth(), 8);
        assert!(!state.should_use_sync());
        assert!(!state.is_cancelled());
        assert!(!state.is_completed());
    }

    #[test]
    fn test_watchdog_state_progress() {
        let state = WatchdogState::new(1_000_000, 8);
        state.update_progress(500_000);
        assert_eq!(state.get_bytes_written(), 500_000);
        assert!((state.progress_pct() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_watchdog_state_add_bytes() {
        let state = WatchdogState::new(1_000_000, 8);
        state.add_bytes(100);
        state.add_bytes(200);
        assert_eq!(state.get_bytes_written(), 300);
    }

    #[test]
    fn test_watchdog_state_queue_depth() {
        let state = WatchdogState::new(1_000_000, 8);
        state.set_queue_depth(4);
        assert_eq!(state.get_queue_depth(), 4);
    }

    #[test]
    fn test_watchdog_state_sync_io() {
        let state = WatchdogState::new(1_000_000, 8);
        assert!(!state.should_use_sync());
        state.set_sync_io();
        assert!(state.should_use_sync());
    }

    #[test]
    fn test_watchdog_state_cancel() {
        let state = WatchdogState::new(1_000_000, 8);
        assert!(!state.is_cancelled());
        state.cancel();
        assert!(state.is_cancelled());
    }

    #[test]
    fn test_watchdog_state_complete() {
        let state = WatchdogState::new(1_000_000, 8);
        assert!(!state.is_completed());
        state.complete();
        assert!(state.is_completed());
    }

    #[test]
    fn test_watchdog_state_progress_pct_zero_total() {
        let state = WatchdogState::new(0, 1);
        assert_eq!(state.progress_pct(), 0.0);
    }

    #[test]
    fn test_watchdog_no_stall_on_progress() {
        let state = test_state();
        let mut wd = WriteWatchdog::new(WatchdogConfig::default(), state.clone()).unwrap();
        state.update_progress(100);
        let action = wd.check(0); // last_bytes=0, current=100 → progress
        assert!(action.is_none());
    }

    #[test]
    fn test_watchdog_stall_detected() {
        let state = test_state();
        let mut wd = WriteWatchdog::new(WatchdogConfig::default(), state.clone()).unwrap();
        state.update_progress(100);
        let action = wd.check(100); // no progress since last check
        assert!(action.is_some());
        assert_eq!(action.unwrap(), RecoveryAction::Warn);
        assert_eq!(wd.stall_count(), 1);
    }

    #[test]
    fn test_watchdog_escalation() {
        let state = test_state();
        let mut wd = WriteWatchdog::new(WatchdogConfig::default(), state.clone()).unwrap();

        // First stall → Warn
        let a1 = wd.check(0);
        assert_eq!(a1, Some(RecoveryAction::Warn));

        // Second stall → ReduceQueueDepth
        let a2 = wd.check(0);
        assert_eq!(a2, Some(RecoveryAction::ReduceQueueDepth));

        // Third stall → ReduceQueueDepth again
        let a3 = wd.check(0);
        assert_eq!(a3, Some(RecoveryAction::ReduceQueueDepth));

        // Fourth stall → FallbackToSync
        let a4 = wd.check(0);
        assert_eq!(a4, Some(RecoveryAction::FallbackToSync));
        assert!(state.should_use_sync());
    }

    #[test]
    fn test_watchdog_queue_depth_reduction() {
        let state = test_state();
        let cfg = WatchdogConfig {
            escalation: vec![RecoveryAction::ReduceQueueDepth],
            ..Default::default()
        };
        let mut wd = WriteWatchdog::new(cfg, state.clone()).unwrap();

        // Initial queue depth = 8
        assert_eq!(state.get_queue_depth(), 8);
        wd.check(0); // stall → reduce queue depth
        assert_eq!(state.get_queue_depth(), 4); // 8 * 0.5 = 4
    }

    #[test]
    fn test_watchdog_completed_no_check() {
        let state = test_state();
        state.complete();
        let mut wd = WriteWatchdog::new(WatchdogConfig::default(), state.clone()).unwrap();
        let action = wd.check(0);
        assert!(action.is_none()); // completed, no stall
    }

    #[test]
    fn test_watchdog_cancelled_no_check() {
        let state = test_state();
        state.cancel();
        let mut wd = WriteWatchdog::new(WatchdogConfig::default(), state.clone()).unwrap();
        let action = wd.check(0);
        assert!(action.is_none());
    }

    #[test]
    fn test_watchdog_max_stalls() {
        let state = test_state();
        let cfg = WatchdogConfig {
            max_stalls: 3,
            escalation: vec![RecoveryAction::Warn],
            ..Default::default()
        };
        let mut wd = WriteWatchdog::new(cfg, state.clone()).unwrap();

        wd.check(0); // stall 1
        wd.check(0); // stall 2
        let action = wd.check(0); // stall 3 = max → Cancel
        assert_eq!(action, Some(RecoveryAction::Cancel));
        assert!(state.is_cancelled());
    }

    #[test]
    fn test_watchdog_escalation_reset_on_progress() {
        let state = test_state();
        let mut wd = WriteWatchdog::new(WatchdogConfig::default(), state.clone()).unwrap();

        // Stall → Warn
        wd.check(0);
        assert_eq!(wd.stall_count(), 1);

        // Progress → reset escalation
        state.update_progress(100);
        wd.check(0); // last_bytes=0, current=100 → progress
        // Next stall should restart from Warn (escalation_index reset)

        state.update_progress(100); // no more progress
        let a = wd.check(100);
        assert_eq!(a, Some(RecoveryAction::Warn)); // back to Warn
    }

    #[test]
    fn test_watchdog_summary() {
        let state = test_state();
        let mut wd = WriteWatchdog::new(WatchdogConfig::default(), state.clone()).unwrap();
        wd.check(0); // stall
        let summary = wd.summary();
        assert_eq!(summary.stall_count, 1);
        assert!(!summary.stalls.is_empty());
        assert!(!summary.was_cancelled);
        assert_eq!(summary.final_queue_depth, 8);
    }

    #[test]
    fn test_watchdog_stall_events() {
        let state = test_state();
        let mut wd = WriteWatchdog::new(WatchdogConfig::default(), state.clone()).unwrap();
        wd.check(0);
        let stalls = wd.stalls();
        assert_eq!(stalls.len(), 1);
        assert_eq!(stalls[0].severity, StallSeverity::Minor);
        assert_eq!(stalls[0].action, RecoveryAction::Warn);
        assert_eq!(stalls[0].queue_depth, 8);
    }

    #[test]
    fn test_format_summary() {
        let state = test_state();
        let mut wd = WriteWatchdog::new(WatchdogConfig::default(), state.clone()).unwrap();
        wd.check(0);
        let summary = wd.summary();
        let text = format_summary(&summary);
        assert!(text.contains("Watchdog Summary"));
        assert!(text.contains("Stalls detected: 1"));
    }

    #[test]
    fn test_stall_event_serialization() {
        let event = StallEvent {
            started_at_secs: 5.5,
            duration_secs: 2.0,
            severity: StallSeverity::Major,
            action: RecoveryAction::ReduceQueueDepth,
            queue_depth: 4,
            bytes_at_stall: 500_000,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("Major"));
        let back: StallEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.queue_depth, 4);
    }

    #[test]
    fn test_summary_serialization() {
        let summary = WatchdogSummary {
            stall_count: 2,
            stalls: vec![],
            was_cancelled: false,
            final_queue_depth: 4,
            used_sync_io: true,
            elapsed_secs: 10.5,
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"stall_count\":2"));
        assert!(json.contains("\"used_sync_io\":true"));
    }
}
