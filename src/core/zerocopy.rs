#![allow(dead_code)]
//! Zero-copy data transfer — splice(2) on Linux, sendfile(2) on macOS/FreeBSD.
//!
//! When copying data between two file descriptors (e.g., file → block device),
//! the kernel can transfer data directly in the page cache without copying it
//! through user-space buffers. This eliminates two memory copies per block and
//! reduces CPU overhead significantly for large transfers.
//!
//! Falls back to standard read/write on Windows and unsupported configurations.

use std::path::Path;

use anyhow::{Context, Result};
use log::warn;

use super::progress::Progress;

/// Result of a zero-copy transfer.
#[derive(Debug)]
pub struct ZeroCopyResult {
    /// Total bytes transferred.
    pub bytes_transferred: u64,
    /// Whether zero-copy was actually used (vs. fallback).
    pub used_zero_copy: bool,
    /// Kernel mechanism used.
    pub mechanism: &'static str,
}

/// Check if zero-copy transfer is available for the given source/target pair.
pub fn is_available(source: &Path, target: &Path) -> bool {
    // Zero-copy requires both source and target to be real files/devices
    // (not pipes, not stdio, not compressed streams)
    #[cfg(target_os = "linux")]
    {
        let _ = (source, target);
        true // splice(2) works for any fd pair on Linux via pipe intermediary
    }

    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    {
        // sendfile works for regular files
        source.exists() && target.exists()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
    {
        let _ = (source, target);
        false
    }
}

/// Transfer data from source to target using zero-copy if available.
///
/// On Linux: uses splice(2) via a pipe intermediary.
/// On macOS/FreeBSD: uses sendfile(2).
/// On Windows: falls back to standard read/write.
pub fn transfer(
    source: &Path,
    target: &Path,
    block_size: usize,
    byte_limit: Option<u64>,
    progress: &Progress,
) -> Result<ZeroCopyResult> {
    #[cfg(target_os = "linux")]
    {
        transfer_splice(source, target, block_size, byte_limit, progress)
    }

    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    {
        transfer_sendfile(source, target, byte_limit, progress)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "freebsd")))]
    {
        transfer_fallback(source, target, block_size, byte_limit, progress)
    }
}

/// Linux splice(2) implementation.
///
/// splice(2) moves data between two file descriptors where at least one
/// is a pipe. We create an internal pipe and use two splice calls:
///   1. splice(source_fd → pipe_write_end)
///   2. splice(pipe_read_end → target_fd)
///
/// This keeps data entirely in kernel space — never copied to user memory.
#[cfg(target_os = "linux")]
fn transfer_splice(
    source: &Path,
    target: &Path,
    block_size: usize,
    byte_limit: Option<u64>,
    progress: &Progress,
) -> Result<ZeroCopyResult> {
    use std::os::unix::io::AsRawFd;

    let source_file = std::fs::File::open(source)
        .with_context(|| format!("Failed to open source {}", source.display()))?;
    let target_file = std::fs::OpenOptions::new()
        .write(true)
        .open(target)
        .with_context(|| format!("Failed to open target {}", target.display()))?;

    // Create pipe for splice intermediary
    let (pipe_read, pipe_write) = nix::unistd::pipe()
        .context("Failed to create pipe for splice")?;

    let source_fd = source_file.as_raw_fd();
    let target_fd = target_file.as_raw_fd();

    let pipe_size = block_size.min(1024 * 1024) as usize; // Cap pipe at 1 MiB
    let max_bytes = byte_limit.unwrap_or(u64::MAX);
    let mut total: u64 = 0;

    info!("Zero-copy splice: {} → {}", source.display(), target.display());

    loop {
        if progress.is_cancelled() {
            break;
        }

        let remaining = max_bytes.saturating_sub(total);
        if remaining == 0 {
            break;
        }

        let chunk = pipe_size.min(remaining as usize);

        // splice: source → pipe
        let spliced_in = match nix::fcntl::splice(
            source_fd,
            None,
            pipe_write.as_raw_fd(),
            None,
            chunk,
            nix::fcntl::SpliceFFlags::SPLICE_F_MOVE | nix::fcntl::SpliceFFlags::SPLICE_F_MORE,
        ) {
            Ok(0) => break, // EOF
            Ok(n) => n,
            Err(nix::errno::Errno::EINVAL) | Err(nix::errno::Errno::ENOSYS) => {
                // splice not supported for this fd pair — fall back
                warn!("splice(2) not supported, falling back to standard I/O");
                drop(source_file);
                drop(target_file);
                let _ = nix::unistd::close(pipe_read.as_raw_fd());
                let _ = nix::unistd::close(pipe_write.as_raw_fd());
                return transfer_fallback(source, target, block_size, byte_limit, progress);
            }
            Err(e) => return Err(anyhow::anyhow!("splice(source→pipe) failed: {}", e)),
        };

        // splice: pipe → target
        let mut sent = 0;
        while sent < spliced_in {
            match nix::fcntl::splice(
                pipe_read.as_raw_fd(),
                None,
                target_fd,
                None,
                spliced_in - sent,
                nix::fcntl::SpliceFFlags::SPLICE_F_MOVE | nix::fcntl::SpliceFFlags::SPLICE_F_MORE,
            ) {
                Ok(n) => sent += n,
                Err(e) => return Err(anyhow::anyhow!("splice(pipe→target) failed: {}", e)),
            }
        }

        total += spliced_in as u64;
        progress.add_bytes(spliced_in as u64);
    }

    info!("splice transfer complete: {} bytes", total);

    Ok(ZeroCopyResult {
        bytes_transferred: total,
        used_zero_copy: true,
        mechanism: "splice(2)",
    })
}

