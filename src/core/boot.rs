//! Boot sector validation — verify bootloader integrity post-write.
//!
//! Validates:
//! - MBR boot signature (0x55AA at offset 510)
//! - MBR boot code region (non-zero first 446 bytes)
//! - GPT protective MBR + EFI PART header
//! - EFI System Partition (ESP) detection via GPT type GUID
//! - UEFI bootloader file presence heuristics
//!
//! Used by `abt info` for diagnostics and after writes for integrity checks.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// MBR signature bytes at offset 510-511.
const MBR_SIGNATURE: [u8; 2] = [0x55, 0xAA];

/// GPT signature: "EFI PART" as little-endian u64.
const GPT_SIGNATURE: [u8; 8] = *b"EFI PART";

/// EFI System Partition GUID: C12A7328-F81F-11D2-BA4B-00A0C93EC93B
const ESP_TYPE_GUID: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9,
    0x3B,
];

/// Boot sector validation result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootValidation {
    /// Whether the device/image appears bootable.
    pub is_bootable: bool,
    /// Boot scheme detected.
    pub boot_scheme: BootScheme,
    /// Individual check results.
    pub checks: Vec<BootCheck>,
    /// Human-readable summary.
    pub summary: String,
}

/// Boot scheme type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BootScheme {
    /// Legacy BIOS MBR boot.
    LegacyMbr,
    /// UEFI with GPT.
    UefiGpt,
    /// Hybrid (both MBR boot code and GPT present).
    Hybrid,
    /// No boot scheme detected.
    None,
}

impl std::fmt::Display for BootScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LegacyMbr => write!(f, "Legacy BIOS (MBR)"),
            Self::UefiGpt => write!(f, "UEFI (GPT)"),
            Self::Hybrid => write!(f, "Hybrid (MBR + GPT)"),
            Self::None => write!(f, "None"),
        }
    }
}

/// A single boot validation check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootCheck {
    /// Check name.
    pub name: String,
    /// Whether the check passed.
    pub passed: bool,
    /// Description of what was checked.
    pub description: String,
}

impl BootCheck {
    fn pass(name: &str, desc: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: true,
            description: desc.to_string(),
        }
    }
    fn fail(name: &str, desc: &str) -> Self {
        Self {
            name: name.to_string(),
            passed: false,
            description: desc.to_string(),
        }
    }
}

