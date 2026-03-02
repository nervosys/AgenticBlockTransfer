// Performance telemetry — runtime analytics and bottleneck detection.
//
// Inspired by rpi-imager's PerformanceStats system which tracks:
//   - Throughput per phase (download, decompress, write, verify)
//   - Bottleneck detection (network vs decompression vs storage vs verifying)
//   - Ring buffer starvation events
//   - Network retry counts
//   - Drain-and-hot-swap events
//   - Session-level statistics for telemetry upload
//
// This module provides generic performance tracking that can be attached
// to any long-running abt operation (write, clone, backup, restore).
// No data is sent externally unless explicitly configured.

#![allow(dead_code)]

use anyhow::Result;
use log::{debug, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{OnceLock, RwLock};

/// Which phase is currently the bottleneck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BottleneckState {
    /// No bottleneck detected (all phases balanced).
    None,
    /// Network download is the slowest phase.
    Network,
    /// Decompression can't keep up with download.
    Decompression,
    /// Storage write is the slowest phase (disk I/O bound).
    Storage,
    /// Verification read-back is the slowest phase.
    Verifying,
    /// CPU-bound hash computation.
    Hashing,
}

impl std::fmt::Display for BottleneckState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BottleneckState::None => write!(f, "None"),
            BottleneckState::Network => write!(f, "Network"),
            BottleneckState::Decompression => write!(f, "Decompression"),
            BottleneckState::Storage => write!(f, "Storage"),
            BottleneckState::Verifying => write!(f, "Verifying"),
            BottleneckState::Hashing => write!(f, "Hashing"),
        }
    }
}

/// A recorded performance event (counter or timing).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    /// Ring buffer ran out of data (writer waiting for producer).
    BufferStarvation,
    /// Ring buffer was full (producer waiting for writer).
    BufferFull,
    /// A network request had to be retried.
    NetworkRetry,
    /// Queue depth was reduced due to stall detection.
    QueueDepthReduction { from: u32, to: u32 },
    /// Switched from async to sync I/O as fallback.
    AsyncToSyncFallback,
    /// Drain-and-hot-swap: flushed pipeline to reset state.
    DrainAndHotSwap,
    /// Generic phase transition marker.
    PhaseTransition { from: String, to: String },
    /// Custom event with arbitrary label.
    Custom(String),
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventType::BufferStarvation => write!(f, "BufferStarvation"),
            EventType::BufferFull => write!(f, "BufferFull"),
            EventType::NetworkRetry => write!(f, "NetworkRetry"),
            EventType::QueueDepthReduction { from, to } => {
                write!(f, "QueueDepthReduction({} → {})", from, to)
            }
            EventType::AsyncToSyncFallback => write!(f, "AsyncToSyncFallback"),
            EventType::DrainAndHotSwap => write!(f, "DrainAndHotSwap"),
            EventType::PhaseTransition { from, to } => {
                write!(f, "PhaseTransition({} → {})", from, to)
            }
            EventType::Custom(label) => write!(f, "Custom({})", label),
        }
    }
}

/// A timestamped performance event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerfEvent {
    /// Seconds since session start.
    pub elapsed_secs: f64,
    /// The event type.
    pub event: EventType,
    /// Optional context/detail.
    pub detail: Option<String>,
}

/// Per-phase throughput measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseThroughput {
    /// Phase name (e.g., "download", "decompress", "write", "verify").
    pub phase: String,
    /// Total bytes processed in this phase.
    pub bytes_processed: u64,
    /// Total time spent in this phase.
    pub duration_secs: f64,
    /// Average throughput (bytes/sec).
    pub avg_bps: f64,
    /// Peak throughput sample (bytes/sec).
    pub peak_bps: f64,
    /// Minimum throughput sample (bytes/sec).
    pub min_bps: f64,
    /// Number of throughput samples.
    pub sample_count: u64,
}

impl PhaseThroughput {
    pub fn new(phase: &str) -> Self {
        Self {
            phase: phase.to_string(),
            bytes_processed: 0,
            duration_secs: 0.0,
            avg_bps: 0.0,
            peak_bps: 0.0,
            min_bps: f64::MAX,
            sample_count: 0,
        }
    }

    /// Record a throughput sample.
    pub fn record(&mut self, bytes: u64, elapsed_secs: f64) {
        self.bytes_processed += bytes;
        self.duration_secs += elapsed_secs;
        self.sample_count += 1;

        if elapsed_secs > 0.0 {
            let bps = bytes as f64 / elapsed_secs;
            if bps > self.peak_bps {
                self.peak_bps = bps;
            }
            if bps < self.min_bps {
                self.min_bps = bps;
            }
        }

        if self.duration_secs > 0.0 {
            self.avg_bps = self.bytes_processed as f64 / self.duration_secs;
        }
    }

    /// Get throughput as human-readable string.
    pub fn throughput_human(&self) -> String {
        format_throughput(self.avg_bps)
    }
}