/// macOS/FreeBSD sendfile(2) implementation.
#[cfg(any(target_os = "macos", target_os = "freebsd"))]
fn transfer_sendfile(
    source: &Path,
    target: &Path,
    byte_limit: Option<u64>,
    progress: &Progress,
) -> Result<ZeroCopyResult> {
    use std::os::unix::io::AsRawFd;

    let source_file = std::fs::File::open(source)
        .with_context(|| format!("Failed to open source {}", source.display()))?;
    let target_file = std::fs::OpenOptions::new()
        .write(true)
        .open(target)
        .with_context(|| format!("Failed to open target {}", target.display()))?;

    let source_size = source_file.metadata()?.len();
    let max_bytes = byte_limit.unwrap_or(source_size);

    let source_fd = source_file.as_raw_fd();
    let target_fd = target_file.as_raw_fd();

    let mut offset: i64 = 0;
    let mut total: u64 = 0;
    let chunk_size: usize = 4 * 1024 * 1024; // 4 MiB per sendfile call

    info!("Zero-copy sendfile: {} → {}", source.display(), target.display());

    loop {
        if progress.is_cancelled() {
            break;
        }

        let remaining = max_bytes.saturating_sub(total);
        if remaining == 0 {
            break;
        }

        let count = chunk_size.min(remaining as usize);

        #[cfg(target_os = "macos")]
        {
            let mut len = count as i64;
            let rc = unsafe {
                libc::sendfile(source_fd, target_fd, offset as libc::off_t, &mut len, std::ptr::null_mut(), 0)
            };
            if rc == -1 && len == 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::Other {
                    break; // EOF or unsupported
                }
                return Err(err.into());
            }
            let sent = len as u64;
            offset += sent as i64;
            total += sent;
            progress.add_bytes(sent);
            if sent == 0 {
                break;
            }
        }

        #[cfg(target_os = "freebsd")]
        {
            let mut sbytes: libc::off_t = 0;
            let rc = unsafe {
                libc::sendfile(source_fd, target_fd, offset as libc::off_t, count, std::ptr::null_mut(), &mut sbytes, 0)
            };
            if rc == -1 && sbytes == 0 {
                break;
            }
            let sent = sbytes as u64;
            offset += sent as i64;
            total += sent;
            progress.add_bytes(sent);
            if sent == 0 {
                break;
            }
        }
    }

    info!("sendfile transfer complete: {} bytes", total);

    Ok(ZeroCopyResult {
        bytes_transferred: total,
        used_zero_copy: true,
        mechanism: "sendfile(2)",
    })
}