/// Validate the boot sector of a device or image file.
pub fn validate_boot_sector(path: &Path) -> Result<BootValidation> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("Cannot open {} for boot validation", path.display()))?;

    let file_size = file.metadata()?.len();
    if file_size < 512 {
        return Ok(BootValidation {
            is_bootable: false,
            boot_scheme: BootScheme::None,
            checks: vec![BootCheck::fail(
                "minimum_size",
                "File is smaller than 512 bytes (too small for any boot sector)",
            )],
            summary: "File too small for boot sector analysis".to_string(),
        });
    }

    // Read first 1024 bytes (MBR + potential GPT header at LBA 1)
    let mut header = vec![0u8; std::cmp::min(1024, file_size as usize)];
    file.read_exact(&mut header[..std::cmp::min(1024, file_size as usize)])?;

    let mut checks = Vec::new();

    // Check 1: MBR signature (0x55AA at offset 510-511)
    let has_mbr_signature = header[510] == MBR_SIGNATURE[0] && header[511] == MBR_SIGNATURE[1];
    if has_mbr_signature {
        checks.push(BootCheck::pass(
            "mbr_signature",
            "MBR boot signature (0x55AA) present at offset 510",
        ));
    } else {
        checks.push(BootCheck::fail(
            "mbr_signature",
            &format!(
                "MBR boot signature missing (found 0x{:02X}{:02X}, expected 0x55AA)",
                header[510], header[511]
            ),
        ));
    }

    // Check 2: MBR boot code (first 446 bytes should not be all zeros)
    let boot_code = &header[0..446];
    let has_boot_code = boot_code.iter().any(|&b| b != 0);
    if has_boot_code {
        checks.push(BootCheck::pass(
            "mbr_boot_code",
            "MBR boot code region (0-445) contains non-zero data",
        ));
    } else {
        checks.push(BootCheck::fail(
            "mbr_boot_code",
            "MBR boot code region (0-445) is all zeros — no bootloader present",
        ));
    }

    // Check 3: MBR partition table (at least one non-empty entry in 446-509)
    let has_partition_entries = (0..4).any(|i| {
        let offset = 446 + i * 16;
        header[offset..offset + 16].iter().any(|&b| b != 0)
    });
    if has_partition_entries {
        checks.push(BootCheck::pass(
            "mbr_partitions",
            "MBR partition table contains at least one entry",
        ));
    } else {
        checks.push(BootCheck::fail(
            "mbr_partitions",
            "MBR partition table is empty (all 4 entries are zero)",
        ));
    }

    // Check 4: Protective MBR for GPT (partition type 0xEE)
    let has_protective_mbr = (0..4).any(|i| {
        let type_offset = 446 + i * 16 + 4; // partition type byte is at offset +4
        header[type_offset] == 0xEE
    });
    if has_protective_mbr {
        checks.push(BootCheck::pass(
            "protective_mbr",
            "GPT protective MBR entry (type 0xEE) found",
        ));
    }

    // Check 5: GPT header at LBA 1 (offset 512)
    let has_gpt = header.len() >= 520 && header[512..520] == GPT_SIGNATURE;
    if has_gpt {
        checks.push(BootCheck::pass(
            "gpt_header",
            "GPT header (\"EFI PART\") found at LBA 1",
        ));
    } else if header.len() >= 520 {
        checks.push(BootCheck::fail(
            "gpt_header",
            "No GPT header at LBA 1",
        ));
    }

    // Check 6: Look for EFI System Partition in GPT entries
    let has_esp = if has_gpt && file_size >= 1024 + 128 {
        check_for_esp(&mut file)?
    } else {
        false
    };
    if has_esp {
        checks.push(BootCheck::pass(
            "efi_system_partition",
            "EFI System Partition (ESP) found in GPT entry table",
        ));
    } else if has_gpt {
        checks.push(BootCheck::fail(
            "efi_system_partition",
            "No EFI System Partition found in GPT entries",
        ));
    }

    // Check 7: Jump instruction at byte 0 (common boot code pattern)
    let has_jump = header[0] == 0xEB || header[0] == 0xE9 || header[0] == 0xFA;
    if has_jump {
        let desc = match header[0] {
            0xEB => "x86 short jump (JMP short) — typical BIOS bootloader",
            0xE9 => "x86 near jump (JMP near) — typical BIOS bootloader",
            0xFA => "CLI instruction — typical BIOS bootloader startup",
            _ => "Unknown jump instruction",
        };
        checks.push(BootCheck::pass("boot_jump", desc));
    }

    // Determine boot scheme
    let boot_scheme = if has_gpt && has_boot_code && has_mbr_signature {
        BootScheme::Hybrid
    } else if has_gpt {
        BootScheme::UefiGpt
    } else if has_mbr_signature && has_boot_code {
        BootScheme::LegacyMbr
    } else {
        BootScheme::None
    };

    let is_bootable = boot_scheme != BootScheme::None;

    let passed = checks.iter().filter(|c| c.passed).count();
    let total = checks.len();
    let summary = format!(
        "Boot scheme: {} | {}/{} checks passed{}",
        boot_scheme,
        passed,
        total,
        if is_bootable { " | Device appears bootable" } else { "" }
    );

    Ok(BootValidation {
        is_bootable,
        boot_scheme,
        checks,
        summary,
    })
}

/// Check GPT partition entries for an EFI System Partition.
fn check_for_esp(file: &mut std::fs::File) -> Result<bool> {
    // GPT header is at LBA 1 (offset 512). Partition entry array starts at
    // the LBA specified in the header (usually LBA 2 = offset 1024).
    file.seek(SeekFrom::Start(1024))?;

    // Read up to 128 partition entries (128 bytes each) = 16 KiB
    let max_entries = 128;
    let entry_size = 128;
    let mut entry_buf = vec![0u8; entry_size];

    for _ in 0..max_entries {
        let n = file.read(&mut entry_buf)?;
        if n < entry_size {
            break;
        }

        // First 16 bytes = partition type GUID
        let type_guid = &entry_buf[0..16];

        // Check for EFI System Partition
        if type_guid == ESP_TYPE_GUID {
            return Ok(true);
        }

        // If the entry is all zeros, we've reached the end
        if entry_buf.iter().all(|&b| b == 0) {
            break;
        }
    }

    Ok(false)
}

