// Syslinux — bootloader installation and version management.
// Handles Syslinux/ISOLINUX/EXTLINUX detection, version identification,
// and installation planning for bootable USB drives. Also covers
// GRUB2 and GRUB4DOS detection. Inspired by Rufus's Syslinux support.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Supported bootloader types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BootloaderType {
    /// Syslinux v4.x (legacy, FAT only).
    SyslinuxV4,
    /// Syslinux v6.x (FAT + NTFS support).
    SyslinuxV6,
    /// ISOLINUX (CD/DVD boot, part of Syslinux project).
    Isolinux,
    /// EXTLINUX (ext2/3/4/btrfs boot, part of Syslinux project).
    Extlinux,
    /// GRUB 2.x (modern, multi-filesystem).
    Grub2,
    /// GRUB4DOS (legacy, multi-boot).
    Grub4dos,
    /// Windows Boot Manager.
    Bootmgr,
    /// ReactOS FreeLoader.
    Freeldr,
}

impl fmt::Display for BootloaderType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SyslinuxV4 => write!(f, "Syslinux v4"),
            Self::SyslinuxV6 => write!(f, "Syslinux v6"),
            Self::Isolinux => write!(f, "ISOLINUX"),
            Self::Extlinux => write!(f, "EXTLINUX"),
            Self::Grub2 => write!(f, "GRUB 2"),
            Self::Grub4dos => write!(f, "GRUB4DOS"),
            Self::Bootmgr => write!(f, "Windows Boot Manager"),
            Self::Freeldr => write!(f, "ReactOS FreeLoader"),
        }
    }
}

impl BootloaderType {
    /// Whether this bootloader supports FAT filesystems.
    pub fn supports_fat(&self) -> bool {
        matches!(
            self,
            Self::SyslinuxV4
                | Self::SyslinuxV6
                | Self::Isolinux
                | Self::Grub2
                | Self::Grub4dos
                | Self::Bootmgr
                | Self::Freeldr
        )
    }

    /// Whether this bootloader supports NTFS.
    pub fn supports_ntfs(&self) -> bool {
        matches!(
            self,
            Self::SyslinuxV6 | Self::Grub2 | Self::Grub4dos | Self::Bootmgr
        )
    }

    /// Whether this bootloader supports ext2/3/4.
    pub fn supports_ext(&self) -> bool {
        matches!(self, Self::Extlinux | Self::Grub2 | Self::Grub4dos)
    }

    /// Whether this bootloader supports UEFI boot.
    pub fn supports_uefi(&self) -> bool {
        matches!(self, Self::Grub2 | Self::Bootmgr)
    }

    /// Whether this bootloader supports BIOS/legacy boot.
    pub fn supports_bios(&self) -> bool {
        matches!(
            self,
            Self::SyslinuxV4
                | Self::SyslinuxV6
                | Self::Isolinux
                | Self::Extlinux
                | Self::Grub2
                | Self::Grub4dos
                | Self::Bootmgr
                | Self::Freeldr
        )
    }

    /// Required files for this bootloader on the target.
    pub fn required_files(&self) -> Vec<&'static str> {
        match self {
            Self::SyslinuxV4 | Self::SyslinuxV6 => {
                vec!["ldlinux.sys", "ldlinux.c32"]
            }
            Self::Isolinux => vec!["isolinux.bin", "isolinux.cfg"],
            Self::Extlinux => vec!["ldlinux.sys", "extlinux.conf"],
            Self::Grub2 => vec!["grub/grub.cfg"],
            Self::Grub4dos => vec!["grldr", "menu.lst"],
            Self::Bootmgr => vec!["bootmgr", "BCD"],
            Self::Freeldr => vec!["freeldr.sys", "freeldr.ini"],
        }
    }
}

/// Syslinux version information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyslinuxVersion {
    /// Major version number.
    pub major: u32,
    /// Minor version number.
    pub minor: u32,
    /// Patch/revision.
    pub patch: Option<u32>,
    /// Pre-release suffix (e.g., "pre1").
    pub pre_release: Option<String>,
    /// Full version string as found in the binary.
    pub version_string: String,
}

