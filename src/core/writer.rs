use anyhow::Result;
use log::{info, warn};
use sha2::{digest, Digest};
use std::io::{BufReader, BufWriter, Read, Seek, Write};
use std::time::Duration;

use super::download;
use super::image;
use super::progress::{OperationPhase, Progress};
use super::types::{HashAlgorithm, WriteConfig};

/// Maximum number of retries for transient I/O errors per block.
const MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff on retry.
const RETRY_BASE_DELAY: Duration = Duration::from_millis(100);

/// The main writer engine. Handles reading from source, optional decompression,
/// writing to target device, hashing during write to avoid double-decompression
/// on verify, and optional verification with retry logic.
pub struct Writer {
    config: WriteConfig,
    progress: Progress,
}

/// Result of a write operation, including an optional hash computed during write.
struct WriteResult {
    bytes_written: u64,
    /// Number of bytes skipped via sparse optimization (zero blocks seeked past).
    bytes_sparse_skipped: u64,
    /// Hash of the data as it was written (computed inline to avoid re-reading
    /// and re-decompressing the source during verification).
    write_hash: Option<String>,
}

impl Writer {
    pub fn new(config: WriteConfig) -> Self {
        Self {
            config,
            progress: Progress::new(0),
        }
    }

    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    /// Execute the write operation.
    pub async fn execute(&self) -> Result<()> {
        info!("Starting write operation");
        info!("  Source: {}", self.config.source);
        info!("  Target: {}", self.config.target);
        info!("  Block size: {} bytes", self.config.block_size);
        info!("  Verify: {}", self.config.verify);
        info!("  Direct I/O: {}", self.config.direct_io);

        // Phase 1: Prepare source
        self.progress.set_phase(OperationPhase::Preparing);
        let (source_path, _is_temp_download) = match &self.config.source {
            super::types::ImageSource::File(p) => (p.clone(), false),
            super::types::ImageSource::Url(url) => {
                info!("Downloading image from URL...");
                let path = download::download_streaming(url, &self.progress).await?;
                (path, true)
            }
            super::types::ImageSource::Stdin => {
                anyhow::bail!("Stdin source not yet implemented");
            }
        };

        let source_meta = std::fs::metadata(&source_path)?;
        self.progress.set_total(source_meta.len());

        // Phase 2: Unmount target
        self.progress.set_phase(OperationPhase::Unmounting);
        let enumerator = super::device::create_enumerator();
        if let Err(e) = enumerator.unmount_device(&self.config.target).await {
            warn!("Could not unmount target: {}", e);
        }

        // Phase 3: Open source with optional decompression + BufReader wrapping
        let format = image::detect_format(&source_path)?;
        let reader: Box<dyn Read + Send> = if format.is_compressed() {
            self.progress.set_phase(OperationPhase::Decompressing);
            info!("Decompressing {} image", format);
            image::open_image(&source_path)?
        } else {
            Box::new(BufReader::with_capacity(
                self.config.block_size,
                std::fs::File::open(&source_path)?,
            ))
        };

        // Determine hash algorithm for inline hashing during write
        let inline_hash_algo = if self.config.verify {
            self.config.hash_algorithm
        } else {
            None
        };

        // Phase 4: Write (blocking I/O dispatched off the async runtime)
        self.progress.set_phase(OperationPhase::Writing);
        let block_size = self.config.block_size;
        let target = self.config.target.clone();
        let sync = self.config.sync;
        let direct_io = self.config.direct_io;
        let sparse = self.config.sparse;
        let progress = self.progress.clone();

        let write_result = tokio::task::spawn_blocking(move || {
            Self::write_blocks(
                reader,
                &target,
                block_size,
                sync,
                direct_io,
                sparse,
                inline_hash_algo,
                &progress,
            )
        })
        .await??;

        info!("Wrote {} bytes", write_result.bytes_written);
        if write_result.bytes_sparse_skipped > 0 {
            info!(
                "Sparse optimization: skipped {} zero-fill bytes ({:.1}% of total)",
                write_result.bytes_sparse_skipped,
                (write_result.bytes_sparse_skipped as f64
                    / (write_result.bytes_written + write_result.bytes_sparse_skipped) as f64)
                    * 100.0
            );
        }

        // Phase 5: Sync — ensure data reaches physical media
        self.progress.set_phase(OperationPhase::Syncing);
        sync_device(&self.config.target)?;

        // Phase 6: Verify — compare hash computed during write with hash of target
        if self.config.verify {
            self.progress.set_phase(OperationPhase::Verifying);
            info!("Verifying write...");

            let target = self.config.target.clone();
            let bytes_written = write_result.bytes_written;
            let write_hash = write_result.write_hash.clone();
            let hash_algo = self.config
                .hash_algorithm
                .unwrap_or(HashAlgorithm::Blake3);
            let progress = self.progress.clone();

            tokio::task::spawn_blocking(move || {
                Self::verify_write(
                    &target,
                    bytes_written,
                    write_hash.as_deref(),
                    hash_algo,
                    &progress,
                )
            })
            .await??;

            info!("Verification successful");
        }

        self.progress.set_phase(OperationPhase::Completed);
        info!("Write operation completed successfully");

        // Clean up temp download if applicable
        if _is_temp_download {
            download::cleanup_download(&source_path);
        }

        Ok(())
    }

