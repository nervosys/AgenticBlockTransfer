use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Real-time progress tracking for write/verify operations.
/// Fully lock-free — all fields use atomics for zero-contention reads
/// from the progress-reporter task.
#[derive(Debug, Clone)]
pub struct Progress {
    inner: Arc<ProgressInner>,
}

#[derive(Debug)]
struct ProgressInner {
    bytes_written: AtomicU64,
    bytes_total: AtomicU64,
    started: Instant,
    cancelled: AtomicBool,
    /// Phase stored as u8 discriminant — lock-free read/write.
    phase: AtomicU8,
}

/// Current operation phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum OperationPhase {
    Preparing = 0,
    Unmounting = 1,
    Decompressing = 2,
    Writing = 3,
    Syncing = 4,
    Verifying = 5,
    Formatting = 6,
    Finalizing = 7,
    Completed = 8,
    Failed = 9,
}

impl OperationPhase {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Preparing,
            1 => Self::Unmounting,
            2 => Self::Decompressing,
            3 => Self::Writing,
            4 => Self::Syncing,
            5 => Self::Verifying,
            6 => Self::Formatting,
            7 => Self::Finalizing,
            8 => Self::Completed,
            9 => Self::Failed,
            _ => Self::Preparing,
        }
    }
}

impl std::fmt::Display for OperationPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preparing => write!(f, "Preparing"),
            Self::Unmounting => write!(f, "Unmounting"),
            Self::Decompressing => write!(f, "Decompressing"),
            Self::Writing => write!(f, "Writing"),
            Self::Syncing => write!(f, "Syncing"),
            Self::Verifying => write!(f, "Verifying"),
            Self::Formatting => write!(f, "Formatting"),
            Self::Finalizing => write!(f, "Finalizing"),
            Self::Completed => write!(f, "Completed"),
            Self::Failed => write!(f, "Failed"),
        }
    }
}

/// Snapshot of progress state for serialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressSnapshot {
    pub phase: OperationPhase,
    pub bytes_written: u64,
    pub bytes_total: u64,
    pub percent: f64,
    pub elapsed_secs: f64,
    pub speed_bytes_per_sec: f64,
    pub eta_secs: Option<f64>,
}

impl Progress {
    pub fn new(total: u64) -> Self {
        Self {
            inner: Arc::new(ProgressInner {
                bytes_written: AtomicU64::new(0),
                bytes_total: AtomicU64::new(total),
                started: Instant::now(),
                cancelled: AtomicBool::new(false),
                phase: AtomicU8::new(OperationPhase::Preparing as u8),
            }),
        }
    }

    pub fn add_bytes(&self, n: u64) {
        self.inner.bytes_written.fetch_add(n, Ordering::Relaxed);
    }

    /// Set the bytes-written counter to an absolute value (for position-based tracking).
    pub fn set_bytes(&self, n: u64) {
        self.inner.bytes_written.store(n, Ordering::Relaxed);
    }

    /// Reset the bytes-written counter to zero (used for multi-pass operations).
    pub fn reset_bytes(&self) {
        self.inner.bytes_written.store(0, Ordering::Relaxed);
    }

    pub fn set_total(&self, total: u64) {
        self.inner.bytes_total.store(total, Ordering::Relaxed);
    }

    pub fn set_phase(&self, phase: OperationPhase) {
        self.inner.phase.store(phase as u8, Ordering::Release);
    }

    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    /// Take a consistent snapshot of all progress fields. Entirely lock-free.
    pub fn snapshot(&self) -> ProgressSnapshot {
        let written = self.inner.bytes_written.load(Ordering::Relaxed);
        let total = self.inner.bytes_total.load(Ordering::Relaxed);
        let elapsed = self.inner.started.elapsed().as_secs_f64();
        let speed = if elapsed > 0.0 {
            written as f64 / elapsed
        } else {
            0.0
        };
        let percent = if total > 0 {
            (written as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        let eta = if speed > 0.0 && total > written {
            Some((total - written) as f64 / speed)
        } else {
            None
        };

        ProgressSnapshot {
            phase: OperationPhase::from_u8(self.inner.phase.load(Ordering::Acquire)),
            bytes_written: written,
            bytes_total: total,
            percent,
            elapsed_secs: elapsed,
            speed_bytes_per_sec: speed,
            eta_secs: eta,
        }
    }
}
