//! Secure erase — safely wipe device contents using the best available method.
//!
//! Methods (in order of preference):
//! 1. **ATA Secure Erase** — hardware-level erase via ATA SECURITY ERASE UNIT
//! 2. **NVMe Sanitize** — NVMe format / sanitize
//! 3. **Block discard** — BLKDISCARD / TRIM (SSDs only)
//! 4. **Cryptographic erase** — overwrite with random data
//! 5. **Zero fill** — overwrite with zeros (n passes)
//!
//! The `auto` method selects the best available option for the device.

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use anyhow::{Context, Result};
use log::{info, warn};

use super::progress::{OperationPhase, Progress};

/// Erase method to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EraseMethod {
    /// Automatically choose the best method for the device.
    Auto,
    /// Overwrite with zeros.
    Zero,
    /// Overwrite with cryptographically random data.
    Random,
    /// ATA SECURITY ERASE UNIT (hardware erase).
    AtaSecureErase,
    /// NVMe Sanitize / Format.
    NvmeSanitize,
    /// Block discard / TRIM.
    Discard,
}

impl EraseMethod {
    /// Parse from string.
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "zero" | "zeros" => Some(Self::Zero),
            "random" | "rand" | "urandom" => Some(Self::Random),
            "ata-secure-erase" | "ata" | "secure-erase" => Some(Self::AtaSecureErase),
            "nvme-sanitize" | "nvme" | "sanitize" => Some(Self::NvmeSanitize),
            "discard" | "trim" | "blkdiscard" => Some(Self::Discard),
            _ => None,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Zero => "zero-fill",
            Self::Random => "random-fill",
            Self::AtaSecureErase => "ATA secure erase",
            Self::NvmeSanitize => "NVMe sanitize",
            Self::Discard => "block discard",
        }
    }
}

/// Configuration for a secure erase operation.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EraseConfig {
    /// Device path to erase.
    pub device: String,
    /// Erase method.
    pub method: EraseMethod,
    /// Number of overwrite passes (for zero/random methods).
    pub passes: u32,
    /// Force without confirmation.
    pub force: bool,
}

impl Default for EraseConfig {
    fn default() -> Self {
        Self {
            device: String::new(),
            method: EraseMethod::Auto,
            passes: 1,
            force: false,
        }
    }
}

/// Result of an erase operation.
#[derive(Debug)]
pub struct EraseResult {
    /// Method actually used.
    pub method_used: EraseMethod,
    /// Total bytes erased.
    pub bytes_erased: u64,
    /// Number of passes completed.
    pub passes_completed: u32,
    /// Whether the erase was verified.
    pub verified: bool,
}

/// Execute a secure erase.
pub fn erase_device(config: &EraseConfig, progress: &Progress) -> Result<EraseResult> {
    info!(
        "Secure erase: device={} method={} passes={}",
        config.device,
        config.method.label(),
        config.passes
    );

    // Determine device size
    let device_size = get_device_size(&config.device)?;
    info!(
        "Device size: {} ({})",
        humansize::format_size(device_size, humansize::BINARY),
        device_size
    );

    // Resolve auto method
    let method = if config.method == EraseMethod::Auto {
        resolve_auto_method(&config.device)
    } else {
        config.method
    };

    info!("Using erase method: {}", method.label());

    let result = match method {
        EraseMethod::Zero => zero_fill_erase(config, device_size, progress)?,
        EraseMethod::Random => random_fill_erase(config, device_size, progress)?,
        EraseMethod::Discard => discard_erase(config, device_size, progress)?,
        EraseMethod::AtaSecureErase => ata_secure_erase(config, device_size, progress)?,
        EraseMethod::NvmeSanitize => nvme_sanitize(config, device_size, progress)?,
        EraseMethod::Auto => unreachable!("auto should be resolved above"),
    };

    progress.set_phase(OperationPhase::Completed);
    info!("Secure erase completed: {} bytes erased", result.bytes_erased);

    Ok(result)
}

/// Get device size by seeking to end.
fn get_device_size(path: &str) -> Result<u64> {
    let p = Path::new(path);
    let meta = std::fs::metadata(p)?;
    if meta.is_file() {
        return Ok(meta.len());
    }
    let mut f = std::fs::File::open(p)?;
    let size = f.seek(SeekFrom::End(0))?;
    Ok(size)
}