    /// Write blocks from reader to target device with inline hashing, sparse
    /// optimization, and retry logic.
    ///
    /// When `sparse` is true, all-zero blocks are seeked past instead of
    /// written. This is equivalent to `dd conv=sparse` and can dramatically
    /// speed up images with large empty regions (e.g. freshly partitioned
    /// disk images where only the first and last few MB are non-zero).
    fn write_blocks(
        mut reader: Box<dyn Read + Send>,
        target: &str,
        block_size: usize,
        sync: bool,
        direct_io: bool,
        sparse: bool,
        hash_algorithm: Option<HashAlgorithm>,
        progress: &Progress,
    ) -> Result<WriteResult> {
        let raw_target = open_device_for_writing(target, direct_io)?;
        let mut target_file = BufWriter::with_capacity(block_size, raw_target);
        let mut buf = vec![0u8; block_size];
        let mut total_written: u64 = 0;
        let mut sparse_skipped: u64 = 0;

        // Inline hasher — compute hash of data as we write it so we never have
        // to re-read / re-decompress the source for verification.
        let mut inline_hasher: Option<Box<dyn InlineHasher>> = hash_algorithm.map(|algo| {
            create_inline_hasher(algo)
        });

        loop {
            if progress.is_cancelled() {
                anyhow::bail!("Write cancelled by user");
            }

            // Read a full block (handle partial reads)
            let block_filled = read_full_block(&mut reader, &mut buf)?;
            if block_filled == 0 {
                break;
            }

            // Feed data to inline hasher before writing (includes zeros for
            // correct hash computation even when sparse-skipped)
            if let Some(ref mut h) = inline_hasher {
                h.update(&buf[..block_filled]);
            }

            // Sparse optimization: skip all-zero blocks by seeking
            if sparse && is_block_zero(&buf[..block_filled]) {
                // Seek forward by block_filled bytes instead of writing zeros
                target_file.seek(std::io::SeekFrom::Current(block_filled as i64))?;
                sparse_skipped += block_filled as u64;
                progress.add_bytes(block_filled as u64);
                continue;
            }

            // Write with retry logic for transient I/O errors
            write_with_retry(&mut target_file, &buf[..block_filled], MAX_RETRIES)?;
            total_written += block_filled as u64;
            progress.add_bytes(block_filled as u64);
        }

        if sync {
            target_file.flush()?;
        }

        let write_hash = inline_hasher.map(|h| h.finalize_hex());

        Ok(WriteResult {
            bytes_written: total_written,
            bytes_sparse_skipped: sparse_skipped,
            write_hash,
        })
    }

    /// Verify written data by hashing the target device and comparing with the
    /// hash computed during write. This avoids re-reading and re-decompressing
    /// the source entirely.
    fn verify_write(
        target: &str,
        size: u64,
        expected_hash: Option<&str>,
        algorithm: HashAlgorithm,
        progress: &Progress,
    ) -> Result<()> {
        let expected = match expected_hash {
            Some(h) => h,
            None => {
                warn!("No write-hash available; skipping hash-based verification");
                return Ok(());
            }
        };

        progress.set_total(size);

        let target_file = std::fs::File::open(target)?;
        let limited = target_file.take(size);
        let mut reader = BufReader::with_capacity(4 * 1024 * 1024, limited);

        let mut hasher = create_inline_hasher(algorithm);
        let mut buf = vec![0u8; 4 * 1024 * 1024];
        let mut offset: u64 = 0;

        while offset < size {
            if progress.is_cancelled() {
                anyhow::bail!("Verification cancelled by user");
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            offset += n as u64;
            progress.add_bytes(n as u64);
        }

        let actual = hasher.finalize_hex();

        if actual != expected {
            return Err(super::error::AbtError::ChecksumMismatch {
                expected: expected.to_string(),
                actual,
            }
            .into());
        }

        Ok(())
    }
}

// ── Helper functions ───────────────────────────────────────────────────────────

/// Check if a buffer is entirely zero. Uses u64-width comparison for speed
/// (processes 8 bytes per iteration instead of 1).
fn is_block_zero(buf: &[u8]) -> bool {
    // Process in u64 chunks for ~8x throughput on aligned data
    let (prefix, aligned, suffix) = unsafe { buf.align_to::<u64>() };
    prefix.iter().all(|&b| b == 0)
        && aligned.iter().all(|&w| w == 0)
        && suffix.iter().all(|&b| b == 0)
}

/// Read a full block from the reader, handling partial reads and EINTR.
fn read_full_block(reader: &mut dyn Read, buf: &mut [u8]) -> Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(filled)
}

