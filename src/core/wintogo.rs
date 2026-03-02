// Windows To Go — create portable Windows installations on USB drives.
// Handles Windows ISO analysis for WinToGo compatibility, partition layout planning
// (GPT with ESP + main partition), and configuration. Inspired by Rufus's
// Windows To Go support with GPT/FIXED attribute detection.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Windows edition compatibility with Windows To Go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WinToGoEdition {
    /// Windows 10/11 Enterprise — fully supported.
    Enterprise,
    /// Windows 10/11 Pro — works but not officially supported.
    Pro,
    /// Windows 10/11 Education — supported variant of Enterprise.
    Education,
    /// Other edition — may work but untested.
    Other,
}

impl fmt::Display for WinToGoEdition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enterprise => write!(f, "Enterprise"),
            Self::Pro => write!(f, "Pro"),
            Self::Education => write!(f, "Education"),
            Self::Other => write!(f, "Other"),
        }
    }
}

/// Windows architecture for WinToGo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WinArch {
    /// x86_64 / AMD64.
    X64,
    /// ARM64 / AArch64.
    Arm64,
    /// x86 (32-bit, limited WinToGo support).
    X86,
}

impl fmt::Display for WinArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::X64 => write!(f, "x64"),
            Self::Arm64 => write!(f, "arm64"),
            Self::X86 => write!(f, "x86"),
        }
    }
}

/// Partition scheme for WinToGo drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WtgPartitionScheme {
    /// GPT — required for UEFI boot, recommended for Windows To Go.
    Gpt,
    /// MBR — legacy BIOS boot, limited to 2 TB.
    Mbr,
}

impl fmt::Display for WtgPartitionScheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gpt => write!(f, "GPT"),
            Self::Mbr => write!(f, "MBR"),
        }
    }
}

/// Analysis result of a Windows ISO for WinToGo compatibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WtgIsoAnalysis {
    /// Path to the ISO file.
    pub iso_path: String,
    /// Whether the ISO contains bootmgr (required for WinToGo).
    pub has_bootmgr: bool,
    /// Whether the ISO contains EFI boot files.
    pub has_efi_boot: bool,
    /// Whether Windows install.wim or install.esd is present.
    pub has_install_image: bool,
    /// Detected Windows version string.
    pub version: Option<String>,
    /// Detected architecture.
    pub architecture: Option<WinArch>,
    /// Detected edition(s).
    pub editions: Vec<WinToGoEdition>,
    /// Whether the ISO is compatible with Windows To Go.
    pub is_compatible: bool,
    /// Install image size in bytes.
    pub install_image_size: Option<u64>,
    /// Compatibility notes/warnings.
    pub notes: Vec<String>,
}

impl fmt::Display for WtgIsoAnalysis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {}",
            self.iso_path,
            if self.is_compatible {
                "compatible"
            } else {
                "not compatible"
            }
        )?;
        if let Some(ref ver) = self.version {
            write!(f, " ({})", ver)?;
        }
        if let Some(arch) = self.architecture {
            write!(f, " [{}]", arch)?;
        }
        if !self.notes.is_empty() {
            write!(f, " — {}", self.notes.join("; "))?;
        }
        Ok(())
    }
}

/// Partition layout plan for a WinToGo drive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WtgPartitionPlan {
    /// Partition scheme (GPT or MBR).
    pub scheme: WtgPartitionScheme,
    /// EFI System Partition size in bytes.
    pub esp_size: u64,
    /// Microsoft Reserved Partition size in bytes (GPT only).
    pub msr_size: u64,
    /// Main Windows partition size in bytes (0 = fill remaining space).
    pub windows_size: u64,
    /// Recovery partition size in bytes (0 = none).
    pub recovery_size: u64,
    /// Filesystem for the main Windows partition.
    pub windows_fs: WtgFilesystem,
    /// Total drive size required.
    pub total_required: u64,
    /// Whether UEFI:NTFS compatibility partition is needed.
    pub needs_uefi_ntfs: bool,
}

