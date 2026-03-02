use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Supported image formats with automatic detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Raw,
    Iso,
    Img,
    Dmg,
    Vhd,
    Vhdx,
    Vmdk,
    Qcow2,
    Wim,
    Ffu,
    /// Compressed wrappers
    Gz,
    Bz2,
    Xz,
    Zstd,
    Zip,
}

impl fmt::Display for ImageFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Raw => write!(f, "raw"),
            Self::Iso => write!(f, "iso"),
            Self::Img => write!(f, "img"),
            Self::Dmg => write!(f, "dmg"),
            Self::Vhd => write!(f, "vhd"),
            Self::Vhdx => write!(f, "vhdx"),
            Self::Vmdk => write!(f, "vmdk"),
            Self::Qcow2 => write!(f, "qcow2"),
            Self::Wim => write!(f, "wim"),
            Self::Ffu => write!(f, "ffu"),
            Self::Gz => write!(f, "gz"),
            Self::Bz2 => write!(f, "bz2"),
            Self::Xz => write!(f, "xz"),
            Self::Zstd => write!(f, "zstd"),
            Self::Zip => write!(f, "zip"),
        }
    }
}

impl ImageFormat {
    /// Detect format from file extension.
    pub fn from_extension(path: &std::path::Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        match ext.as_str() {
            "raw" | "bin" | "dd" | "dsk" | "sdcard" | "hddimg" | "rpi-sdimg" => Some(Self::Raw),
            "iso" => Some(Self::Iso),
            "img" => Some(Self::Img),
            "dmg" => Some(Self::Dmg),
            "vhd" => Some(Self::Vhd),
            "vhdx" => Some(Self::Vhdx),
            "vmdk" => Some(Self::Vmdk),
            "qcow2" => Some(Self::Qcow2),
            "wim" => Some(Self::Wim),
            "ffu" => Some(Self::Ffu),
            "gz" | "gzip" => Some(Self::Gz),
            "bz2" | "bzip2" => Some(Self::Bz2),
            "xz" => Some(Self::Xz),
            "zst" | "zstd" => Some(Self::Zstd),
            "zip" => Some(Self::Zip),
            _ => None,
        }
    }

    /// Whether this format is a compression wrapper.
    pub fn is_compressed(&self) -> bool {
        matches!(self, Self::Gz | Self::Bz2 | Self::Xz | Self::Zstd | Self::Zip)
    }
}

/// Supported filesystem types for formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Filesystem {
    Fat16,
    Fat32,
    ExFat,
    Ntfs,
    Ext2,
    Ext3,
    Ext4,
    Xfs,
    Btrfs,
    /// For creating blank/zeroed media
    None,
}

impl fmt::Display for Filesystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fat16 => write!(f, "FAT16"),
            Self::Fat32 => write!(f, "FAT32"),
            Self::ExFat => write!(f, "exFAT"),
            Self::Ntfs => write!(f, "NTFS"),
            Self::Ext2 => write!(f, "ext2"),
            Self::Ext3 => write!(f, "ext3"),
            Self::Ext4 => write!(f, "ext4"),
            Self::Xfs => write!(f, "XFS"),
            Self::Btrfs => write!(f, "Btrfs"),
            Self::None => write!(f, "none"),
        }
    }
}

/// Hash algorithm choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Sha512,
    Blake3,
    Crc32,
}

impl fmt::Display for HashAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Md5 => write!(f, "MD5"),
            Self::Sha1 => write!(f, "SHA-1"),
            Self::Sha256 => write!(f, "SHA-256"),
            Self::Sha512 => write!(f, "SHA-512"),
            Self::Blake3 => write!(f, "BLAKE3"),
            Self::Crc32 => write!(f, "CRC32"),
        }
    }
}

/// Partition table type.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PartitionTable {
    Mbr,
    Gpt,
}

/// Device type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    Usb,
    Sd,
    Nvme,
    Sata,
    Scsi,
    Mmc,
    Emmc,
    Spi,
    I2cEeprom,
    Virtual,
    Network,
    Unknown,
}

impl fmt::Display for DeviceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usb => write!(f, "USB"),
            Self::Sd => write!(f, "SD"),
            Self::Nvme => write!(f, "NVMe"),
            Self::Sata => write!(f, "SATA"),
            Self::Scsi => write!(f, "SCSI"),
            Self::Mmc => write!(f, "MMC"),
            Self::Emmc => write!(f, "eMMC"),
            Self::Spi => write!(f, "SPI Flash"),
            Self::I2cEeprom => write!(f, "I2C EEPROM"),
            Self::Virtual => write!(f, "Virtual"),
            Self::Network => write!(f, "Network"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Write mode configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteMode {
    /// Raw dd-style block copy
    Raw,
    /// Extract ISO contents (for bootable USB creation)
    Extract,
    /// Clone entire device
    Clone,
}

/// Represents an image source — either a local file or a URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageSource {
    File(PathBuf),
    Url(String),
    Stdin,
}

impl fmt::Display for ImageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(p) => write!(f, "{}", p.display()),
            Self::Url(u) => write!(f, "{}", u),
            Self::Stdin => write!(f, "<stdin>"),
        }
    }
}

/// Configuration for a write operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteConfig {
    pub source: ImageSource,
    pub target: String,
    pub mode: WriteMode,
    pub block_size: usize,
    pub verify: bool,
    pub hash_algorithm: Option<HashAlgorithm>,
    pub expected_hash: Option<String>,
    pub force: bool,
    pub direct_io: bool,
    pub sync: bool,
    pub decompress: bool,
    /// Sparse write: skip all-zero blocks by seeking past them (like dd conv=sparse).
    pub sparse: bool,
}

impl Default for WriteConfig {
    fn default() -> Self {
        Self {
            source: ImageSource::File(PathBuf::new()),
            target: String::new(),
            mode: WriteMode::Raw,
            block_size: 4 * 1024 * 1024, // 4 MiB
            verify: true,
            hash_algorithm: Some(HashAlgorithm::Sha256),
            expected_hash: None,
            force: false,
            direct_io: true,
            sync: true,
            decompress: true,
            sparse: false,
        }
    }
}