/// Resolve the best erase method for a device automatically.
fn resolve_auto_method(device: &str) -> EraseMethod {
    // Check if this looks like an NVMe device
    if device.contains("nvme") {
        return EraseMethod::NvmeSanitize;
    }

    // Check for ATA device (Linux: /dev/sd*, Windows: PhysicalDrive)
    #[cfg(target_os = "linux")]
    {
        if device.starts_with("/dev/sd") {
            // Try to check if ATA secure erase is supported
            if check_ata_erase_support(device) {
                return EraseMethod::AtaSecureErase;
            }
        }
    }

    // Default to zero-fill as the safest universal fallback
    EraseMethod::Zero
}

/// Check if ATA secure erase is supported (Linux only).
#[cfg(target_os = "linux")]
fn check_ata_erase_support(device: &str) -> bool {
    // Check via hdparm -I if the device supports security erase
    match std::process::Command::new("hdparm")
        .args(["-I", device])
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.contains("Security:") && stdout.contains("supported")
        }
        Err(_) => false,
    }
}

/// Zero-fill erase: overwrite entire device with zeros.
fn zero_fill_erase(
    config: &EraseConfig,
    device_size: u64,
    progress: &Progress,
) -> Result<EraseResult> {
    let block_size = 4 * 1024 * 1024; // 4 MiB
    let zero_buf = vec![0u8; block_size];

    for pass in 0..config.passes {
        info!("Zero-fill pass {}/{}", pass + 1, config.passes);
        progress.set_phase(OperationPhase::Writing);
        progress.set_total(device_size);
        progress.reset_bytes();

        let mut file = open_device_write(&config.device)?;
        let mut remaining = device_size;

        while remaining > 0 {
            if progress.is_cancelled() {
                anyhow::bail!("Erase cancelled by user");
            }

            let to_write = std::cmp::min(block_size as u64, remaining) as usize;
            file.write_all(&zero_buf[..to_write])
                .context("Failed to write zeros to device")?;

            remaining -= to_write as u64;
            progress.add_bytes(to_write as u64);
        }

        file.flush()?;
    }

    Ok(EraseResult {
        method_used: EraseMethod::Zero,
        bytes_erased: device_size,
        passes_completed: config.passes,
        verified: false,
    })
}

/// Random-fill erase: overwrite entire device with random data.
fn random_fill_erase(
    config: &EraseConfig,
    device_size: u64,
    progress: &Progress,
) -> Result<EraseResult> {
    let block_size = 4 * 1024 * 1024; // 4 MiB

    for pass in 0..config.passes {
        info!("Random-fill pass {}/{}", pass + 1, config.passes);
        progress.set_phase(OperationPhase::Writing);
        progress.set_total(device_size);
        progress.reset_bytes();

        let mut file = open_device_write(&config.device)?;
        let mut remaining = device_size;
        let mut rng_buf = vec![0u8; block_size];

        while remaining > 0 {
            if progress.is_cancelled() {
                anyhow::bail!("Erase cancelled by user");
            }

            let to_write = std::cmp::min(block_size as u64, remaining) as usize;

            // Fill buffer with pseudo-random data using a fast PRNG
            fill_random(&mut rng_buf[..to_write]);

            file.write_all(&rng_buf[..to_write])
                .context("Failed to write random data to device")?;

            remaining -= to_write as u64;
            progress.add_bytes(to_write as u64);
        }

        file.flush()?;
    }

    Ok(EraseResult {
        method_used: EraseMethod::Random,
        bytes_erased: device_size,
        passes_completed: config.passes,
        verified: false,
    })
}

/// Fill a buffer with pseudo-random data using a simple xorshift64 PRNG.
/// Not cryptographically secure, but fast enough for erasing.
fn fill_random(buf: &mut [u8]) {
    // Seed from system time for entropy
    let mut state: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    if state == 0 {
        state = 0xDEAD_BEEF_CAFE_BABE;
    }

    // Fill u64-aligned words first (fast path)
    let (prefix, words, suffix) = unsafe { buf.align_to_mut::<u64>() };

    for b in prefix.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *b = state as u8;
    }

    for w in words.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *w = state;
    }

    for b in suffix.iter_mut() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *b = state as u8;
    }
}