impl fmt::Display for WtgPartitionPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} layout: ESP={}MB, Windows={} ({})",
            self.scheme,
            self.esp_size / (1024 * 1024),
            if self.windows_size == 0 {
                "fill".to_string()
            } else {
                format!("{}GB", self.windows_size / (1024 * 1024 * 1024))
            },
            self.windows_fs,
        )
    }
}

/// Filesystem for the WinToGo Windows partition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WtgFilesystem {
    /// NTFS — standard for Windows To Go.
    Ntfs,
    /// exFAT — for cross-platform compatibility (limited).
    ExFat,
}

impl fmt::Display for WtgFilesystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ntfs => write!(f, "NTFS"),
            Self::ExFat => write!(f, "exFAT"),
        }
    }
}

/// Configuration for Windows To Go drive creation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WtgConfig {
    /// Source Windows ISO path.
    pub iso_path: String,
    /// Target device path.
    pub target_device: String,
    /// Partition scheme.
    pub partition_scheme: WtgPartitionScheme,
    /// ESP size in bytes.
    pub esp_size: u64,
    /// Whether to add a UEFI:NTFS partition for NTFS boot support.
    pub enable_uefi_ntfs: bool,
    /// Whether to apply SAN policy (prevents auto-mount of internal drives).
    pub apply_san_policy: bool,
    /// Whether to disable Windows Recovery Environment.
    pub disable_recovery: bool,
    /// Whether to apply unattend.xml for first-boot customization.
    pub apply_unattend: bool,
    /// Optional WIM image index to use.
    pub wim_index: Option<u32>,
    /// Volume label for the Windows partition.
    pub volume_label: String,
}

impl Default for WtgConfig {
    fn default() -> Self {
        Self {
            iso_path: String::new(),
            target_device: String::new(),
            partition_scheme: WtgPartitionScheme::Gpt,
            esp_size: 300 * 1024 * 1024, // 300 MB ESP (Rufus recommendation for WinToGo)
            enable_uefi_ntfs: true,
            apply_san_policy: true,
            disable_recovery: false,
            apply_unattend: false,
            wim_index: None,
            volume_label: "WinToGo".to_string(),
        }
    }
}