impl fmt::Display for SyslinuxVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)?;
        if let Some(p) = self.patch {
            write!(f, ".{}", p)?;
        }
        if let Some(ref pre) = self.pre_release {
            write!(f, "-{}", pre)?;
        }
        Ok(())
    }
}

impl SyslinuxVersion {
    /// Whether this is Syslinux 6.x or later.
    pub fn is_v6_or_later(&self) -> bool {
        self.major >= 6
    }

    /// Whether this version requires the .c32 module chain.
    pub fn needs_c32_modules(&self) -> bool {
        self.major >= 5
    }

    /// Compare versions.
    pub fn is_newer_than(&self, other: &SyslinuxVersion) -> bool {
        if self.major != other.major {
            return self.major > other.major;
        }
        if self.minor != other.minor {
            return self.minor > other.minor;
        }
        match (self.patch, other.patch) {
            (Some(a), Some(b)) => a > b,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

/// Detection result for a bootloader in an ISO or device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootloaderDetection {
    /// Type of bootloader detected.
    pub bootloader_type: BootloaderType,
    /// Version (if determinable).
    pub version: Option<SyslinuxVersion>,
    /// Files associated with this bootloader that were found.
    pub files_found: Vec<String>,
    /// Files that are expected but missing.
    pub files_missing: Vec<String>,
    /// Confidence level (0.0 - 1.0).
    pub confidence: f64,
    /// Additional notes.
    pub notes: Vec<String>,
}

impl fmt::Display for BootloaderDetection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.bootloader_type)?;
        if let Some(ref ver) = self.version {
            write!(f, " v{}", ver)?;
        }
        write!(f, " (confidence: {:.0}%)", self.confidence * 100.0)?;
        if !self.files_found.is_empty() {
            write!(f, " [found: {}]", self.files_found.join(", "))?;
        }
        Ok(())
    }
}

/// Syslinux configuration template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyslinuxConfig {
    /// Default boot entry label.
    pub default_label: String,
    /// Boot timeout in tenths of a second (0 = wait forever).
    pub timeout: u32,
    /// Boot prompt display.
    pub prompt: bool,
    /// Boot entries.
    pub entries: Vec<BootEntry>,
    /// Optional splash/background image.
    pub splash: Option<String>,
    /// Menu title.
    pub menu_title: Option<String>,
}

impl Default for SyslinuxConfig {
    fn default() -> Self {
        Self {
            default_label: "linux".to_string(),
            timeout: 100, // 10 seconds
            prompt: true,
            entries: Vec::new(),
            splash: None,
            menu_title: None,
        }
    }
}

/// A single boot entry in Syslinux configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootEntry {
    /// Label identifier.
    pub label: String,
    /// Menu display text.
    pub menu_label: String,
    /// Kernel or COM32 module path.
    pub kernel: String,
    /// Append parameters (kernel command line).
    pub append: String,
    /// Initial ramdisk path.
    pub initrd: Option<String>,
}

/// Installation plan for a bootloader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    /// Bootloader to install.
    pub bootloader_type: BootloaderType,
    /// Target filesystem.
    pub target_fs: String,
    /// Files to copy to the target.
    pub files_to_copy: Vec<FileCopy>,
    /// MBR/VBR modifications needed.
    pub boot_record_action: BootRecordAction,
    /// Whether internet download is needed for additional files.
    pub needs_download: bool,
    /// Download URLs for additional files.
    pub download_urls: Vec<String>,
    /// Estimated total size of files to install.
    pub total_size: u64,
}

impl fmt::Display for InstallPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Install {} on {}: {} file(s), {}",
            self.bootloader_type,
            self.target_fs,
            self.files_to_copy.len(),
            self.boot_record_action,
        )
    }
}

/// A file copy operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileCopy {
    /// Source path (relative to ISO or resource).
    pub source: String,
    /// Destination path on target.
    pub destination: String,
    /// File size in bytes.
    pub size: u64,
    /// Whether the file must be set to hidden+system attributes.
    pub hidden_system: bool,
}

/// Boot record modification action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BootRecordAction {
    /// Write Syslinux MBR.
    WriteSyslinuxMbr,
    /// Write GRUB2 MBR.
    WriteGrub2Mbr,
    /// Write GRUB4DOS MBR.
    WriteGrub4dosMbr,
    /// Write generic protective MBR.
    WriteProtectiveMbr,
    /// No MBR modification needed.
    None,
}

