use anyhow::Result;
use std::io::{BufReader, Read};
use std::path::Path;

use super::types::ImageFormat;
use super::qcow2;
use super::vhd;
use super::vmdk;

/// BufReader capacity for decompressors (256 KiB — decompressors benefit from
/// a well-sized upstream buffer even though they also buffer internally).
const DECOMPRESS_BUF_SIZE: usize = 256 * 1024;

/// Metadata about an image file.
#[derive(Debug, Clone)]
pub struct ImageInfo {
    pub path: String,
    pub format: ImageFormat,
    pub size: u64,
    pub decompressed_size: Option<u64>,
    pub inner_format: Option<ImageFormat>,
}

/// Detect the image format from magic bytes and extension.
///
/// Opens the file once, reads 16 bytes, then closes. If you plan to also read
/// from the file afterward, prefer `open_image` which detects + opens in one step.
pub fn detect_format(path: &Path) -> Result<ImageFormat> {
    // Try magic bytes first
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut magic = [0u8; 16];
        if f.read(&mut magic).unwrap_or(0) >= 4 {
            if let Some(fmt) = detect_from_magic(&magic) {
                return Ok(fmt);
            }
        }
    }

    // Fall back to extension
    ImageFormat::from_extension(path)
        .ok_or_else(|| anyhow::anyhow!("Unable to detect image format for {}", path.display()))
}

fn detect_from_magic(magic: &[u8; 16]) -> Option<ImageFormat> {
    // Use slice matching for cleaner comparisons
    match magic {
        [0x1f, 0x8b, ..] => Some(ImageFormat::Gz),
        [0x42, 0x5a, 0x68, ..] => Some(ImageFormat::Bz2),
        [0xfd, 0x37, 0x7a, 0x58, 0x5a, 0x00, ..] => Some(ImageFormat::Xz),
        [0x28, 0xb5, 0x2f, 0xfd, ..] => Some(ImageFormat::Zstd),
        [0x50, 0x4b, 0x03, 0x04, ..] => Some(ImageFormat::Zip),
        [0x51, 0x46, 0x49, 0xfb, ..] => Some(ImageFormat::Qcow2),
        [0x4b, 0x44, 0x4d, 0x56, ..] => Some(ImageFormat::Vmdk),
        _ => {
            if &magic[0..8] == b"vhdxfile" {
                Some(ImageFormat::Vhdx)
            } else if &magic[0..6] == b"MSWIM\0" {
                Some(ImageFormat::Wim)
            } else {
                None
            }
        }
    }
}

/// Get information about an image file.
pub fn get_image_info(path: &Path) -> Result<ImageInfo> {
    let metadata = std::fs::metadata(path)?;
    let format = detect_format(path)?;

    // Determine inner format for compressed files
    let inner_format = if format.is_compressed() {
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let inner_path = Path::new(stem);
        ImageFormat::from_extension(inner_path)
    } else {
        None
    };

    Ok(ImageInfo {
        path: path.to_string_lossy().to_string(),
        format,
        size: metadata.len(),
        decompressed_size: None,
        inner_format,
    })
}

/// Open an image file and wrap it in the appropriate decompressor.
/// For uncompressed formats, returns a BufReader-wrapped file handle.
///
/// The file is opened exactly once and a BufReader is inserted between
/// the raw File and the decompressor for optimal I/O patterns.
pub fn open_image(path: &Path) -> Result<Box<dyn Read + Send>> {
    let format = detect_format(path)?;
    let file = std::fs::File::open(path)?;
    let buffered = BufReader::with_capacity(DECOMPRESS_BUF_SIZE, file);

    match format {
        ImageFormat::Gz => {
            let decoder = flate2::read::GzDecoder::new(buffered);
            Ok(Box::new(decoder))
        }
        ImageFormat::Bz2 => {
            let decoder = bzip2::read::BzDecoder::new(buffered);
            Ok(Box::new(decoder))
        }
        ImageFormat::Xz => {
            let decoder = xz2::read::XzDecoder::new(buffered);
            Ok(Box::new(decoder))
        }
        ImageFormat::Zstd => {
            let decoder = zstd::stream::read::Decoder::new(buffered)?;
            Ok(Box::new(decoder))
        }
        ImageFormat::Qcow2 => {
            // Re-open the file for seekable QCOW2 reader
            drop(buffered);
            let reader = qcow2::open_qcow2(path)?;
            Ok(Box::new(reader))
        }
        ImageFormat::Vhd => {
            drop(buffered);
            let reader = vhd::open_vhd(path)?;
            Ok(Box::new(reader))
        }
        ImageFormat::Vmdk => {
            drop(buffered);
            let reader = vmdk::open_vmdk(path)?;
            Ok(Box::new(reader))
        }
        _ => Ok(Box::new(buffered)),
    }
}