/// Write data with exponential-backoff retry for transient I/O errors.
fn write_with_retry(writer: &mut dyn Write, data: &[u8], max_retries: u32) -> Result<()> {
    let mut attempt = 0;
    loop {
        match writer.write_all(data) {
            Ok(()) => return Ok(()),
            Err(e) => {
                // Only retry on transient errors
                let retryable = matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                );
                if retryable && attempt < max_retries {
                    attempt += 1;
                    let delay = RETRY_BASE_DELAY * 2u32.pow(attempt - 1);
                    warn!(
                        "Transient I/O error (attempt {}/{}): {}. Retrying in {:?}...",
                        attempt, max_retries, e, delay
                    );
                    std::thread::sleep(delay);
                } else {
                    return Err(e.into());
                }
            }
        }
    }
}

/// Open a device/file for writing (platform-specific).
pub(crate) fn open_device_for_writing(path: &str, direct_io: bool) -> Result<Box<dyn WriteSeek + Send>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut flags = libc::O_SYNC;
        if direct_io {
            flags |= libc::O_DIRECT;
        }
        let file = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(flags)
            .open(path)?;
        Ok(Box::new(file))
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let mut flags = 0u32;
        // FILE_FLAG_WRITE_THROUGH = 0x80000000 — bypass write cache
        flags |= 0x80000000;
        if direct_io {
            // FILE_FLAG_NO_BUFFERING = 0x20000000 — unbuffered I/O
            flags |= 0x20000000;
        }
        let file = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(flags)
            .open(path)?;
        Ok(Box::new(file))
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = direct_io;
        let file = std::fs::OpenOptions::new().write(true).open(path)?;
        Ok(Box::new(file))
    }
}

/// Combined Write + Seek trait for sparse write support.
pub(crate) trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

/// Sync/flush device buffers to ensure data is on disk.
pub(crate) fn sync_device(path: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let _ = path;
        unsafe {
            libc::sync();
        }
    }

    #[cfg(windows)]
    {
        // Open the device and call FlushFileBuffers via std::fs::File::sync_all
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
            if let Err(e) = file.sync_all() {
                warn!("FlushFileBuffers failed: {}. Data may not be fully flushed.", e);
            }
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
    }

    Ok(())
}

// ── Inline hasher (used during write for zero-cost verification) ───────────────

/// Trait for inline hashing during write. This uses the same DynHasher pattern
/// as hasher.rs but is private to the writer to avoid coupling.
trait InlineHasher: Send {
    fn update(&mut self, data: &[u8]);
    fn finalize_hex(self: Box<Self>) -> String;
}

struct DigestInline<D: Digest + Send>(D);
impl<D: Digest + Send> InlineHasher for DigestInline<D>
where
    digest::Output<D>: std::fmt::LowerHex,
{
    fn update(&mut self, data: &[u8]) { self.0.update(data); }
    fn finalize_hex(self: Box<Self>) -> String { format!("{:x}", self.0.finalize()) }
}

struct Blake3Inline(blake3::Hasher);
impl InlineHasher for Blake3Inline {
    fn update(&mut self, data: &[u8]) { self.0.update(data); }
    fn finalize_hex(self: Box<Self>) -> String { self.0.finalize().to_hex().to_string() }
}

struct Crc32Inline(crc32fast::Hasher);
impl InlineHasher for Crc32Inline {
    fn update(&mut self, data: &[u8]) { self.0.update(data); }
    fn finalize_hex(self: Box<Self>) -> String { format!("{:08x}", self.0.finalize()) }
}

fn create_inline_hasher(algo: HashAlgorithm) -> Box<dyn InlineHasher> {
    match algo {
        HashAlgorithm::Md5 => Box::new(DigestInline(md5::Md5::new())),
        HashAlgorithm::Sha1 => Box::new(DigestInline(sha1::Sha1::new())),
        HashAlgorithm::Sha256 => Box::new(DigestInline(sha2::Sha256::new())),
        HashAlgorithm::Sha512 => Box::new(DigestInline(sha2::Sha512::new())),
        HashAlgorithm::Blake3 => Box::new(Blake3Inline(blake3::Hasher::new())),
        HashAlgorithm::Crc32 => Box::new(Crc32Inline(crc32fast::Hasher::new())),
    }
}