/// Complete session telemetry report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryReport {
    /// Session identifier.
    pub session_id: String,
    /// abt version.
    pub abt_version: String,
    /// Platform (os/arch).
    pub platform: String,
    /// Operation type (write, clone, backup, etc.).
    pub operation: String,
    /// Start time (RFC 3339).
    pub start_time: String,
    /// Total duration in seconds.
    pub duration_secs: f64,
    /// Total bytes processed.
    pub total_bytes: u64,
    /// Overall throughput (bytes/sec).
    pub overall_bps: f64,
    /// Current (final) bottleneck state.
    pub bottleneck: BottleneckState,
    /// Bottleneck state history: how long each state was active.
    pub bottleneck_durations: HashMap<String, f64>,
    /// Per-phase throughput breakdown.
    pub phases: Vec<PhaseThroughput>,
    /// Recorded events.
    pub events: Vec<PerfEvent>,
    /// Success or failure.
    pub success: bool,
    /// Error message if failed.
    pub error: Option<String>,
}

/// Throughput sample for rolling average.
#[derive(Debug, Clone)]
struct ThroughputSample {
    bytes: u64,
    timestamp: Instant,
}

/// Main performance tracker — attach to any long-running operation.
#[derive(Debug)]
pub struct TelemetrySession {
    /// When the session started.
    start: Instant,
    /// Session ID.
    session_id: String,
    /// Operation name.
    operation: String,
    /// Per-phase throughput.
    phases: HashMap<String, PhaseThroughput>,
    /// Recorded events.
    events: Vec<PerfEvent>,
    /// Current bottleneck.
    bottleneck: BottleneckState,
    /// How long each bottleneck state has been active.
    bottleneck_durations: HashMap<BottleneckState, f64>,
    /// When the current bottleneck state started.
    bottleneck_since: Instant,
    /// Total bytes.
    total_bytes: u64,
    /// Rolling throughput window (last N samples).
    rolling_samples: Vec<ThroughputSample>,
    /// Maximum rolling window size.
    rolling_window_size: usize,
}

impl TelemetrySession {
    /// Create a new telemetry session.
    pub fn new(operation: &str) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            session_id: uuid::Uuid::new_v4().to_string(),
            operation: operation.to_string(),
            phases: HashMap::new(),
            events: Vec::new(),
            bottleneck: BottleneckState::None,
            bottleneck_durations: HashMap::new(),
            bottleneck_since: now,
            total_bytes: 0,
            rolling_samples: Vec::new(),
            rolling_window_size: 64,
        }
    }

    /// Record bytes processed in a phase.
    pub fn record_phase(&mut self, phase: &str, bytes: u64, elapsed_secs: f64) {
        let entry = self.phases
            .entry(phase.to_string())
            .or_insert_with(|| PhaseThroughput::new(phase));
        entry.record(bytes, elapsed_secs);
        self.total_bytes += bytes;

        self.rolling_samples.push(ThroughputSample {
            bytes,
            timestamp: Instant::now(),
        });
        if self.rolling_samples.len() > self.rolling_window_size {
            self.rolling_samples.remove(0);
        }
    }

    /// Record a performance event.
    pub fn record_event(&mut self, event: EventType, detail: Option<&str>) {
        let elapsed = self.start.elapsed().as_secs_f64();
        debug!("Telemetry event at {:.3}s: {}", elapsed, event);
        self.events.push(PerfEvent {
            elapsed_secs: elapsed,
            event,
            detail: detail.map(|s| s.to_string()),
        });
    }

    /// Update the current bottleneck state.
    pub fn set_bottleneck(&mut self, state: BottleneckState) {
        if state != self.bottleneck {
            // Record duration of previous state.
            let elapsed = self.bottleneck_since.elapsed().as_secs_f64();
            *self.bottleneck_durations.entry(self.bottleneck).or_insert(0.0) += elapsed;

            info!("Bottleneck changed: {} → {}", self.bottleneck, state);
            self.record_event(
                EventType::PhaseTransition {
                    from: self.bottleneck.to_string(),
                    to: state.to_string(),
                },
                None,
            );
            self.bottleneck = state;
            self.bottleneck_since = Instant::now();
        }
    }

    /// Detect bottleneck from phase throughputs.
    pub fn detect_bottleneck(&mut self) -> BottleneckState {
        let phases: Vec<_> = self.phases.values().collect();
        if phases.is_empty() {
            return BottleneckState::None;
        }

        // Find the phase with the lowest throughput (it's the bottleneck).
        let mut min_phase = "";
        let mut min_bps = f64::MAX;

        for p in &phases {
            if p.avg_bps > 0.0 && p.avg_bps < min_bps && p.sample_count > 2 {
                min_bps = p.avg_bps;
                min_phase = &p.phase;
            }
        }

        let state = match min_phase {
            "download" | "network" => BottleneckState::Network,
            "decompress" | "decompression" => BottleneckState::Decompression,
            "write" | "storage" => BottleneckState::Storage,
            "verify" | "verifying" | "verification" => BottleneckState::Verifying,
            "hash" | "hashing" | "checksum" => BottleneckState::Hashing,
            _ => BottleneckState::None,
        };

        self.set_bottleneck(state);
        state
    }

    /// Get current rolling throughput (bytes/sec).
    pub fn rolling_throughput(&self) -> f64 {
        if self.rolling_samples.len() < 2 {
            return 0.0;
        }
        let first = &self.rolling_samples[0];
        let last = &self.rolling_samples[self.rolling_samples.len() - 1];
        let elapsed = last.timestamp.duration_since(first.timestamp).as_secs_f64();
        if elapsed <= 0.0 {
            return 0.0;
        }
        let total_bytes: u64 = self.rolling_samples.iter().map(|s| s.bytes).sum();
        total_bytes as f64 / elapsed
    }

    /// Get overall throughput since session start.
    pub fn overall_throughput(&self) -> f64 {
        let elapsed = self.start.elapsed().as_secs_f64();
        if elapsed <= 0.0 {
            return 0.0;
        }
        self.total_bytes as f64 / elapsed
    }

    /// Get total elapsed time.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get current bottleneck state.
    pub fn bottleneck(&self) -> BottleneckState {
        self.bottleneck
    }

    /// Get phase throughput for a specific phase.
    pub fn phase(&self, name: &str) -> Option<&PhaseThroughput> {
        self.phases.get(name)
    }

    /// Number of events recorded.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }

    /// Generate the final telemetry report.
    pub fn finalize(&mut self, success: bool, error: Option<&str>) -> TelemetryReport {
        // Record final bottleneck duration.
        let elapsed = self.bottleneck_since.elapsed().as_secs_f64();
        *self.bottleneck_durations.entry(self.bottleneck).or_insert(0.0) += elapsed;

        let total_elapsed = self.start.elapsed().as_secs_f64();
        let overall_bps = if total_elapsed > 0.0 {
            self.total_bytes as f64 / total_elapsed
        } else {
            0.0
        };

        let bottleneck_durations: HashMap<String, f64> = self.bottleneck_durations
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect();

        let phases: Vec<PhaseThroughput> = self.phases.values().cloned().collect();

        TelemetryReport {
            session_id: self.session_id.clone(),
            abt_version: env!("CARGO_PKG_VERSION").to_string(),
            platform: format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH),
            operation: self.operation.clone(),
            start_time: chrono::Utc::now().to_rfc3339(),
            duration_secs: total_elapsed,
            total_bytes: self.total_bytes,
            overall_bps,
            bottleneck: self.bottleneck,
            bottleneck_durations,
            phases,
            events: self.events.clone(),
            success,
            error: error.map(|s| s.to_string()),
        }
    }
}