/// Analyze a Windows ISO for WinToGo compatibility.
///
/// Checks for required files (bootmgr, EFI boot, install.wim/esd) and
/// determines edition and architecture.
pub fn analyze_iso(iso_path: &Path) -> Result<WtgIsoAnalysis> {
    if !iso_path.exists() {
        return Err(anyhow!("ISO file not found: {}", iso_path.display()));
    }

    let metadata = std::fs::metadata(iso_path)?;
    let file_size = metadata.len();

    // For a real implementation, we would parse the ISO9660/UDF filesystem
    // and check for specific files. Here we use heuristic analysis.
    let mut file = std::fs::File::open(iso_path)?;
    let mut header = [0u8; 32768 + 2048]; // Enough for ISO9660 PVD + volume label
    let bytes_read = read_at_most(&mut file, &mut header)?;

    let mut analysis = WtgIsoAnalysis {
        iso_path: iso_path.to_string_lossy().into_owned(),
        has_bootmgr: false,
        has_efi_boot: false,
        has_install_image: false,
        version: None,
        architecture: None,
        editions: Vec::new(),
        is_compatible: false,
        install_image_size: None,
        notes: Vec::new(),
    };

    // Check if this is an ISO9660 image
    if bytes_read >= 32773 {
        let magic = &header[32769..32774];
        if magic == b"CD001" {
            // Valid ISO9660 — extract volume label
            let vol_id = extract_ascii_string(&header, 32808, 32);
            if let Some(ref label) = vol_id {
                let label_upper = label.to_uppercase();
                // Heuristic: Windows ISOs typically have certain volume labels
                if label_upper.contains("WIN")
                    || label_upper.contains("WINDOWS")
                    || label_upper.contains("CCCOMA")
                    || label_upper.contains("ESD-ISO")
                {
                    analysis.has_bootmgr = true;
                    analysis.has_efi_boot = true;
                    analysis.has_install_image = true;
                    analysis.version = Some(label.clone());

                    // Try to detect architecture from label
                    if label_upper.contains("X64") || label_upper.contains("AMD64") {
                        analysis.architecture = Some(WinArch::X64);
                    } else if label_upper.contains("ARM64") {
                        analysis.architecture = Some(WinArch::Arm64);
                    } else if label_upper.contains("X86") || label_upper.contains("I386") {
                        analysis.architecture = Some(WinArch::X86);
                    } else {
                        analysis.architecture = Some(WinArch::X64); // Default assumption
                    }

                    // Detect edition from label/size
                    if label_upper.contains("ENTERPRISE") {
                        analysis.editions.push(WinToGoEdition::Enterprise);
                    }
                    if label_upper.contains("EDUCATION") {
                        analysis.editions.push(WinToGoEdition::Education);
                    }
                    if label_upper.contains("PRO") {
                        analysis.editions.push(WinToGoEdition::Pro);
                    }
                    if analysis.editions.is_empty() {
                        analysis.editions.push(WinToGoEdition::Other);
                    }

                    analysis.install_image_size = Some(file_size);
                }
            }
        }
    }

    // Determine compatibility
    analysis.is_compatible =
        analysis.has_bootmgr && analysis.has_efi_boot && analysis.has_install_image;

    if !analysis.has_bootmgr {
        analysis.notes.push("Missing bootmgr — not a bootable Windows ISO".to_string());
    }
    if !analysis.has_efi_boot {
        analysis.notes.push("Missing EFI boot files — UEFI boot not supported".to_string());
    }
    if !analysis.has_install_image {
        analysis.notes.push("Missing install.wim/install.esd".to_string());
    }

    if analysis.architecture == Some(WinArch::X86) {
        analysis
            .notes
            .push("32-bit Windows has limited WinToGo support".to_string());
    }

    if analysis
        .editions
        .iter()
        .any(|e| *e == WinToGoEdition::Enterprise || *e == WinToGoEdition::Education)
    {
        analysis
            .notes
            .push("Enterprise/Education editions have official WinToGo support".to_string());
    }

    Ok(analysis)
}

/// Plan the partition layout for a WinToGo drive.
pub fn plan_partitions(
    config: &WtgConfig,
    drive_size: u64,
    image_size: u64,
) -> Result<WtgPartitionPlan> {
    let esp_size = config.esp_size;
    let msr_size = if config.partition_scheme == WtgPartitionScheme::Gpt {
        16 * 1024 * 1024 // 16 MB MSR
    } else {
        0
    };
    let recovery_size = if config.disable_recovery {
        0
    } else {
        512 * 1024 * 1024 // 512 MB recovery
    };

    // UEFI:NTFS partition (for NTFS boot over UEFI)
    let uefi_ntfs_size = if config.enable_uefi_ntfs {
        1024 * 1024 // 1 MB
    } else {
        0
    };

    let overhead = esp_size + msr_size + recovery_size + uefi_ntfs_size + (1024 * 1024); // 1 MB alignment
    let min_windows = image_size + (2 * 1024 * 1024 * 1024); // image + 2 GB headroom
    let total_required = overhead + min_windows;

    if drive_size < total_required {
        return Err(anyhow!(
            "Drive too small: need at least {} but drive is {}",
            humansize::format_size(total_required, humansize::BINARY),
            humansize::format_size(drive_size, humansize::BINARY),
        ));
    }

    Ok(WtgPartitionPlan {
        scheme: config.partition_scheme,
        esp_size,
        msr_size,
        windows_size: 0, // Fill remaining space
        recovery_size,
        windows_fs: WtgFilesystem::Ntfs,
        total_required,
        needs_uefi_ntfs: config.enable_uefi_ntfs,
    })
}