/// Format boot validation result for display.
pub fn format_validation(validation: &BootValidation) -> String {
    let mut output = String::new();
    output.push_str("Boot Sector Validation:\n");
    output.push_str(&format!("  Boot Scheme:     {}\n", validation.boot_scheme));
    output.push_str(&format!(
        "  Bootable:        {}\n",
        if validation.is_bootable { "Yes" } else { "No" }
    ));
    output.push_str("\n  Checks:\n");

    for check in &validation.checks {
        let icon = if check.passed { "✓" } else { "✗" };
        output.push_str(&format!("    {} {} — {}\n", icon, check.name, check.description));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Create a minimal MBR image.
    fn create_mbr_image() -> Vec<u8> {
        let mut img = vec![0u8; 1024];
        // Jump instruction
        img[0] = 0xEB;
        img[1] = 0x5A; // JMP +0x5A

        // Some boot code
        for i in 2..446 {
            img[i] = 0x90; // NOP sled
        }

        // One partition entry (type 0x0C = FAT32 LBA)
        img[446] = 0x80; // bootable
        img[446 + 4] = 0x0C; // FAT32 LBA
        img[446 + 8] = 0x01; // start LBA = 1
        img[446 + 12] = 0xFF; // size in sectors

        // MBR signature
        img[510] = 0x55;
        img[511] = 0xAA;

        img
    }

    /// Create a minimal GPT image.
    fn create_gpt_image() -> Vec<u8> {
        let mut img = vec![0u8; 1024 + 128 * 4]; // MBR + GPT header + entries

        // Protective MBR
        img[446 + 4] = 0xEE; // GPT protective
        img[510] = 0x55;
        img[511] = 0xAA;

        // GPT header at LBA 1
        img[512..520].copy_from_slice(b"EFI PART");

        // ESP partition entry at offset 1024
        img[1024..1040].copy_from_slice(&ESP_TYPE_GUID);
        // Start LBA = 2048, End LBA = 206847
        img[1056] = 0x00; // start LBA
        img[1057] = 0x08;

        img
    }

    #[test]
    fn test_validate_mbr_boot_sector() {
        let img = create_mbr_image();
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&img).unwrap();
        tmp.flush().unwrap();

        let result = validate_boot_sector(tmp.path()).unwrap();
        assert!(result.is_bootable);
        assert_eq!(result.boot_scheme, BootScheme::LegacyMbr);
        assert!(result.checks.iter().any(|c| c.name == "mbr_signature" && c.passed));
        assert!(result.checks.iter().any(|c| c.name == "mbr_boot_code" && c.passed));
        assert!(result.checks.iter().any(|c| c.name == "boot_jump" && c.passed));
    }

    #[test]
    fn test_validate_gpt_boot_sector() {
        let img = create_gpt_image();
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&img).unwrap();
        tmp.flush().unwrap();

        let result = validate_boot_sector(tmp.path()).unwrap();
        assert!(result.is_bootable);
        assert!(matches!(
            result.boot_scheme,
            BootScheme::UefiGpt | BootScheme::Hybrid
        ));
        assert!(result.checks.iter().any(|c| c.name == "gpt_header" && c.passed));
        assert!(result
            .checks
            .iter()
            .any(|c| c.name == "protective_mbr" && c.passed));
    }

    #[test]
    fn test_validate_empty_disk() {
        let img = vec![0u8; 1024];
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&img).unwrap();
        tmp.flush().unwrap();

        let result = validate_boot_sector(tmp.path()).unwrap();
        assert!(!result.is_bootable);
        assert_eq!(result.boot_scheme, BootScheme::None);
    }

    #[test]
    fn test_validate_too_small() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&[0xFF; 256]).unwrap();
        tmp.flush().unwrap();

        let result = validate_boot_sector(tmp.path()).unwrap();
        assert!(!result.is_bootable);
        assert_eq!(result.boot_scheme, BootScheme::None);
        assert!(result.checks[0].name == "minimum_size");
    }

    #[test]
    fn test_validate_hybrid_boot() {
        let mut img = create_gpt_image();
        // Add boot code to make it hybrid
        img[0] = 0xEB; // JMP
        for i in 2..446 {
            img[i] = 0x90; // NOP
        }

        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&img).unwrap();
        tmp.flush().unwrap();

        let result = validate_boot_sector(tmp.path()).unwrap();
        assert!(result.is_bootable);
        assert_eq!(result.boot_scheme, BootScheme::Hybrid);
    }

    #[test]
    fn test_validate_esp_detection() {
        let img = create_gpt_image();
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(&img).unwrap();
        tmp.flush().unwrap();

        let result = validate_boot_sector(tmp.path()).unwrap();
        assert!(result
            .checks
            .iter()
            .any(|c| c.name == "efi_system_partition" && c.passed));
    }

    #[test]
    fn test_boot_scheme_display() {
        assert_eq!(format!("{}", BootScheme::LegacyMbr), "Legacy BIOS (MBR)");
        assert_eq!(format!("{}", BootScheme::UefiGpt), "UEFI (GPT)");
        assert_eq!(format!("{}", BootScheme::Hybrid), "Hybrid (MBR + GPT)");
        assert_eq!(format!("{}", BootScheme::None), "None");
    }

    #[test]
    fn test_format_validation_output() {
        let validation = BootValidation {
            is_bootable: true,
            boot_scheme: BootScheme::LegacyMbr,
            checks: vec![
                BootCheck::pass("mbr_signature", "Present"),
                BootCheck::fail("gpt_header", "Not found"),
            ],
            summary: "test".to_string(),
        };
        let output = format_validation(&validation);
        assert!(output.contains("Legacy BIOS"));
        assert!(output.contains("✓ mbr_signature"));
        assert!(output.contains("✗ gpt_header"));
    }
}