/// Thread-safe telemetry session wrapper.
#[derive(Debug, Clone)]
pub struct SharedTelemetry {
    inner: Arc<Mutex<TelemetrySession>>,
}

impl SharedTelemetry {
    pub fn new(operation: &str) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TelemetrySession::new(operation))),
        }
    }

    pub fn record_phase(&self, phase: &str, bytes: u64, elapsed_secs: f64) {
        if let Ok(mut session) = self.inner.lock() {
            session.record_phase(phase, bytes, elapsed_secs);
        }
    }

    pub fn record_event(&self, event: EventType, detail: Option<&str>) {
        if let Ok(mut session) = self.inner.lock() {
            session.record_event(event, detail);
        }
    }

    pub fn set_bottleneck(&self, state: BottleneckState) {
        if let Ok(mut session) = self.inner.lock() {
            session.set_bottleneck(state);
        }
    }

    pub fn detect_bottleneck(&self) -> BottleneckState {
        if let Ok(mut session) = self.inner.lock() {
            session.detect_bottleneck()
        } else {
            BottleneckState::None
        }
    }

    pub fn rolling_throughput(&self) -> f64 {
        if let Ok(session) = self.inner.lock() {
            session.rolling_throughput()
        } else {
            0.0
        }
    }

    pub fn finalize(&self, success: bool, error: Option<&str>) -> Option<TelemetryReport> {
        if let Ok(mut session) = self.inner.lock() {
            Some(session.finalize(success, error))
        } else {
            None
        }
    }
}

/// Format throughput as human-readable string.
pub fn format_throughput(bps: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    if bps >= GB {
        format!("{:.1} GiB/s", bps / GB)
    } else if bps >= MB {
        format!("{:.1} MiB/s", bps / MB)
    } else if bps >= KB {
        format!("{:.1} KiB/s", bps / KB)
    } else {
        format!("{:.0} B/s", bps)
    }
}

/// Export a telemetry report to a JSON file.
pub fn export_report(report: &TelemetryReport, path: &std::path::Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)?;
    info!("Telemetry report exported to {}", path.display());
    Ok(())
}

/// Load a telemetry report from a JSON file.
pub fn load_report(path: &std::path::Path) -> Result<TelemetryReport> {
    let content = std::fs::read_to_string(path)?;
    let report: TelemetryReport = serde_json::from_str(&content)?;
    Ok(report)
}