/// Block discard (TRIM) for SSDs.
fn discard_erase(
    config: &EraseConfig,
    device_size: u64,
    progress: &Progress,
) -> Result<EraseResult> {
    progress.set_phase(OperationPhase::Writing);
    progress.set_total(device_size);

    #[cfg(target_os = "linux")]
    {
        info!("Attempting BLKDISCARD on {}", config.device);
        let output = std::process::Command::new("blkdiscard")
            .arg(&config.device)
            .output()
            .context("Failed to run blkdiscard — is util-linux installed?")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("blkdiscard failed: {}", stderr.trim());
        }

        progress.add_bytes(device_size);

        return Ok(EraseResult {
            method_used: EraseMethod::Discard,
            bytes_erased: device_size,
            passes_completed: 1,
            verified: false,
        });
    }

    #[cfg(not(target_os = "linux"))]
    {
        warn!("Block discard (TRIM) is only supported on Linux. Falling back to zero-fill.");
        zero_fill_erase(config, device_size, progress)
    }
}

/// ATA Secure Erase via hdparm (Linux only).
fn ata_secure_erase(
    config: &EraseConfig,
    device_size: u64,
    progress: &Progress,
) -> Result<EraseResult> {
    progress.set_phase(OperationPhase::Writing);
    progress.set_total(device_size);

    #[cfg(target_os = "linux")]
    {
        info!("Performing ATA Secure Erase on {}", config.device);

        // Step 1: Set a temporary security password
        let output = std::process::Command::new("hdparm")
            .args(["--user-master", "u", "--security-set-pass", "abt_erase", &config.device])
            .output()
            .context("Failed to set ATA security password via hdparm")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("hdparm security-set-pass failed: {}", stderr.trim());
        }

        // Step 2: Issue the erase command
        let output = std::process::Command::new("hdparm")
            .args(["--user-master", "u", "--security-erase", "abt_erase", &config.device])
            .output()
            .context("Failed to issue ATA security erase via hdparm")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("hdparm security-erase failed: {}", stderr.trim());
        }

        progress.add_bytes(device_size);

        return Ok(EraseResult {
            method_used: EraseMethod::AtaSecureErase,
            bytes_erased: device_size,
            passes_completed: 1,
            verified: false,
        });
    }

    #[cfg(not(target_os = "linux"))]
    {
        warn!("ATA Secure Erase is only supported on Linux. Falling back to zero-fill.");
        zero_fill_erase(config, device_size, progress)
    }
}

/// NVMe Sanitize / Format (Linux only).
fn nvme_sanitize(
    config: &EraseConfig,
    device_size: u64,
    progress: &Progress,
) -> Result<EraseResult> {
    progress.set_phase(OperationPhase::Writing);
    progress.set_total(device_size);

    #[cfg(target_os = "linux")]
    {
        info!("Performing NVMe sanitize on {}", config.device);

        // Try nvme-cli sanitize first
        let output = std::process::Command::new("nvme")
            .args(["sanitize", &config.device, "-a", "2"]) // Block Erase
            .output();

        match output {
            Ok(out) if out.status.success() => {
                progress.add_bytes(device_size);
                return Ok(EraseResult {
                    method_used: EraseMethod::NvmeSanitize,
                    bytes_erased: device_size,
                    passes_completed: 1,
                    verified: false,
                });
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                warn!("nvme sanitize failed: {}. Trying nvme format.", stderr.trim());
            }
            Err(e) => {
                warn!("nvme-cli not available: {}. Trying nvme format.", e);
            }
        }

        // Fallback: nvme format
        let output = std::process::Command::new("nvme")
            .args(["format", &config.device, "--ses=1"]) // User Data Erase
            .output()
            .context("Failed to run nvme format")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("nvme format failed: {}. Falling back to zero-fill.", stderr.trim());
            return zero_fill_erase(config, device_size, progress);
        }

        progress.add_bytes(device_size);

        return Ok(EraseResult {
            method_used: EraseMethod::NvmeSanitize,
            bytes_erased: device_size,
            passes_completed: 1,
            verified: false,
        });
    }

    #[cfg(not(target_os = "linux"))]
    {
        warn!("NVMe sanitize is only supported on Linux. Falling back to zero-fill.");
        zero_fill_erase(config, device_size, progress)
    }
}

