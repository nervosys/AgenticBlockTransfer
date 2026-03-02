// Loopback device testing — safe automated write/read testing without real media.
//
// Creates temporary files that act as virtual block devices, enabling the full
// write→verify pipeline to be exercised in CI without elevated privileges or
// physical hardware.

#![allow(dead_code)]

use anyhow::Result;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// A loopback "device" backed by a temporary file.
///
/// Provides the same interface as a real block device (seekable read/write)
/// but is safe for automated testing.
pub struct LoopbackDevice {
    file: NamedTempFile,
    size: u64,
}

impl LoopbackDevice {
    /// Create a new loopback device of the given size (bytes).
    /// The file is zero-filled and can be written/read like a block device.
    pub fn new(size: u64) -> Result<Self> {
        let mut file = NamedTempFile::new()?;
        // Pre-allocate by writing zeros (or seeking + writing a byte at end)
        file.as_file_mut().set_len(size)?;
        // Write a zero byte at the end to ensure the file is allocated
        file.as_file_mut().seek(SeekFrom::Start(size.saturating_sub(1)))?;
        file.as_file_mut().write_all(&[0])?;
        file.as_file_mut().seek(SeekFrom::Start(0))?;

        Ok(Self { file, size })
    }

    /// Get the path to the loopback device file.
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// Get the path as a String (for use with abt's string-based target paths).
    pub fn path_string(&self) -> String {
        self.file.path().to_string_lossy().to_string()
    }

    /// Get the size of the device.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Read the entire contents of the loopback device.
    pub fn read_all(&mut self) -> Result<Vec<u8>> {
        self.file.as_file_mut().seek(SeekFrom::Start(0))?;
        let mut buf = Vec::with_capacity(self.size as usize);
        self.file.as_file_mut().read_to_end(&mut buf)?;
        Ok(buf)
    }