/// Summarize a telemetry report as a human-readable string.
pub fn summarize_report(report: &TelemetryReport) -> String {
    let mut lines = Vec::new();
    lines.push(format!("Session: {}", report.session_id));
    lines.push(format!("Operation: {}", report.operation));
    lines.push(format!("Platform: {}", report.platform));
    lines.push(format!("Duration: {:.2}s", report.duration_secs));
    lines.push(format!(
        "Total: {} ({}/s)",
        format_bytes_human(report.total_bytes),
        format_throughput(report.overall_bps)
    ));
    lines.push(format!("Bottleneck: {}", report.bottleneck));
    lines.push(format!("Result: {}", if report.success { "SUCCESS" } else { "FAILED" }));

    if let Some(ref err) = report.error {
        lines.push(format!("Error: {}", err));
    }

    if !report.phases.is_empty() {
        lines.push(String::new());
        lines.push("Phases:".into());
        for p in &report.phases {
            lines.push(format!(
                "  {}: {} processed, {} avg, {} peak",
                p.phase,
                format_bytes_human(p.bytes_processed),
                format_throughput(p.avg_bps),
                format_throughput(p.peak_bps),
            ));
        }
    }

    if !report.events.is_empty() {
        lines.push(String::new());
        lines.push(format!("Events: {} recorded", report.events.len()));
    }

    lines.join("\n")
}

fn format_bytes_human(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if bytes >= GIB {
        format!("{:.1} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────


// ──────────────────────────────────────────────
// Remote usage telemetry — opt-in AWS CloudWatch
// ──────────────────────────────────────────────
//
// Opt-in anonymous usage monitoring. Disabled by default.
// Enable with `ABT_TELEMETRY=1` or by calling `usage_enable()`.
// Events are buffered locally and flushed to AWS CloudWatch
// on a configurable interval (default 300 s). No PII is collected.
// All data is keyed by a random per-session UUID.

/// CloudWatch metric namespace.
const CLOUDWATCH_NAMESPACE: &str = "ABT/Usage";

/// AWS region for telemetry endpoint.
const CLOUDWATCH_REGION: &str = "us-east-1";

/// CloudWatch API endpoint.
const CLOUDWATCH_ENDPOINT: &str = "https://monitoring.us-east-1.amazonaws.com";

/// Maximum buffered events before oldest are dropped.
const MAX_BUFFER_SIZE: usize = 1000;

/// Background flush interval in seconds.
const FLUSH_INTERVAL_SECS: u64 = 300;

/// AWS CloudWatch service name for SigV4 signing.
const CLOUDWATCH_SERVICE: &str = "monitoring";

/// Opt-in flag (default: disabled).
static USAGE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Per-session random UUID.
static USAGE_SESSION_ID: OnceLock<String> = OnceLock::new();

/// Buffered usage events awaiting flush.
static USAGE_EVENTS: OnceLock<RwLock<Vec<UsageTelemetryEvent>>> = OnceLock::new();

/// A single usage telemetry event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageTelemetryEvent {
    /// ISO 8601 timestamp.
    pub timestamp: String,
    /// Event name (e.g., "command_invoked", "error").
    pub event_name: String,
    /// Dimension key-value pairs (e.g., command name, OS).
    pub dimensions: HashMap<String, String>,
    /// Metric value (default 1.0 for counters).
    pub value: f64,
}

fn events_store() -> &'static RwLock<Vec<UsageTelemetryEvent>> {
    USAGE_EVENTS.get_or_init(|| RwLock::new(Vec::new()))
}

fn session_id() -> &'static str {
    USAGE_SESSION_ID.get_or_init(|| uuid::Uuid::new_v4().to_string())
}

/// Check whether usage telemetry is enabled.
pub fn usage_is_enabled() -> bool {
    USAGE_ENABLED.load(Ordering::Relaxed)
}

/// Enable usage telemetry collection.
pub fn usage_enable() {
    USAGE_ENABLED.store(true, Ordering::Relaxed);
    info!("Usage telemetry enabled (session {})", session_id());
}

/// Disable usage telemetry collection.
pub fn usage_disable() {
    USAGE_ENABLED.store(false, Ordering::Relaxed);
    info!("Usage telemetry disabled");
}

/// Initialize telemetry from environment.
/// Call once at startup. Enables collection if `ABT_TELEMETRY=1`.
pub fn usage_init() {
    if std::env::var("ABT_TELEMETRY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        usage_enable();
    }
}

/// Human-readable status string.
pub fn usage_status() -> String {
    if usage_is_enabled() {
        format!(
            "Usage telemetry: ENABLED (session {}, {} buffered events)",
            session_id(),
            events_store().read().map(|v| v.len()).unwrap_or(0)
        )
    } else {
        "Usage telemetry: DISABLED (set ABT_TELEMETRY=1 to enable)".to_string()
    }
}

/// Record a generic usage event.
pub fn record_usage(event_name: &str, dimensions: HashMap<String, String>, value: f64) {
    if !usage_is_enabled() {
        return;
    }

    let event = UsageTelemetryEvent {
        timestamp: chrono::Utc::now().to_rfc3339(),
        event_name: event_name.to_string(),
        dimensions,
        value,
    };

    if let Ok(mut store) = events_store().write() {
        if store.len() >= MAX_BUFFER_SIZE {
            store.remove(0); // drop oldest
        }
        store.push(event);
    }
}