impl fmt::Display for BootRecordAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteSyslinuxMbr => write!(f, "write Syslinux MBR"),
            Self::WriteGrub2Mbr => write!(f, "write GRUB2 MBR"),
            Self::WriteGrub4dosMbr => write!(f, "write GRUB4DOS MBR"),
            Self::WriteProtectiveMbr => write!(f, "write protective MBR"),
            Self::None => write!(f, "no MBR change"),
        }
    }
}

/// Parse a Syslinux version string from binary data.
///
/// Syslinux binaries embed version strings like "SYSLINUX 6.04" or
/// "SYSLINUX 4.07 EDD 2013-07-25" in their ldlinux.sys/ldlinux.c32.
pub fn parse_syslinux_version(data: &[u8]) -> Option<SyslinuxVersion> {
    let text = String::from_utf8_lossy(data);

    // Look for "SYSLINUX X.YY" or "EXTLINUX X.YY" pattern
    for prefix in &["SYSLINUX ", "EXTLINUX ", "ISOLINUX "] {
        if let Some(pos) = text.find(prefix) {
            let version_start = pos + prefix.len();
            let remaining = &text[version_start..];

            // Extract version digits
            let version_str: String = remaining
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
                .collect();

            return parse_version_string(&version_str);
        }
    }

    None
}

/// Parse a version string like "6.04", "4.07", "6.04-pre1".
pub fn parse_version_string(version: &str) -> Option<SyslinuxVersion> {
    let (version_part, pre_release) = if let Some(idx) = version.find('-') {
        (&version[..idx], Some(version[idx + 1..].to_string()))
    } else {
        (version, None)
    };

    let parts: Vec<&str> = version_part.split('.').collect();
    if parts.is_empty() {
        return None;
    }

    let major = parts[0].parse::<u32>().ok()?;
    let minor = parts.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|s| s.parse::<u32>().ok());

    Some(SyslinuxVersion {
        major,
        minor,
        patch,
        pre_release: pre_release.filter(|s| !s.is_empty()),
        version_string: version.to_string(),
    })
}

/// Detect bootloaders in a list of file paths (from an ISO or device).
pub fn detect_bootloaders(file_paths: &[&str]) -> Vec<BootloaderDetection> {
    let mut detections = Vec::new();
    let paths_lower: Vec<String> = file_paths.iter().map(|p| p.to_lowercase()).collect();

    // Check for Syslinux
    let syslinux_files = check_bootloader_files(
        &paths_lower,
        &["ldlinux.sys", "ldlinux.c32", "syslinux.cfg"],
    );
    if !syslinux_files.is_empty() {
        let has_c32 = syslinux_files.iter().any(|f| f.ends_with(".c32"));
        let bl_type = if has_c32 {
            BootloaderType::SyslinuxV6
        } else {
            BootloaderType::SyslinuxV4
        };
        detections.push(BootloaderDetection {
            bootloader_type: bl_type,
            version: None,
            files_found: syslinux_files,
            files_missing: Vec::new(),
            confidence: 0.85,
            notes: Vec::new(),
        });
    }

    // Check for ISOLINUX
    let isolinux_files = check_bootloader_files(
        &paths_lower,
        &["isolinux.bin", "isolinux.cfg", "isolinux/isolinux.bin"],
    );
    if !isolinux_files.is_empty() {
        detections.push(BootloaderDetection {
            bootloader_type: BootloaderType::Isolinux,
            version: None,
            files_found: isolinux_files,
            files_missing: Vec::new(),
            confidence: 0.90,
            notes: Vec::new(),
        });
    }

    // Check for GRUB2
    let grub2_files = check_bootloader_files(
        &paths_lower,
        &[
            "grub/grub.cfg",
            "boot/grub/grub.cfg",
            "efi/boot/grubx64.efi",
            "boot/grub2/grub.cfg",
        ],
    );
    if !grub2_files.is_empty() {
        detections.push(BootloaderDetection {
            bootloader_type: BootloaderType::Grub2,
            version: None,
            files_found: grub2_files,
            files_missing: Vec::new(),
            confidence: 0.90,
            notes: Vec::new(),
        });
    }

    // Check for GRUB4DOS
    let grub4dos_files = check_bootloader_files(&paths_lower, &["grldr", "menu.lst"]);
    if !grub4dos_files.is_empty() {
        detections.push(BootloaderDetection {
            bootloader_type: BootloaderType::Grub4dos,
            version: None,
            files_found: grub4dos_files,
            files_missing: Vec::new(),
            confidence: 0.85,
            notes: Vec::new(),
        });
    }

    // Check for Windows Boot Manager
    let bootmgr_files = check_bootloader_files(
        &paths_lower,
        &[
            "bootmgr",
            "bootmgr.efi",
            "boot/bcd",
            "efi/boot/bootx64.efi",
            "efi/microsoft/boot/bcd",
        ],
    );
    if !bootmgr_files.is_empty() {
        detections.push(BootloaderDetection {
            bootloader_type: BootloaderType::Bootmgr,
            version: None,
            files_found: bootmgr_files,
            files_missing: Vec::new(),
            confidence: 0.90,
            notes: Vec::new(),
        });
    }

    // Check for ReactOS FreeLoader
    let freeldr_files = check_bootloader_files(
        &paths_lower,
        &["freeldr.sys", "freeldr.ini"],
    );
    if !freeldr_files.is_empty() {
        detections.push(BootloaderDetection {
            bootloader_type: BootloaderType::Freeldr,
            version: None,
            files_found: freeldr_files,
            files_missing: Vec::new(),
            confidence: 0.85,
            notes: Vec::new(),
        });
    }

    detections
}