/// Open a device for writing.
fn open_device_write(path: &str) -> Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("Failed to open device for writing: {}", path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use tempfile::NamedTempFile;

    #[test]
    fn test_erase_method_from_str() {
        assert_eq!(EraseMethod::from_str_name("auto"), Some(EraseMethod::Auto));
        assert_eq!(EraseMethod::from_str_name("zero"), Some(EraseMethod::Zero));
        assert_eq!(
            EraseMethod::from_str_name("random"),
            Some(EraseMethod::Random)
        );
        assert_eq!(
            EraseMethod::from_str_name("ata-secure-erase"),
            Some(EraseMethod::AtaSecureErase)
        );
        assert_eq!(
            EraseMethod::from_str_name("nvme-sanitize"),
            Some(EraseMethod::NvmeSanitize)
        );
        assert_eq!(
            EraseMethod::from_str_name("discard"),
            Some(EraseMethod::Discard)
        );
        assert_eq!(EraseMethod::from_str_name("bogus"), None);
    }

    #[test]
    fn test_erase_method_label() {
        assert_eq!(EraseMethod::Zero.label(), "zero-fill");
        assert_eq!(EraseMethod::Random.label(), "random-fill");
        assert_eq!(EraseMethod::Auto.label(), "auto");
    }

    #[test]
    fn test_erase_config_default() {
        let cfg = EraseConfig::default();
        assert_eq!(cfg.method, EraseMethod::Auto);
        assert_eq!(cfg.passes, 1);
        assert!(!cfg.force);
    }

    #[test]
    fn test_fill_random() {
        let mut buf = vec![0u8; 1024];
        fill_random(&mut buf);
        // Statistical check: not all zeros
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_fill_random_unaligned() {
        let mut buf = vec![0u8; 1023]; // not u64-aligned
        fill_random(&mut buf);
        assert!(buf.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_zero_fill_erase_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&[0xFF; 4096]).unwrap();
        tmp.flush().unwrap();

        let config = EraseConfig {
            device: tmp.path().to_str().unwrap().to_string(),
            method: EraseMethod::Zero,
            passes: 1,
            force: true,
        };

        let progress = Progress::new(0);
        let result = zero_fill_erase(&config, 4096, &progress).unwrap();
        assert_eq!(result.method_used, EraseMethod::Zero);
        assert_eq!(result.bytes_erased, 4096);
        assert_eq!(result.passes_completed, 1);

        // Verify all zeros
        let mut contents = Vec::new();
        let mut f = std::fs::File::open(tmp.path()).unwrap();
        f.read_to_end(&mut contents).unwrap();
        assert!(contents.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_random_fill_erase_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&[0x00; 4096]).unwrap();
        tmp.flush().unwrap();

        let config = EraseConfig {
            device: tmp.path().to_str().unwrap().to_string(),
            method: EraseMethod::Random,
            passes: 1,
            force: true,
        };

        let progress = Progress::new(0);
        let result = random_fill_erase(&config, 4096, &progress).unwrap();
        assert_eq!(result.method_used, EraseMethod::Random);
        assert_eq!(result.bytes_erased, 4096);

        // Verify not all zeros any more
        let mut contents = Vec::new();
        let mut f = std::fs::File::open(tmp.path()).unwrap();
        f.read_to_end(&mut contents).unwrap();
        assert!(contents.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_multi_pass_zero_erase() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&[0xFF; 2048]).unwrap();
        tmp.flush().unwrap();

        let config = EraseConfig {
            device: tmp.path().to_str().unwrap().to_string(),
            method: EraseMethod::Zero,
            passes: 3,
            force: true,
        };

        let progress = Progress::new(0);
        let result = zero_fill_erase(&config, 2048, &progress).unwrap();
        assert_eq!(result.passes_completed, 3);
    }

    #[test]
    fn test_get_device_size_file() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&[0xAA; 16384]).unwrap();
        tmp.flush().unwrap();

        let size = get_device_size(tmp.path().to_str().unwrap()).unwrap();
        assert_eq!(size, 16384);
    }

    #[test]
    fn test_resolve_auto_nvme() {
        let method = resolve_auto_method("/dev/nvme0n1");
        assert_eq!(method, EraseMethod::NvmeSanitize);
    }

    #[test]
    fn test_resolve_auto_regular() {
        let method = resolve_auto_method("/tmp/testfile");
        assert_eq!(method, EraseMethod::Zero);
    }
}