/// Record a CLI command invocation.
pub fn record_command(command: &str) {
    let mut dims = HashMap::new();
    dims.insert("command".to_string(), command.to_string());
    dims.insert("os".to_string(), std::env::consts::OS.to_string());
    dims.insert("arch".to_string(), std::env::consts::ARCH.to_string());
    dims.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());
    dims.insert("session_id".to_string(), session_id().to_string());
    record_usage("command_invoked", dims, 1.0);
}

/// Record an error event.
pub fn record_error(error_type: &str, message: &str) {
    let mut dims = HashMap::new();
    dims.insert("error_type".to_string(), error_type.to_string());
    dims.insert("message".to_string(), message.chars().take(256).collect());
    dims.insert("os".to_string(), std::env::consts::OS.to_string());
    dims.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());
    dims.insert("session_id".to_string(), session_id().to_string());
    record_usage("error", dims, 1.0);
}

/// Get a snapshot of buffered events.
pub fn usage_events() -> Vec<UsageTelemetryEvent> {
    events_store().read().map(|v| v.clone()).unwrap_or_default()
}

/// Clear buffered events.
pub fn reset_usage() {
    if let Ok(mut store) = events_store().write() {
        store.clear();
    }
}

/// Get buffered event count.
pub fn usage_event_count() -> usize {
    events_store().read().map(|v| v.len()).unwrap_or(0)
}

// ── AWS CloudWatch push ──────────────────────

/// Read AWS credentials from environment variables.
fn get_aws_credentials() -> Option<(String, String)> {
    let key = std::env::var("AWS_ACCESS_KEY_ID").ok()?;
    let secret = std::env::var("AWS_SECRET_ACCESS_KEY").ok()?;
    if key.is_empty() || secret.is_empty() {
        return None;
    }
    Some((key, secret))
}

/// HMAC-SHA256.
fn hmac_sha256(key: &[u8], msg: &[u8]) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC key length");
    mac.update(msg);
    mac.finalize().into_bytes().to_vec()
}

/// SHA-256 hex digest.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(data);
    hex::encode(hash)
}

/// Build CloudWatch PutMetricData query-string payload from buffered events.
fn build_cloudwatch_payload(events: &[UsageTelemetryEvent]) -> String {
    let mut params: Vec<String> = vec![
        "Action=PutMetricData".to_string(),
        format!("Namespace={}", CLOUDWATCH_NAMESPACE),
        "Version=2010-08-01".to_string(),
    ];

    for (i, event) in events.iter().enumerate() {
        let idx = i + 1;
        params.push(format!(
            "MetricData.member.{}.MetricName={}",
            idx, event.event_name
        ));
        params.push(format!("MetricData.member.{}.Value={}", idx, event.value));
        params.push(format!(
            "MetricData.member.{}.Timestamp={}",
            idx, event.timestamp
        ));
        params.push(format!("MetricData.member.{}.Unit=Count", idx));

        let mut dim_idx = 1;
        for (k, v) in &event.dimensions {
            params.push(format!(
                "MetricData.member.{}.Dimensions.member.{}.Name={}",
                idx, dim_idx, k
            ));
            params.push(format!(
                "MetricData.member.{}.Dimensions.member.{}.Value={}",
                idx, dim_idx, v
            ));
            dim_idx += 1;
        }
    }

    params.sort();
    params.join("&")
}

/// Sign and send metrics to AWS CloudWatch using SigV4.
async fn send_cloudwatch_metrics(events: &[UsageTelemetryEvent]) -> Result<()> {
    let (access_key, secret_key) = get_aws_credentials()
        .ok_or_else(|| anyhow::anyhow!("AWS credentials not set (AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY)"))?;

    let payload = build_cloudwatch_payload(events);
    let now = chrono::Utc::now();
    let date_stamp = now.format("%Y%m%d").to_string();
    let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
    let payload_hash = sha256_hex(payload.as_bytes());

    // Canonical request
    let canonical_headers = format!(
        "content-type:application/x-www-form-urlencoded\nhost:monitoring.{}.amazonaws.com\nx-amz-date:{}\n",
        CLOUDWATCH_REGION, amz_date
    );
    let signed_headers = "content-type;host;x-amz-date";
    let canonical_request = format!(
        "POST\n/\n\n{}{}\n{}",
        canonical_headers, signed_headers, payload_hash
    );

    // String to sign
    let credential_scope = format!("{}/{}/{}/aws4_request", date_stamp, CLOUDWATCH_REGION, CLOUDWATCH_SERVICE);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date, credential_scope, sha256_hex(canonical_request.as_bytes())
    );

    // Signing key
    let k_date = hmac_sha256(format!("AWS4{}", secret_key).as_bytes(), date_stamp.as_bytes());
    let k_region = hmac_sha256(&k_date, CLOUDWATCH_REGION.as_bytes());
    let k_service = hmac_sha256(&k_region, CLOUDWATCH_SERVICE.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        access_key, credential_scope, signed_headers, signature
    );

    let client = reqwest::Client::new();
    let resp = client
        .post(CLOUDWATCH_ENDPOINT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("X-Amz-Date", &amz_date)
        .header("Authorization", &authorization)
        .body(payload)
        .send()
        .await?;

    if resp.status().is_success() {
        info!("Flushed {} usage events to CloudWatch", events.len());
        Ok(())
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        warn!("CloudWatch push failed ({}): {}", status, body);
        Err(anyhow::anyhow!("CloudWatch push failed: {} {}", status, body))
    }
}

