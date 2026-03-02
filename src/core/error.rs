#![allow(dead_code)]

use thiserror::Error;

/// Central error type for abt operations.
#[derive(Error, Debug)]
pub enum AbtError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Device not found: {0}")]
    DeviceNotFound(String),

    #[error("Device is a system drive and cannot be written to: {0}")]
    SystemDrive(String),

    #[error("Device is read-only: {0}")]
    ReadOnly(String),

    #[error("Image file not found: {0}")]
    ImageNotFound(String),

    #[error("Unsupported image format: {0}")]
    UnsupportedFormat(String),

    #[error("Image too large for device ({image_size} > {device_size})")]
    ImageTooLarge { image_size: u64, device_size: u64 },

    #[error("Verification failed at offset {offset}: expected {expected:#04x}, got {actual:#04x}")]
    VerificationFailed {
        offset: u64,
        expected: u8,
        actual: u8,
    },

    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("Write aborted by user")]
    Aborted,

    #[error("Permission denied: elevated privileges required")]
    PermissionDenied,

    #[error("Decompression error: {0}")]
    Decompression(String),

    #[error("Format error: {0}")]
    FormatError(String),

    #[error("Platform error: {0}")]
    PlatformError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    // ── New variants for robust error handling ──────────────────────────

    #[error("Operation timed out after {elapsed_secs:.1}s")]
    Timeout { elapsed_secs: f64 },

    #[error("Cancelled by user or agent")]
    CancelledByUser,

    #[error("Partition table backup failed: {0}")]
    BackupFailed(String),

    #[error("Device confirmation token mismatch: expected {expected}, got {actual}")]
    TokenMismatch { expected: String, actual: String },

    #[error("I/O write failed after {retries} retries: {msg}")]
    RetryExhausted { retries: u32, msg: String },

    #[error("Device changed between enumeration and write: {0}")]
    DeviceChanged(String),
}

pub type Result<T> = std::result::Result<T, AbtError>;