/// Standard read/write fallback for platforms without zero-copy support.
fn transfer_fallback(
    source: &Path,
    target: &Path,
    block_size: usize,
    byte_limit: Option<u64>,
    progress: &Progress,
) -> Result<ZeroCopyResult> {
    use std::io::{BufReader, BufWriter, Read, Write};

    warn!("Using standard read/write fallback (no zero-copy)");

    let source_file = std::fs::File::open(source)
        .with_context(|| format!("Cannot open source {}", source.display()))?;
    let target_file = std::fs::OpenOptions::new()
        .write(true)
        .open(target)
        .with_context(|| format!("Cannot open target {}", target.display()))?;

    let mut reader = BufReader::with_capacity(block_size, source_file);
    let mut writer = BufWriter::with_capacity(block_size, target_file);
    let mut buf = vec![0u8; block_size];
    let max_bytes = byte_limit.unwrap_or(u64::MAX);
    let mut total: u64 = 0;

    loop {
        if progress.is_cancelled() {
            break;
        }

        let remaining = max_bytes.saturating_sub(total);
        if remaining == 0 {
            break;
        }

        let to_read = block_size.min(remaining as usize);
        let mut filled = 0;
        while filled < to_read {
            match reader.read(&mut buf[filled..to_read]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e.into()),
            }
        }

        if filled == 0 {
            break;
        }

        writer.write_all(&buf[..filled])?;
        total += filled as u64;
        progress.add_bytes(filled as u64);
    }

    writer.flush()?;

    Ok(ZeroCopyResult {
        bytes_transferred: total,
        used_zero_copy: false,
        mechanism: "read/write (fallback)",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_is_available() {
        let source = NamedTempFile::new().unwrap();
        let target = NamedTempFile::new().unwrap();
        // Should not panic
        let _ = is_available(source.path(), target.path());
    }

    #[test]
    fn test_transfer_basic() {
        let data: Vec<u8> = (0..16384).map(|i| (i % 256) as u8).collect();
        let mut source = NamedTempFile::new().unwrap();
        source.write_all(&data).unwrap();
        source.flush().unwrap();

        let target = NamedTempFile::new().unwrap();
        let progress = Progress::new(data.len() as u64);

        let result = transfer(source.path(), target.path(), 4096, None, &progress).unwrap();
        assert_eq!(result.bytes_transferred, 16384);

        let written = std::fs::read(target.path()).unwrap();
        assert_eq!(written, data);
    }

    #[test]
    fn test_transfer_with_limit() {
        let data: Vec<u8> = (0..8192).map(|i| (i % 256) as u8).collect();
        let mut source = NamedTempFile::new().unwrap();
        source.write_all(&data).unwrap();
        source.flush().unwrap();

        let target = NamedTempFile::new().unwrap();
        let progress = Progress::new(4096);

        let result =
            transfer(source.path(), target.path(), 1024, Some(4096), &progress).unwrap();
        assert_eq!(result.bytes_transferred, 4096);

        let written = std::fs::read(target.path()).unwrap();
        assert_eq!(written, &data[..4096]);
    }

    #[test]
    fn test_transfer_empty() {
        let source = NamedTempFile::new().unwrap();
        let target = NamedTempFile::new().unwrap();
        let progress = Progress::new(0);

        let result = transfer(source.path(), target.path(), 4096, None, &progress).unwrap();
        assert_eq!(result.bytes_transferred, 0);
    }

    #[test]
    fn test_transfer_cancel() {
        let data = vec![0u8; 65536];
        let mut source = NamedTempFile::new().unwrap();
        source.write_all(&data).unwrap();
        source.flush().unwrap();

        let target = NamedTempFile::new().unwrap();
        let progress = Progress::new(data.len() as u64);
        progress.cancel();

        let result = transfer(source.path(), target.path(), 4096, None, &progress).unwrap();
        assert_eq!(result.bytes_transferred, 0);
    }

    #[test]
    fn test_fallback_mechanism_name() {
        let data = vec![42u8; 1024];
        let mut source = NamedTempFile::new().unwrap();
        source.write_all(&data).unwrap();
        source.flush().unwrap();

        let target = NamedTempFile::new().unwrap();
        let progress = Progress::new(data.len() as u64);

        let result =
            transfer_fallback(source.path(), target.path(), 1024, None, &progress).unwrap();
        assert!(!result.used_zero_copy);
        assert_eq!(result.mechanism, "read/write (fallback)");
    }

    #[test]
    fn test_zero_copy_result_debug() {
        let result = ZeroCopyResult {
            bytes_transferred: 1024,
            used_zero_copy: false,
            mechanism: "test",
        };
        assert!(format!("{:?}", result).contains("1024"));
    }
}