/// Flush buffered usage events to AWS CloudWatch.
///
/// Requires `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` environment variables.
/// Events are sent in batches of up to 20 (CloudWatch limit).
pub async fn flush_to_cloudwatch() -> Result<()> {
    if !usage_is_enabled() {
        return Ok(());
    }

    let events = {
        let mut store = events_store().write().map_err(|e| anyhow::anyhow!("lock: {}", e))?;
        std::mem::take(&mut *store)
    };

    if events.is_empty() {
        debug!("No usage events to flush");
        return Ok(());
    }

    info!("Flushing {} usage events to CloudWatch", events.len());

    // CloudWatch PutMetricData accepts up to 20 metrics per call
    for chunk in events.chunks(20) {
        if let Err(e) = send_cloudwatch_metrics(chunk).await {
            warn!("CloudWatch batch flush error: {}", e);
            // Re-buffer failed events
            if let Ok(mut store) = events_store().write() {
                for evt in chunk {
                    if store.len() < MAX_BUFFER_SIZE {
                        store.push(evt.clone());
                    }
                }
            }
            return Err(e);
        }
    }

    Ok(())
}

/// Start a background task that periodically flushes usage events.
///
/// Spawns a `tokio` task; call once at application startup.
pub fn start_background_flush() {
    if !usage_is_enabled() {
        return;
    }
    tokio::spawn(async {
        let interval = Duration::from_secs(FLUSH_INTERVAL_SECS);
        loop {
            tokio::time::sleep(interval).await;
            if !usage_is_enabled() {
                break;
            }
            if let Err(e) = flush_to_cloudwatch().await {
                warn!("Background telemetry flush error: {}", e);
            }
        }
    });
    info!(
        "Background usage telemetry flush started (every {}s)",
        FLUSH_INTERVAL_SECS
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bottleneck_state_display() {
        assert_eq!(BottleneckState::None.to_string(), "None");
        assert_eq!(BottleneckState::Network.to_string(), "Network");
        assert_eq!(BottleneckState::Decompression.to_string(), "Decompression");
        assert_eq!(BottleneckState::Storage.to_string(), "Storage");
        assert_eq!(BottleneckState::Verifying.to_string(), "Verifying");
        assert_eq!(BottleneckState::Hashing.to_string(), "Hashing");
    }

    #[test]
    fn test_event_type_display() {
        assert_eq!(EventType::BufferStarvation.to_string(), "BufferStarvation");
        assert_eq!(EventType::NetworkRetry.to_string(), "NetworkRetry");
        assert!(EventType::QueueDepthReduction { from: 8, to: 4 }
            .to_string()
            .contains("8"));
        assert!(EventType::Custom("test".into()).to_string().contains("test"));
    }

    #[test]
    fn test_phase_throughput_new() {
        let pt = PhaseThroughput::new("write");
        assert_eq!(pt.phase, "write");
        assert_eq!(pt.bytes_processed, 0);
        assert_eq!(pt.sample_count, 0);
        assert_eq!(pt.avg_bps, 0.0);
    }

    #[test]
    fn test_phase_throughput_record() {
        let mut pt = PhaseThroughput::new("write");
        pt.record(1_000_000, 1.0);
        assert_eq!(pt.bytes_processed, 1_000_000);
        assert_eq!(pt.sample_count, 1);
        assert!((pt.avg_bps - 1_000_000.0).abs() < 0.1);
        assert!((pt.peak_bps - 1_000_000.0).abs() < 0.1);
    }

    #[test]
    fn test_phase_throughput_multiple_records() {
        let mut pt = PhaseThroughput::new("download");
        pt.record(1_000_000, 1.0); // 1 MB/s
        pt.record(2_000_000, 1.0); // 2 MB/s
        assert_eq!(pt.bytes_processed, 3_000_000);
        assert_eq!(pt.sample_count, 2);
        assert!((pt.avg_bps - 1_500_000.0).abs() < 0.1);
        assert!((pt.peak_bps - 2_000_000.0).abs() < 0.1);
        assert!((pt.min_bps - 1_000_000.0).abs() < 0.1);
    }

    #[test]
    fn test_phase_throughput_human() {
        let mut pt = PhaseThroughput::new("write");
        pt.record(100 * 1024 * 1024, 1.0);
        let human = pt.throughput_human();
        assert!(human.contains("MiB/s"));
    }

    #[test]
    fn test_session_creation() {
        let session = TelemetrySession::new("write");
        assert_eq!(session.operation, "write");
        assert_eq!(session.bottleneck, BottleneckState::None);
        assert_eq!(session.total_bytes, 0);
        assert!(!session.session_id().is_empty());
    }

    #[test]
    fn test_session_record_phase() {
        let mut session = TelemetrySession::new("write");
        session.record_phase("write", 4_000_000, 0.1);
        assert_eq!(session.total_bytes, 4_000_000);
        let phase = session.phase("write").unwrap();
        assert_eq!(phase.bytes_processed, 4_000_000);
    }

    #[test]
    fn test_session_record_event() {
        let mut session = TelemetrySession::new("write");
        session.record_event(EventType::BufferStarvation, Some("ring buffer empty"));
        assert_eq!(session.event_count(), 1);
    }

    #[test]
    fn test_session_bottleneck_transition() {
        let mut session = TelemetrySession::new("write");
        assert_eq!(session.bottleneck(), BottleneckState::None);
        session.set_bottleneck(BottleneckState::Storage);
        assert_eq!(session.bottleneck(), BottleneckState::Storage);
        // Should have recorded a PhaseTransition event
        assert!(session.event_count() >= 1);
    }

    #[test]
    fn test_session_detect_bottleneck_empty() {
        let mut session = TelemetrySession::new("write");
        let state = session.detect_bottleneck();
        assert_eq!(state, BottleneckState::None);
    }

    #[test]
    fn test_session_detect_bottleneck_storage() {
        let mut session = TelemetrySession::new("write");
        // Simulate network being fast, storage being slow
        for _ in 0..5 {
            session.record_phase("download", 10_000_000, 0.1); // 100 MB/s
            session.record_phase("write", 1_000_000, 0.1); // 10 MB/s (slow)
        }
        let state = session.detect_bottleneck();
        assert_eq!(state, BottleneckState::Storage);
    }

    #[test]
    fn test_session_rolling_throughput_empty() {
        let session = TelemetrySession::new("write");
        assert_eq!(session.rolling_throughput(), 0.0);
    }

    #[test]
    fn test_session_overall_throughput() {
        let mut session = TelemetrySession::new("write");
        session.record_phase("write", 100_000_000, 1.0);
        let throughput = session.overall_throughput();
        assert!(throughput > 0.0);
    }

    #[test]
    fn test_session_finalize() {
        let mut session = TelemetrySession::new("clone");
        session.record_phase("write", 1_000_000, 0.5);
        session.record_event(EventType::NetworkRetry, None);
        let report = session.finalize(true, None);
        assert_eq!(report.operation, "clone");
        assert!(report.success);
        assert!(report.error.is_none());
        assert_eq!(report.total_bytes, 1_000_000);
        assert!(report.duration_secs >= 0.0);
        assert!(!report.session_id.is_empty());
        assert!(!report.platform.is_empty());
        assert_eq!(report.events.len(), 1);
    }

    #[test]
    fn test_session_finalize_failure() {
        let mut session = TelemetrySession::new("write");
        let report = session.finalize(false, Some("disk I/O error"));
        assert!(!report.success);
        assert_eq!(report.error.as_deref(), Some("disk I/O error"));
    }

    #[test]
    fn test_shared_telemetry() {
        let shared = SharedTelemetry::new("write");
        shared.record_phase("write", 1_000_000, 0.1);
        shared.record_event(EventType::BufferFull, None);
        shared.set_bottleneck(BottleneckState::Storage);
        let throughput = shared.rolling_throughput();
        assert!(throughput >= 0.0); // May be 0 with only 1 sample
    }

    #[test]
    fn test_shared_telemetry_finalize() {
        let shared = SharedTelemetry::new("backup");
        shared.record_phase("write", 2_000_000, 0.2);
        let report = shared.finalize(true, None).unwrap();
        assert_eq!(report.operation, "backup");
        assert!(report.success);
    }

    #[test]
    fn test_format_throughput() {
        assert_eq!(format_throughput(500.0), "500 B/s");
        assert!(format_throughput(1024.0).contains("KiB/s"));
        assert!(format_throughput(1024.0 * 1024.0).contains("MiB/s"));
        assert!(format_throughput(1024.0 * 1024.0 * 1024.0).contains("GiB/s"));
    }

    #[test]
    fn test_format_bytes_human() {
        assert_eq!(format_bytes_human(0), "0 B");
        assert_eq!(format_bytes_human(512), "512 B");
        assert!(format_bytes_human(1024 * 1024).contains("MiB"));
        assert!(format_bytes_human(1024 * 1024 * 1024).contains("GiB"));
    }

    #[test]
    fn test_report_serialization() {
        let mut session = TelemetrySession::new("write");
        session.record_phase("write", 1000, 0.01);
        let report = session.finalize(true, None);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"operation\":\"write\""));
        assert!(json.contains("\"success\":true"));
        // Can deserialize back
        let deserialized: TelemetryReport = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.operation, "write");
    }

    #[test]
    fn test_summarize_report() {
        let mut session = TelemetrySession::new("write");
        session.record_phase("write", 100_000_000, 2.0);
        let report = session.finalize(true, None);
        let summary = summarize_report(&report);
        assert!(summary.contains("write"));
        assert!(summary.contains("SUCCESS"));
        assert!(summary.contains("Session:"));
    }

    #[test]
    fn test_export_and_load_report() {
        let mut session = TelemetrySession::new("test");
        session.record_phase("write", 5000, 0.1);
        let report = session.finalize(true, None);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("telemetry.json");
        export_report(&report, &path).unwrap();

        let loaded = load_report(&path).unwrap();
        assert_eq!(loaded.session_id, report.session_id);
        assert_eq!(loaded.operation, "test");
    }

    #[test]
    fn test_perf_event_fields() {
        let event = PerfEvent {
            elapsed_secs: 1.5,
            event: EventType::NetworkRetry,
            detail: Some("timeout".into()),
        };
        assert_eq!(event.elapsed_secs, 1.5);
        assert_eq!(event.detail.as_deref(), Some("timeout"));
    }

    #[test]
    fn test_bottleneck_durations_tracked() {
        let mut session = TelemetrySession::new("write");
        session.set_bottleneck(BottleneckState::Network);
        std::thread::sleep(Duration::from_millis(10));
        session.set_bottleneck(BottleneckState::Storage);
        std::thread::sleep(Duration::from_millis(10));
        let report = session.finalize(true, None);
        // Should have durations for None, Network, and Storage
        assert!(report.bottleneck_durations.len() >= 2);
    }

    #[test]
    fn test_usage_telemetry_disabled_by_default() {
        assert!(!usage_is_enabled());
    }

    #[test]
    fn test_usage_enable_disable() {
        // Save original state
        let was_enabled = usage_is_enabled();
        usage_enable();
        assert!(usage_is_enabled());
        usage_disable();
        assert!(!usage_is_enabled());
        // Restore
        if was_enabled { usage_enable(); } else { usage_disable(); }
    }

    #[test]
    fn test_usage_status_disabled() {
        let was_enabled = usage_is_enabled();
        usage_disable();
        let status = usage_status();
        assert!(status.contains("DISABLED"));
        if was_enabled { usage_enable(); }
    }

    #[test]
    fn test_record_usage_when_disabled() {
        let was_enabled = usage_is_enabled();
        usage_disable();
        let before = usage_event_count();
        record_usage("disabled_test_event", HashMap::new(), 1.0);
        let after = usage_event_count();
        // When disabled, record_usage should be a no-op.
        assert_eq!(before, after, "event count should not change when telemetry is disabled");
        if was_enabled { usage_enable(); }
    }
    #[test]
    fn test_record_usage_when_enabled() {
        let was_enabled = usage_is_enabled();
        usage_enable();
        reset_usage();
        record_usage("test_event", HashMap::new(), 1.0);
        assert!(usage_event_count() >= 1);
        let events = usage_events();
        assert!(events.iter().any(|e| e.event_name == "test_event"));
        reset_usage();
        if !was_enabled { usage_disable(); }
    }

    #[test]
    fn test_record_command() {
        let was_enabled = usage_is_enabled();
        usage_enable();
        reset_usage();
        record_command("write");
        let events = usage_events();
        assert!(events.iter().any(|e| {
            e.event_name == "command_invoked"
                && e.dimensions.get("command").map(|s| s.as_str()) == Some("write")
        }));
        reset_usage();
        if !was_enabled { usage_disable(); }
    }

    #[test]
    fn test_record_error() {
        let was_enabled = usage_is_enabled();
        usage_enable();
        reset_usage();
        record_error("io", "disk full");
        let events = usage_events();
        assert!(events.iter().any(|e| {
            e.event_name == "error"
                && e.dimensions.get("error_type").map(|s| s.as_str()) == Some("io")
        }));
        reset_usage();
        if !was_enabled { usage_disable(); }
    }

    #[test]
    fn test_usage_session_id_stable() {
        let id1 = session_id();
        let id2 = session_id();
        assert_eq!(id1, id2);
        assert!(!id1.is_empty());
    }

    #[test]
    fn test_build_cloudwatch_payload() {
        let events = vec![UsageTelemetryEvent {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            event_name: "command_invoked".to_string(),
            dimensions: {
                let mut d = HashMap::new();
                d.insert("command".to_string(), "write".to_string());
                d
            },
            value: 1.0,
        }];
        let payload = build_cloudwatch_payload(&events);
        assert!(payload.contains("Action=PutMetricData"));
        assert!(payload.contains("Namespace=ABT/Usage"));
        assert!(payload.contains("MetricName=command_invoked"));
        assert!(payload.contains("Value=1"));
    }

    #[test]
    fn test_hmac_sha256_deterministic() {
        let result1 = hmac_sha256(b"key", b"message");
        let result2 = hmac_sha256(b"key", b"message");
        assert_eq!(result1, result2);
        assert!(!result1.is_empty());
    }

    #[test]
    fn test_sha256_hex_known_value() {
        // SHA-256 of empty string
        let hash = sha256_hex(b"");
        assert_eq!(hash, "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn test_reset_usage() {
        let was_enabled = usage_is_enabled();
        usage_enable();
        record_usage("test", HashMap::new(), 1.0);
        reset_usage();
        assert_eq!(usage_event_count(), 0);
        if !was_enabled { usage_disable(); }
    }

    #[test]
    fn test_usage_event_serialization() {
        let event = UsageTelemetryEvent {
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            event_name: "test".to_string(),
            dimensions: HashMap::new(),
            value: 42.0,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event_name\":\"test\""));
        let deserialized: UsageTelemetryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.event_name, "test");
        assert_eq!(deserialized.value, 42.0);
    }
}