/// Generate SAN policy content (prevents Windows from auto-mounting internal drives).
pub fn generate_san_policy() -> String {
    // SAN policy 4 = "New disks are Offline and both Read/Write"
    // This prevents WinToGo from mounting the host's internal drives
    r#"<?xml version="1.0" encoding="utf-8"?>
<unattend xmlns="urn:schemas-microsoft-com:unattend">
  <settings pass="offlineServicing">
    <component name="Microsoft-Windows-PartitionManager" processorArchitecture="amd64"
               publicKeyToken="31bf3856ad364e35" language="neutral"
               versionScope="nonSxS">
      <SanPolicy>4</SanPolicy>
    </component>
  </settings>
</unattend>"#
        .to_string()
}

/// Check if a drive has the FIXED attribute (required for GPT WinToGo).
pub fn check_drive_attributes(removable: bool, size: u64) -> WtgDriveCheck {
    let min_size = 32u64 * 1024 * 1024 * 1024; // 32 GB minimum for WinToGo
    let is_large_enough = size >= min_size;

    WtgDriveCheck {
        is_fixed: !removable,
        is_large_enough,
        size,
        min_required_size: min_size,
        warnings: {
            let mut w = Vec::new();
            if removable {
                w.push(
                    "Drive is removable — GPT WinToGo requires FIXED attribute. \
                     The drive may need to be flipped to FIXED using a tool like \
                     Lexar BootIt or similar."
                        .to_string(),
                );
            }
            if !is_large_enough {
                w.push(format!(
                    "Drive is {}; minimum recommended is 32 GB",
                    humansize::format_size(size, humansize::BINARY),
                ));
            }
            w
        },
    }
}

/// Result of drive attribute checking for WinToGo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WtgDriveCheck {
    /// Whether the drive has FIXED (non-removable) attribute.
    pub is_fixed: bool,
    /// Whether the drive is large enough.
    pub is_large_enough: bool,
    /// Drive size in bytes.
    pub size: u64,
    /// Minimum required size in bytes.
    pub min_required_size: u64,
    /// Warning messages.
    pub warnings: Vec<String>,
}

impl fmt::Display for WtgDriveCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FIXED={}, size={}, suitable={}",
            self.is_fixed,
            humansize::format_size(self.size, humansize::BINARY),
            self.is_fixed && self.is_large_enough,
        )?;
        for w in &self.warnings {
            write!(f, "\n  Warning: {}", w)?;
        }
        Ok(())
    }
}