/// Check which bootloader files exist in the path list.
fn check_bootloader_files(paths: &[String], check_files: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    for check in check_files {
        let check_lower = check.to_lowercase();
        for path in paths {
            if path.ends_with(&check_lower) || path.contains(&check_lower) {
                found.push(check.to_string());
                break;
            }
        }
    }
    found
}

/// Generate a Syslinux configuration file.
pub fn generate_syslinux_config(config: &SyslinuxConfig) -> String {
    let mut out = String::new();

    // Header
    if let Some(ref title) = config.menu_title {
        out.push_str(&format!("MENU TITLE {}\n", title));
    }
    out.push_str(&format!("DEFAULT {}\n", config.default_label));
    out.push_str(&format!("TIMEOUT {}\n", config.timeout));
    if config.prompt {
        out.push_str("PROMPT 1\n");
    } else {
        out.push_str("PROMPT 0\n");
    }
    if let Some(ref splash) = config.splash {
        out.push_str(&format!("MENU BACKGROUND {}\n", splash));
    }
    out.push('\n');

    // Boot entries
    for entry in &config.entries {
        out.push_str(&format!("LABEL {}\n", entry.label));
        out.push_str(&format!("  MENU LABEL {}\n", entry.menu_label));
        out.push_str(&format!("  KERNEL {}\n", entry.kernel));
        if let Some(ref initrd) = entry.initrd {
            out.push_str(&format!("  INITRD {}\n", initrd));
        }
        if !entry.append.is_empty() {
            out.push_str(&format!("  APPEND {}\n", entry.append));
        }
        out.push('\n');
    }

    out
}