    /// Read a range of bytes from the device.
    pub fn read_range(&mut self, offset: u64, len: usize) -> Result<Vec<u8>> {
        self.file.as_file_mut().seek(SeekFrom::Start(offset))?;
        let mut buf = vec![0u8; len];
        self.file.as_file_mut().read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Write data at a specific offset.
    pub fn write_at(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        self.file.as_file_mut().seek(SeekFrom::Start(offset))?;
        self.file.as_file_mut().write_all(data)?;
        self.file.as_file_mut().flush()?;
        Ok(())
    }

    /// Reset the device to all zeros.
    pub fn zero_fill(&mut self) -> Result<()> {
        self.file.as_file_mut().seek(SeekFrom::Start(0))?;
        let zeros = vec![0u8; 64 * 1024]; // 64 KiB chunks
        let mut remaining = self.size;
        while remaining > 0 {
            let chunk = std::cmp::min(remaining, zeros.len() as u64) as usize;
            self.file.as_file_mut().write_all(&zeros[..chunk])?;
            remaining -= chunk as u64;
        }
        self.file.as_file_mut().flush()?;
        Ok(())
    }

    /// Persist the loopback device to a named path (useful for debugging).
    pub fn persist_to(self, path: &Path) -> Result<PathBuf> {
        let persisted = self.file.into_temp_path();
        std::fs::copy(&persisted, path)?;
        Ok(path.to_path_buf())
    }
}

/// Create a test image file with a repeating byte pattern.
/// Returns a NamedTempFile with the given size and `.img` extension.
pub fn create_test_image(size: u64) -> Result<NamedTempFile> {
    let file = tempfile::Builder::new()
        .suffix(".img")
        .tempfile()?;

    let mut writer = std::io::BufWriter::new(file.as_file().try_clone()?);
    let pattern: Vec<u8> = (0..=255u8).collect();
    let mut written = 0u64;

    while written < size {
        let remaining = (size - written) as usize;
        let chunk = std::cmp::min(remaining, pattern.len());
        writer.write_all(&pattern[..chunk])?;
        written += chunk as u64;
    }
    writer.flush()?;

    Ok(file)
}

/// Create a test image with a specific byte pattern and compress it.
/// Returns the path to the compressed file.
pub fn create_compressed_test_image(size: u64, format: &str) -> Result<NamedTempFile> {
    let raw = create_test_image(size)?;
    let mut raw_data = Vec::new();
    std::fs::File::open(raw.path())?.read_to_end(&mut raw_data)?;

    let suffix = match format {
        "gz" | "gzip" => ".img.gz",
        "bz2" | "bzip2" => ".img.bz2",
        "xz" => ".img.xz",
        "zstd" | "zst" => ".img.zst",
        _ => anyhow::bail!("Unsupported compression format: {}", format),
    };

    let compressed = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()?;

    match format {
        "gz" | "gzip" => {
            let mut encoder =
                flate2::write::GzEncoder::new(compressed.as_file().try_clone()?, flate2::Compression::fast());
            encoder.write_all(&raw_data)?;
            encoder.finish()?;
        }
        "bz2" | "bzip2" => {
            let mut encoder =
                bzip2::write::BzEncoder::new(compressed.as_file().try_clone()?, bzip2::Compression::fast());
            encoder.write_all(&raw_data)?;
            encoder.finish()?;
        }
        "xz" => {
            let mut encoder = xz2::write::XzEncoder::new(compressed.as_file().try_clone()?, 1);
            encoder.write_all(&raw_data)?;
            encoder.finish()?;
        }
        "zstd" | "zst" => {
            let mut encoder = zstd::stream::write::Encoder::new(compressed.as_file().try_clone()?, 1)?;
            encoder.write_all(&raw_data)?;
            encoder.finish()?;
        }
        _ => unreachable!(),
    }

    Ok(compressed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_create_and_read() {
        let mut dev = LoopbackDevice::new(4096).unwrap();
        let data = dev.read_all().unwrap();
        assert_eq!(data.len(), 4096);
        assert!(data.iter().all(|&b| b == 0));
    }

    #[test]
    fn loopback_write_and_read_back() {
        let mut dev = LoopbackDevice::new(1024).unwrap();
        dev.write_at(0, b"Hello, loopback!").unwrap();
        let data = dev.read_range(0, 16).unwrap();
        assert_eq!(&data, b"Hello, loopback!");
    }

    #[test]
    fn loopback_zero_fill() {
        let mut dev = LoopbackDevice::new(2048).unwrap();
        dev.write_at(0, &[0xFF; 2048]).unwrap();
        dev.zero_fill().unwrap();
        let data = dev.read_all().unwrap();
        assert!(data.iter().all(|&b| b == 0));
    }

    #[test]
    fn loopback_path_is_valid() {
        let dev = LoopbackDevice::new(512).unwrap();
        assert!(dev.path().exists());
        assert!(!dev.path_string().is_empty());
    }

    #[test]
    fn create_test_image_correct_size() {
        let img = create_test_image(1024).unwrap();
        let meta = std::fs::metadata(img.path()).unwrap();
        assert_eq!(meta.len(), 1024);
    }

    #[test]
    fn create_test_image_has_pattern() {
        let img = create_test_image(512).unwrap();
        let mut data = Vec::new();
        std::fs::File::open(img.path())
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        // First 256 bytes should be 0..=255
        for i in 0..256 {
            assert_eq!(data[i], i as u8);
        }
        // Next 256 should repeat
        for i in 0..256 {
            assert_eq!(data[256 + i], i as u8);
        }
    }

    #[test]
    fn create_compressed_gz() {
        let compressed = create_compressed_test_image(2048, "gz").unwrap();
        assert!(compressed.path().exists());
        // Verify it starts with gzip magic
        let mut f = std::fs::File::open(compressed.path()).unwrap();
        let mut magic = [0u8; 2];
        f.read_exact(&mut magic).unwrap();
        assert_eq!(magic, [0x1f, 0x8b]);
    }

    #[test]
    fn create_compressed_xz() {
        let compressed = create_compressed_test_image(2048, "xz").unwrap();
        assert!(compressed.path().exists());
        let mut f = std::fs::File::open(compressed.path()).unwrap();
        let mut magic = [0u8; 6];
        f.read_exact(&mut magic).unwrap();
        assert_eq!(magic, [0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00]);
    }

    #[test]
    fn loopback_write_full_pipeline() {
        // Create a test image
        let img = create_test_image(4096).unwrap();

        // Create a loopback device
        let mut dev = LoopbackDevice::new(4096).unwrap();

        // "Write" the image to the loopback device (simulating writer)
        let mut src_data = Vec::new();
        std::fs::File::open(img.path())
            .unwrap()
            .read_to_end(&mut src_data)
            .unwrap();
        dev.write_at(0, &src_data).unwrap();

        // Read back and verify
        let dev_data = dev.read_all().unwrap();
        assert_eq!(src_data, dev_data);
    }

    #[test]
    fn loopback_partial_write() {
        let mut dev = LoopbackDevice::new(4096).unwrap();
        // Write only first 1024 bytes
        let pattern: Vec<u8> = (0..=255u8).cycle().take(1024).collect();
        dev.write_at(0, &pattern).unwrap();

        // First 1024 bytes should be the pattern
        let first = dev.read_range(0, 1024).unwrap();
        assert_eq!(first, pattern);

        // Rest should still be zeros
        let rest = dev.read_range(1024, 1024).unwrap();
        assert!(rest.iter().all(|&b| b == 0));
    }
}