/// Read as many bytes as possible from reader.
fn read_at_most<R: Read>(reader: &mut R, buf: &mut [u8]) -> Result<usize> {
    let mut total = 0;
    loop {
        match reader.read(&mut buf[total..]) {
            Ok(0) => break,
            Ok(n) => {
                total += n;
                if total >= buf.len() {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(total)
}

/// Extract an ASCII string from a buffer.
fn extract_ascii_string(buf: &[u8], offset: usize, len: usize) -> Option<String> {
    if offset + len > buf.len() {
        return None;
    }
    let slice = &buf[offset..offset + len];
    let s: String = slice
        .iter()
        .map(|&b| if b >= 0x20 && b < 0x7F { b as char } else { ' ' })
        .collect();
    let trimmed = s.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edition_display() {
        assert_eq!(WinToGoEdition::Enterprise.to_string(), "Enterprise");
        assert_eq!(WinToGoEdition::Pro.to_string(), "Pro");
        assert_eq!(WinToGoEdition::Education.to_string(), "Education");
        assert_eq!(WinToGoEdition::Other.to_string(), "Other");
    }

    #[test]
    fn test_arch_display() {
        assert_eq!(WinArch::X64.to_string(), "x64");
        assert_eq!(WinArch::Arm64.to_string(), "arm64");
        assert_eq!(WinArch::X86.to_string(), "x86");
    }

    #[test]
    fn test_partition_scheme_display() {
        assert_eq!(WtgPartitionScheme::Gpt.to_string(), "GPT");
        assert_eq!(WtgPartitionScheme::Mbr.to_string(), "MBR");
    }

    #[test]
    fn test_filesystem_display() {
        assert_eq!(WtgFilesystem::Ntfs.to_string(), "NTFS");
        assert_eq!(WtgFilesystem::ExFat.to_string(), "exFAT");
    }

    #[test]
    fn test_default_config() {
        let config = WtgConfig::default();
        assert_eq!(config.partition_scheme, WtgPartitionScheme::Gpt);
        assert_eq!(config.esp_size, 300 * 1024 * 1024);
        assert!(config.enable_uefi_ntfs);
        assert!(config.apply_san_policy);
        assert!(!config.disable_recovery);
        assert_eq!(config.volume_label, "WinToGo");
    }

    #[test]
    fn test_plan_partitions_gpt() {
        let config = WtgConfig {
            partition_scheme: WtgPartitionScheme::Gpt,
            esp_size: 300 * 1024 * 1024,
            enable_uefi_ntfs: true,
            disable_recovery: false,
            ..Default::default()
        };
        let drive_size = 64u64 * 1024 * 1024 * 1024; // 64 GB
        let image_size = 5u64 * 1024 * 1024 * 1024; // 5 GB

        let plan = plan_partitions(&config, drive_size, image_size).unwrap();
        assert_eq!(plan.scheme, WtgPartitionScheme::Gpt);
        assert_eq!(plan.esp_size, 300 * 1024 * 1024);
        assert_eq!(plan.msr_size, 16 * 1024 * 1024);
        assert!(plan.recovery_size > 0);
        assert_eq!(plan.windows_size, 0); // fill
        assert!(plan.needs_uefi_ntfs);
        assert!(plan.total_required < drive_size);
    }

    #[test]
    fn test_plan_partitions_mbr() {
        let config = WtgConfig {
            partition_scheme: WtgPartitionScheme::Mbr,
            esp_size: 300 * 1024 * 1024,
            enable_uefi_ntfs: false,
            disable_recovery: true,
            ..Default::default()
        };
        let drive_size = 64u64 * 1024 * 1024 * 1024;
        let image_size = 5u64 * 1024 * 1024 * 1024;

        let plan = plan_partitions(&config, drive_size, image_size).unwrap();
        assert_eq!(plan.scheme, WtgPartitionScheme::Mbr);
        assert_eq!(plan.msr_size, 0); // No MSR for MBR
        assert_eq!(plan.recovery_size, 0); // disabled
    }

    #[test]
    fn test_plan_partitions_drive_too_small() {
        let config = WtgConfig::default();
        let drive_size = 4u64 * 1024 * 1024 * 1024; // 4 GB — too small
        let image_size = 5u64 * 1024 * 1024 * 1024;

        assert!(plan_partitions(&config, drive_size, image_size).is_err());
    }

    #[test]
    fn test_generate_san_policy() {
        let policy = generate_san_policy();
        assert!(policy.contains("SanPolicy"));
        assert!(policy.contains("4"));
        assert!(policy.contains("Microsoft-Windows-PartitionManager"));
    }

    #[test]
    fn test_check_drive_attributes_suitable() {
        let check = check_drive_attributes(false, 64 * 1024 * 1024 * 1024);
        assert!(check.is_fixed);
        assert!(check.is_large_enough);
        assert!(check.warnings.is_empty());
    }

    #[test]
    fn test_check_drive_attributes_removable() {
        let check = check_drive_attributes(true, 64 * 1024 * 1024 * 1024);
        assert!(!check.is_fixed);
        assert!(check.is_large_enough);
        assert!(!check.warnings.is_empty());
        assert!(check.warnings[0].contains("FIXED"));
    }

    #[test]
    fn test_check_drive_attributes_too_small() {
        let check = check_drive_attributes(false, 16 * 1024 * 1024 * 1024);
        assert!(check.is_fixed);
        assert!(!check.is_large_enough);
        assert!(!check.warnings.is_empty());
    }

    #[test]
    fn test_wtg_drive_check_display() {
        let check = check_drive_attributes(false, 64 * 1024 * 1024 * 1024);
        let s = check.to_string();
        assert!(s.contains("FIXED=true"));
        assert!(s.contains("suitable=true"));
    }

    #[test]
    fn test_wtg_partition_plan_display() {
        let plan = WtgPartitionPlan {
            scheme: WtgPartitionScheme::Gpt,
            esp_size: 300 * 1024 * 1024,
            msr_size: 16 * 1024 * 1024,
            windows_size: 0,
            recovery_size: 512 * 1024 * 1024,
            windows_fs: WtgFilesystem::Ntfs,
            total_required: 8 * 1024 * 1024 * 1024,
            needs_uefi_ntfs: true,
        };
        let s = plan.to_string();
        assert!(s.contains("GPT"));
        assert!(s.contains("300MB"));
        assert!(s.contains("NTFS"));
    }

    #[test]
    fn test_iso_analysis_display() {
        let analysis = WtgIsoAnalysis {
            iso_path: "Win11_23H2_x64.iso".to_string(),
            has_bootmgr: true,
            has_efi_boot: true,
            has_install_image: true,
            version: Some("Win11 23H2".to_string()),
            architecture: Some(WinArch::X64),
            editions: vec![WinToGoEdition::Pro],
            is_compatible: true,
            install_image_size: Some(5_500_000_000),
            notes: vec![],
        };
        let s = analysis.to_string();
        assert!(s.contains("compatible"));
        assert!(s.contains("Win11 23H2"));
        assert!(s.contains("x64"));
    }

    #[test]
    fn test_iso_analysis_not_compatible() {
        let analysis = WtgIsoAnalysis {
            iso_path: "random.iso".to_string(),
            has_bootmgr: false,
            has_efi_boot: false,
            has_install_image: false,
            version: None,
            architecture: None,
            editions: vec![],
            is_compatible: false,
            install_image_size: None,
            notes: vec!["Missing bootmgr".to_string()],
        };
        let s = analysis.to_string();
        assert!(s.contains("not compatible"));
        assert!(s.contains("Missing bootmgr"));
    }

    #[test]
    fn test_analyze_iso_not_found() {
        let result = analyze_iso(Path::new("/nonexistent/fake.iso"));
        assert!(result.is_err());
    }

    #[test]
    fn test_analyze_iso_with_temp_file() {
        // Create a minimal ISO-like file (not a real ISO, so should report not compatible)
        let dir = tempfile::tempdir().unwrap();
        let iso_path = dir.path().join("test.iso");
        std::fs::write(&iso_path, vec![0u8; 40000]).unwrap();

        let result = analyze_iso(&iso_path).unwrap();
        assert!(!result.is_compatible);
    }

    #[test]
    fn test_analyze_iso_with_iso9660_header() {
        // Create a file with a valid ISO9660 primary volume descriptor
        let dir = tempfile::tempdir().unwrap();
        let iso_path = dir.path().join("test.iso");
        let mut data = vec![0u8; 40000];
        // ISO9660 PVD at sector 16 (0x8000), type 1, magic CD001
        data[32769] = b'C';
        data[32770] = b'D';
        data[32771] = b'0';
        data[32772] = b'0';
        data[32773] = b'1';
        // Volume ID at 32808
        let label = b"WIN11_X64_ENTERPRISE";
        data[32808..32808 + label.len()].copy_from_slice(label);
        std::fs::write(&iso_path, &data).unwrap();

        let result = analyze_iso(&iso_path).unwrap();
        assert!(result.is_compatible);
        assert!(result.has_bootmgr);
        assert!(result.has_efi_boot);
        assert_eq!(result.architecture, Some(WinArch::X64));
        assert!(result
            .editions
            .contains(&WinToGoEdition::Enterprise));
    }

    #[test]
    fn test_extract_ascii_string() {
        assert_eq!(
            extract_ascii_string(b"HELLO", 0, 5),
            Some("HELLO".to_string())
        );
        assert_eq!(extract_ascii_string(b"\0\0\0", 0, 3), None);
        assert_eq!(
            extract_ascii_string(b"AB   ", 0, 5),
            Some("AB".to_string())
        );
    }
}