/// Create an installation plan for a bootloader.
pub fn plan_installation(
    bootloader: BootloaderType,
    target_fs: &str,
) -> Result<InstallPlan> {
    // Validate filesystem compatibility
    let fs_lower = target_fs.to_lowercase();
    match bootloader {
        BootloaderType::SyslinuxV4 => {
            if !matches!(fs_lower.as_str(), "fat" | "fat16" | "fat32" | "vfat") {
                return Err(anyhow!(
                    "Syslinux v4 only supports FAT filesystems, not {}",
                    target_fs
                ));
            }
        }
        BootloaderType::SyslinuxV6 => {
            if !matches!(
                fs_lower.as_str(),
                "fat" | "fat16" | "fat32" | "vfat" | "ntfs"
            ) {
                return Err(anyhow!(
                    "Syslinux v6 only supports FAT and NTFS, not {}",
                    target_fs
                ));
            }
        }
        BootloaderType::Extlinux => {
            if !matches!(fs_lower.as_str(), "ext2" | "ext3" | "ext4" | "btrfs") {
                return Err(anyhow!(
                    "EXTLINUX only supports ext2/3/4 and btrfs, not {}",
                    target_fs
                ));
            }
        }
        _ => {} // GRUB2 and others support most filesystems
    }

    let boot_record_action = match bootloader {
        BootloaderType::SyslinuxV4 | BootloaderType::SyslinuxV6 | BootloaderType::Extlinux => {
            BootRecordAction::WriteSyslinuxMbr
        }
        BootloaderType::Grub2 => BootRecordAction::WriteGrub2Mbr,
        BootloaderType::Grub4dos => BootRecordAction::WriteGrub4dosMbr,
        _ => BootRecordAction::None,
    };

    let needs_download = matches!(
        bootloader,
        BootloaderType::SyslinuxV6 | BootloaderType::Grub2
    );

    let files_to_copy: Vec<FileCopy> = bootloader
        .required_files()
        .iter()
        .map(|f| FileCopy {
            source: f.to_string(),
            destination: f.to_string(),
            size: 0, // Would be filled in by actual installer
            hidden_system: *f == "ldlinux.sys",
        })
        .collect();

    Ok(InstallPlan {
        bootloader_type: bootloader,
        target_fs: target_fs.to_string(),
        files_to_copy,
        boot_record_action,
        needs_download,
        download_urls: Vec::new(),
        total_size: 0,
    })
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bootloader_type_display() {
        assert_eq!(BootloaderType::SyslinuxV4.to_string(), "Syslinux v4");
        assert_eq!(BootloaderType::SyslinuxV6.to_string(), "Syslinux v6");
        assert_eq!(BootloaderType::Isolinux.to_string(), "ISOLINUX");
        assert_eq!(BootloaderType::Extlinux.to_string(), "EXTLINUX");
        assert_eq!(BootloaderType::Grub2.to_string(), "GRUB 2");
        assert_eq!(BootloaderType::Grub4dos.to_string(), "GRUB4DOS");
        assert_eq!(BootloaderType::Bootmgr.to_string(), "Windows Boot Manager");
        assert_eq!(BootloaderType::Freeldr.to_string(), "ReactOS FreeLoader");
    }

    #[test]
    fn test_bootloader_fs_support() {
        assert!(BootloaderType::SyslinuxV4.supports_fat());
        assert!(!BootloaderType::SyslinuxV4.supports_ntfs());
        assert!(BootloaderType::SyslinuxV6.supports_ntfs());
        assert!(BootloaderType::Grub2.supports_ext());
        assert!(BootloaderType::Grub2.supports_uefi());
        assert!(!BootloaderType::SyslinuxV4.supports_uefi());
        assert!(BootloaderType::SyslinuxV4.supports_bios());
        assert!(BootloaderType::Extlinux.supports_ext());
    }

    #[test]
    fn test_bootloader_required_files() {
        let files = BootloaderType::SyslinuxV4.required_files();
        assert!(files.contains(&"ldlinux.sys"));
        let files = BootloaderType::Grub2.required_files();
        assert!(files.contains(&"grub/grub.cfg"));
    }

    #[test]
    fn test_parse_version_string() {
        let v = parse_version_string("6.04").unwrap();
        assert_eq!(v.major, 6);
        assert_eq!(v.minor, 4);
        assert!(v.patch.is_none());
        assert!(v.pre_release.is_none());

        let v = parse_version_string("6.04-pre1").unwrap();
        assert_eq!(v.major, 6);
        assert_eq!(v.minor, 4);
        assert_eq!(v.pre_release, Some("pre1".to_string()));

        let v = parse_version_string("4.07").unwrap();
        assert_eq!(v.major, 4);
        assert_eq!(v.minor, 7);
    }

    #[test]
    fn test_syslinux_version_display() {
        let v = SyslinuxVersion {
            major: 6,
            minor: 4,
            patch: None,
            pre_release: None,
            version_string: "6.04".into(),
        };
        assert_eq!(v.to_string(), "6.4");

        let v = SyslinuxVersion {
            major: 6,
            minor: 4,
            patch: Some(1),
            pre_release: Some("pre1".into()),
            version_string: "6.04.1-pre1".into(),
        };
        assert_eq!(v.to_string(), "6.4.1-pre1");
    }

    #[test]
    fn test_syslinux_version_comparison() {
        let v4 = parse_version_string("4.07").unwrap();
        let v6 = parse_version_string("6.04").unwrap();
        assert!(v6.is_newer_than(&v4));
        assert!(!v4.is_newer_than(&v6));
        assert!(v6.is_v6_or_later());
        assert!(!v4.is_v6_or_later());
    }

    #[test]
    fn test_syslinux_version_needs_c32() {
        let v4 = parse_version_string("4.07").unwrap();
        let v5 = parse_version_string("5.10").unwrap();
        assert!(!v4.needs_c32_modules());
        assert!(v5.needs_c32_modules());
    }

    #[test]
    fn test_parse_syslinux_version_from_binary() {
        let data = b"Some binary data SYSLINUX 6.04 EDD 2019-02-22 more data";
        let v = parse_syslinux_version(data).unwrap();
        assert_eq!(v.major, 6);
        assert_eq!(v.minor, 4);

        let data = b"ISOLINUX 3.86 more stuff";
        let v = parse_syslinux_version(data).unwrap();
        assert_eq!(v.major, 3);
        assert_eq!(v.minor, 86);
    }

    #[test]
    fn test_parse_syslinux_version_not_found() {
        let data = b"No syslinux here, just random binary data";
        assert!(parse_syslinux_version(data).is_none());
    }

    #[test]
    fn test_detect_bootloaders_syslinux() {
        let files = vec![
            "/boot/ldlinux.sys",
            "/boot/ldlinux.c32",
            "/boot/syslinux.cfg",
        ];
        let detections = detect_bootloaders(&files);
        assert!(!detections.is_empty());
        assert_eq!(detections[0].bootloader_type, BootloaderType::SyslinuxV6);
    }

    #[test]
    fn test_detect_bootloaders_isolinux() {
        let files = vec!["/isolinux/isolinux.bin", "/isolinux/isolinux.cfg"];
        let detections = detect_bootloaders(&files);
        assert!(detections
            .iter()
            .any(|d| d.bootloader_type == BootloaderType::Isolinux));
    }

    #[test]
    fn test_detect_bootloaders_grub2() {
        let files = vec!["/boot/grub/grub.cfg", "/EFI/BOOT/grubx64.efi"];
        let detections = detect_bootloaders(&files);
        assert!(detections
            .iter()
            .any(|d| d.bootloader_type == BootloaderType::Grub2));
    }

    #[test]
    fn test_detect_bootloaders_grub4dos() {
        let files = vec!["/grldr", "/menu.lst"];
        let detections = detect_bootloaders(&files);
        assert!(detections
            .iter()
            .any(|d| d.bootloader_type == BootloaderType::Grub4dos));
    }

    #[test]
    fn test_detect_bootloaders_bootmgr() {
        let files = vec!["/bootmgr", "/Boot/BCD", "/EFI/Boot/bootx64.efi"];
        let detections = detect_bootloaders(&files);
        assert!(detections
            .iter()
            .any(|d| d.bootloader_type == BootloaderType::Bootmgr));
    }

    #[test]
    fn test_detect_bootloaders_none() {
        let files: Vec<&str> = vec!["/readme.txt", "/autorun.inf"];
        let detections = detect_bootloaders(&files);
        assert!(detections.is_empty());
    }

    #[test]
    fn test_detect_bootloaders_multiple() {
        let files = vec![
            "/isolinux/isolinux.bin",
            "/isolinux/isolinux.cfg",
            "/boot/grub/grub.cfg",
            "/EFI/BOOT/grubx64.efi",
        ];
        let detections = detect_bootloaders(&files);
        assert!(detections.len() >= 2);
    }

    #[test]
    fn test_generate_syslinux_config() {
        let config = SyslinuxConfig {
            default_label: "linux".to_string(),
            timeout: 50,
            prompt: true,
            menu_title: Some("My Boot Menu".to_string()),
            splash: None,
            entries: vec![
                BootEntry {
                    label: "linux".to_string(),
                    menu_label: "Boot Linux".to_string(),
                    kernel: "/vmlinuz".to_string(),
                    append: "root=/dev/sda1 ro quiet".to_string(),
                    initrd: Some("/initrd.img".to_string()),
                },
                BootEntry {
                    label: "safe".to_string(),
                    menu_label: "Safe Mode".to_string(),
                    kernel: "/vmlinuz".to_string(),
                    append: "root=/dev/sda1 ro single".to_string(),
                    initrd: Some("/initrd.img".to_string()),
                },
            ],
        };
        let output = generate_syslinux_config(&config);
        assert!(output.contains("MENU TITLE My Boot Menu"));
        assert!(output.contains("DEFAULT linux"));
        assert!(output.contains("TIMEOUT 50"));
        assert!(output.contains("PROMPT 1"));
        assert!(output.contains("LABEL linux"));
        assert!(output.contains("KERNEL /vmlinuz"));
        assert!(output.contains("INITRD /initrd.img"));
        assert!(output.contains("APPEND root=/dev/sda1 ro quiet"));
        assert!(output.contains("LABEL safe"));
    }

    #[test]
    fn test_plan_installation_syslinux_fat() {
        let plan = plan_installation(BootloaderType::SyslinuxV4, "FAT32").unwrap();
        assert_eq!(plan.bootloader_type, BootloaderType::SyslinuxV4);
        assert_eq!(plan.boot_record_action, BootRecordAction::WriteSyslinuxMbr);
        assert!(!plan.files_to_copy.is_empty());
    }

    #[test]
    fn test_plan_installation_syslinux_ntfs_v4_fails() {
        let result = plan_installation(BootloaderType::SyslinuxV4, "NTFS");
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_installation_syslinux_ntfs_v6_ok() {
        let plan = plan_installation(BootloaderType::SyslinuxV6, "NTFS").unwrap();
        assert_eq!(plan.bootloader_type, BootloaderType::SyslinuxV6);
    }

    #[test]
    fn test_plan_installation_extlinux_ext4() {
        let plan = plan_installation(BootloaderType::Extlinux, "ext4").unwrap();
        assert_eq!(plan.bootloader_type, BootloaderType::Extlinux);
    }

    #[test]
    fn test_plan_installation_extlinux_fat_fails() {
        let result = plan_installation(BootloaderType::Extlinux, "FAT32");
        assert!(result.is_err());
    }

    #[test]
    fn test_plan_installation_grub2() {
        let plan = plan_installation(BootloaderType::Grub2, "ext4").unwrap();
        assert_eq!(plan.boot_record_action, BootRecordAction::WriteGrub2Mbr);
        assert!(plan.needs_download);
    }

    #[test]
    fn test_boot_record_action_display() {
        assert_eq!(
            BootRecordAction::WriteSyslinuxMbr.to_string(),
            "write Syslinux MBR"
        );
        assert_eq!(BootRecordAction::None.to_string(), "no MBR change");
    }

    #[test]
    fn test_install_plan_display() {
        let plan = InstallPlan {
            bootloader_type: BootloaderType::SyslinuxV6,
            target_fs: "FAT32".to_string(),
            files_to_copy: vec![FileCopy {
                source: "ldlinux.sys".into(),
                destination: "ldlinux.sys".into(),
                size: 120000,
                hidden_system: true,
            }],
            boot_record_action: BootRecordAction::WriteSyslinuxMbr,
            needs_download: false,
            download_urls: Vec::new(),
            total_size: 120000,
        };
        let s = plan.to_string();
        assert!(s.contains("Syslinux v6"));
        assert!(s.contains("FAT32"));
    }

    #[test]
    fn test_bootloader_detection_display() {
        let det = BootloaderDetection {
            bootloader_type: BootloaderType::Grub2,
            version: None,
            files_found: vec!["grub/grub.cfg".into()],
            files_missing: Vec::new(),
            confidence: 0.90,
            notes: Vec::new(),
        };
        let s = det.to_string();
        assert!(s.contains("GRUB 2"));
        assert!(s.contains("90%"));
    }

    #[test]
    fn test_file_copy_hidden_system() {
        let plan = plan_installation(BootloaderType::SyslinuxV4, "FAT32").unwrap();
        let ldlinux = plan
            .files_to_copy
            .iter()
            .find(|f| f.destination == "ldlinux.sys");
        assert!(ldlinux.is_some());
        assert!(ldlinux.unwrap().hidden_system);
    }
}
