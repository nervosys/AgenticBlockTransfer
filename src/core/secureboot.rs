// Secure Boot detection — read UEFI firmware state and boot chain status.
//
// Inspired by Ventoy's secure boot detection and Rufus's UEFI secure boot handling:
//   - Detect whether Secure Boot is enabled in the firmware
//   - Read EFI variables (Linux: /sys/firmware/efi/efivars, Windows: GetFirmwareEnvironmentVariable)
//   - Determine Setup Mode vs User Mode
//   - Check Platform Key (PK), Key Exchange Key (KEK), and db/dbx databases
//   - Advise on boot signing requirements for the target device
//
// This is essential for creating bootable media: if the target system has
// Secure Boot enabled, the bootloader must be signed with a trusted key
// (or the user needs a shim/MOK enrollment process).

#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Secure Boot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecureBootState {
    /// Secure Boot is enabled and enforcing.
    Enabled,
    /// Secure Boot is disabled.
    Disabled,
    /// Firmware is in Setup Mode (keys can be enrolled).
    SetupMode,
    /// Cannot determine (legacy BIOS or no access).
    Unknown,
}

impl std::fmt::Display for SecureBootState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SecureBootState::Enabled => write!(f, "Enabled"),
            SecureBootState::Disabled => write!(f, "Disabled"),
            SecureBootState::SetupMode => write!(f, "SetupMode"),
            SecureBootState::Unknown => write!(f, "Unknown"),
        }
    }
}

/// UEFI firmware mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FirmwareMode {
    /// UEFI firmware with EFI variables accessible.
    Uefi,
    /// Legacy BIOS (no EFI variables).
    LegacyBios,
    /// Cannot determine.
    Unknown,
}

impl std::fmt::Display for FirmwareMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FirmwareMode::Uefi => write!(f, "UEFI"),
            FirmwareMode::LegacyBios => write!(f, "Legacy BIOS"),
            FirmwareMode::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Key database type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeyDatabase {
    /// Platform Key (root of trust).
    PK,
    /// Key Exchange Key.
    KEK,
    /// Authorized Signature Database.
    Db,
    /// Forbidden Signature Database.
    Dbx,
    /// Machine Owner Key (MOK, shim-managed).
    MOK,
}

impl std::fmt::Display for KeyDatabase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyDatabase::PK => write!(f, "PK"),
            KeyDatabase::KEK => write!(f, "KEK"),
            KeyDatabase::Db => write!(f, "db"),
            KeyDatabase::Dbx => write!(f, "dbx"),
            KeyDatabase::MOK => write!(f, "MOK"),
        }
    }
}

/// Information about a key in a UEFI key database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEntry {
    /// Key database this entry belongs to.
    pub database: KeyDatabase,
    /// Certificate subject / owner.
    pub subject: String,
    /// Issuer of the certificate.
    pub issuer: String,
    /// Certificate fingerprint (SHA-256 hex).
    pub fingerprint: String,
    /// Not valid before (RFC 3339).
    pub not_before: Option<String>,
    /// Not valid after (RFC 3339).
    pub not_after: Option<String>,
}

/// Complete Secure Boot status report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecureBootReport {
    /// Overall firmware mode.
    pub firmware_mode: FirmwareMode,
    /// Secure Boot state.
    pub secure_boot: SecureBootState,
    /// Whether Setup Mode is active (keys can be modified).
    pub setup_mode: bool,
    /// Whether the system booted via UEFI.
    pub uefi_boot: bool,
    /// Platform Key present.
    pub has_pk: bool,
    /// Number of KEK entries.
    pub kek_count: u32,
    /// Number of db entries.
    pub db_count: u32,
    /// Number of dbx entries.
    pub dbx_count: u32,
    /// Key entries (if enumerated).
    pub keys: Vec<KeyEntry>,
    /// Boot chain advice.
    pub advice: Vec<String>,
    /// Platform details.
    pub platform: String,
}

/// EFI variable GUID for Secure Boot variables.
pub const EFI_GLOBAL_VARIABLE_GUID: &str = "8be4df61-93ca-11d2-aa0d-00e098032b8c";

/// EFI variable path prefix on Linux.
#[cfg(target_os = "linux")]
pub const EFI_VARS_PATH: &str = "/sys/firmware/efi/efivars";

/// Detect the firmware mode of the current system.
pub fn detect_firmware_mode() -> FirmwareMode {
    #[cfg(target_os = "linux")]
    {
        if Path::new("/sys/firmware/efi").exists() {
            return FirmwareMode::Uefi;
        }
        return FirmwareMode::LegacyBios;
    }

    #[cfg(target_os = "windows")]
    {
        // On Windows, we can check if GetFirmwareType returns UefiFirmwareType.
        // For now, assume UEFI if the system is modern.
        return FirmwareMode::Uefi;
    }

    #[cfg(target_os = "macos")]
    {
        // All modern Macs use EFI.
        return FirmwareMode::Uefi;
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        return FirmwareMode::Unknown;
    }
}

/// Read a UEFI EFI variable as raw bytes.
#[cfg(target_os = "linux")]
pub fn read_efi_variable(name: &str, guid: &str) -> Result<Vec<u8>> {
    let path = format!("{}/{}-{}", EFI_VARS_PATH, name, guid);
    let data = std::fs::read(&path)
        .with_context(|| format!("Cannot read EFI variable: {}", path))?;

    // EFI variable files have a 4-byte attributes prefix.
    if data.len() < 4 {
        bail!("EFI variable too short: {}", path);
    }

    // Skip the 4-byte attributes header.
    Ok(data[4..].to_vec())
}

/// Read a UEFI EFI variable as raw bytes (Windows stub).
#[cfg(target_os = "windows")]
pub fn read_efi_variable(name: &str, guid: &str) -> Result<Vec<u8>> {
    debug!("Reading EFI variable {} (GUID: {})", name, guid);
    // Windows would use GetFirmwareEnvironmentVariable API.
    // Requires SE_SYSTEM_ENVIRONMENT_NAME privilege.
    bail!("EFI variable reading requires elevated privileges on Windows")
}

/// Read a UEFI EFI variable (unsupported platform stub).
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
pub fn read_efi_variable(name: &str, guid: &str) -> Result<Vec<u8>> {
    bail!("EFI variable reading not supported on this platform")
}

/// Check if Secure Boot is enabled.
pub fn detect_secure_boot() -> SecureBootState {
    match read_efi_variable("SecureBoot", EFI_GLOBAL_VARIABLE_GUID) {
        Ok(data) => {
            if data.is_empty() {
                SecureBootState::Unknown
            } else if data[0] == 1 {
                SecureBootState::Enabled
            } else {
                SecureBootState::Disabled
            }
        }
        Err(_) => SecureBootState::Unknown,
    }
}

/// Check if the system is in Setup Mode.
pub fn detect_setup_mode() -> bool {
    match read_efi_variable("SetupMode", EFI_GLOBAL_VARIABLE_GUID) {
        Ok(data) => !data.is_empty() && data[0] == 1,
        Err(_) => false,
    }
}

/// Check if a Platform Key is enrolled.
pub fn has_platform_key() -> bool {
    read_efi_variable("PK", EFI_GLOBAL_VARIABLE_GUID).is_ok()
}

/// Generate a comprehensive Secure Boot status report.
pub fn generate_report() -> SecureBootReport {
    let firmware_mode = detect_firmware_mode();
    let secure_boot = detect_secure_boot();
    let setup_mode = detect_setup_mode();
    let has_pk = has_platform_key();

    let uefi_boot = firmware_mode == FirmwareMode::Uefi;

    let mut advice = Vec::new();

    match secure_boot {
        SecureBootState::Enabled => {
            advice.push("Secure Boot is ENABLED — bootable media must use a signed bootloader".into());
            advice.push("Use a shim-signed GRUB2, or the Microsoft-signed UEFI bootloader".into());
            advice.push("Alternatively, enroll the distro's MOK key at boot time".into());
        }
        SecureBootState::Disabled => {
            advice.push("Secure Boot is DISABLED — any bootloader will work".into());
            advice.push("Consider enabling Secure Boot for enhanced security".into());
        }
        SecureBootState::SetupMode => {
            advice.push("Firmware is in SETUP MODE — custom keys can be enrolled".into());
            advice.push("Enroll your own PK and KEK for full Secure Boot control".into());
        }
        SecureBootState::Unknown => {
            advice.push("Cannot determine Secure Boot state".into());
            if firmware_mode == FirmwareMode::LegacyBios {
                advice.push("System appears to use Legacy BIOS (no Secure Boot support)".into());
            } else {
                advice.push("Try running with elevated privileges to access EFI variables".into());
            }
        }
    }

    if setup_mode {
        advice.push("Setup Mode is active — PK can be updated without authentication".into());
    }

    let platform = format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH);

    SecureBootReport {
        firmware_mode,
        secure_boot,
        setup_mode,
        uefi_boot,
        has_pk,
        kek_count: 0, // Would be populated by parsing KEK variable
        db_count: 0,
        dbx_count: 0,
        keys: Vec::new(),
        advice,
        platform,
    }
}

/// Format a Secure Boot report as human-readable text.
pub fn format_report(report: &SecureBootReport) -> String {
    let mut lines = Vec::new();
    lines.push("Secure Boot Report".into());
    lines.push(format!("  Firmware: {}", report.firmware_mode));
    lines.push(format!("  Secure Boot: {}", report.secure_boot));
    lines.push(format!("  Setup Mode: {}", if report.setup_mode { "Yes" } else { "No" }));
    lines.push(format!("  UEFI Boot: {}", if report.uefi_boot { "Yes" } else { "No" }));
    lines.push(format!("  Platform Key: {}", if report.has_pk { "Present" } else { "Not enrolled" }));
    lines.push(format!("  KEK entries: {}", report.kek_count));
    lines.push(format!("  db entries: {}", report.db_count));
    lines.push(format!("  dbx entries: {}", report.dbx_count));
    lines.push(format!("  Platform: {}", report.platform));

    if !report.advice.is_empty() {
        lines.push(String::new());
        lines.push("Advice:".into());
        for a in &report.advice {
            lines.push(format!("  • {}", a));
        }
    }

    if !report.keys.is_empty() {
        lines.push(String::new());
        lines.push("Enrolled Keys:".into());
        for key in &report.keys {
            lines.push(format!("  [{}] {} (issuer: {})", key.database, key.subject, key.issuer));
        }
    }

    lines.join("\n")
}

/// Determine if a bootloader file is likely signed.
pub fn is_likely_signed(path: &Path) -> Result<bool> {
    // PE/COFF executables (.efi) that are signed have a non-zero
    // "Certificate Table" entry in the optional header data directories.
    let mut file = std::fs::File::open(path)?;
    let mut dos_header = [0u8; 64];
    match std::io::Read::read_exact(&mut file, &mut dos_header) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(false),
        Err(e) => return Err(e.into()),
    }

    // Check MZ magic
    if dos_header[0] != b'M' || dos_header[1] != b'Z' {
        return Ok(false);
    }

    // PE signature offset is at offset 0x3C (4 bytes, little-endian)
    let pe_offset = u32::from_le_bytes([
        dos_header[0x3C],
        dos_header[0x3D],
        dos_header[0x3E],
        dos_header[0x3F],
    ]) as u64;

    // Seek to PE signature
    std::io::Seek::seek(&mut file, SeekFrom::Start(pe_offset))?;
    let mut pe_sig = [0u8; 4];
    std::io::Read::read_exact(&mut file, &mut pe_sig)?;

    // Check PE\0\0 signature
    if &pe_sig != b"PE\0\0" {
        return Ok(false);
    }

    // COFF header is 20 bytes; Optional header follows.
    // We need to find the Certificate Table data directory.
    // For PE32+, it's at optional header offset 144 (0x90).
    // For PE32, it's at optional header offset 128 (0x80).

    // Read COFF header to get SizeOfOptionalHeader
    let mut coff = [0u8; 20];
    std::io::Read::read_exact(&mut file, &mut coff)?;
    let optional_header_size = u16::from_le_bytes([coff[16], coff[17]]);

    if optional_header_size < 128 {
        return Ok(false);
    }

    // Read optional header magic (2 bytes)
    let mut opt_magic = [0u8; 2];
    std::io::Read::read_exact(&mut file, &mut opt_magic)?;
    let magic = u16::from_le_bytes(opt_magic);

    let cert_table_offset = match magic {
        0x10b => 128 - 2, // PE32: Certificate Table at offset 128 from start of optional header
        0x20b => 144 - 2, // PE32+: Certificate Table at offset 144
        _ => return Ok(false),
    };

    // Seek to Certificate Table RVA+Size (2 * u32 = 8 bytes)
    std::io::Seek::seek(
        &mut file,
        SeekFrom::Start(pe_offset + 24 + 2 + cert_table_offset as u64),
    )?;
    let mut cert_entry = [0u8; 8];
    std::io::Read::read_exact(&mut file, &mut cert_entry)?;

    let cert_rva = u32::from_le_bytes([cert_entry[0], cert_entry[1], cert_entry[2], cert_entry[3]]);
    let cert_size = u32::from_le_bytes([cert_entry[4], cert_entry[5], cert_entry[6], cert_entry[7]]);

    Ok(cert_rva != 0 && cert_size != 0)
}

/// Common Secure Boot signed bootloader filenames.
pub fn known_signed_bootloaders() -> Vec<&'static str> {
    vec![
        "shimx64.efi",
        "shimia32.efi",
        "shimaa64.efi",
        "grubx64.efi",
        "grubia32.efi",
        "grubaa64.efi",
        "bootx64.efi",
        "bootia32.efi",
        "bootaa64.efi",
        "mmx64.efi",     // MOK Manager
        "fwupx64.efi",   // firmware update
    ]
}

/// Check if a filename looks like a Secure Boot shim.
pub fn is_shim_bootloader(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    lower.starts_with("shim") && lower.ends_with(".efi")
}

/// Check if a filename looks like a MOK Manager.
pub fn is_mok_manager(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    (lower.starts_with("mm") || lower.contains("mokmanager")) && lower.ends_with(".efi")
}

use std::io::SeekFrom;

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_boot_state_display() {
        assert_eq!(SecureBootState::Enabled.to_string(), "Enabled");
        assert_eq!(SecureBootState::Disabled.to_string(), "Disabled");
        assert_eq!(SecureBootState::SetupMode.to_string(), "SetupMode");
        assert_eq!(SecureBootState::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_firmware_mode_display() {
        assert_eq!(FirmwareMode::Uefi.to_string(), "UEFI");
        assert_eq!(FirmwareMode::LegacyBios.to_string(), "Legacy BIOS");
        assert_eq!(FirmwareMode::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_key_database_display() {
        assert_eq!(KeyDatabase::PK.to_string(), "PK");
        assert_eq!(KeyDatabase::KEK.to_string(), "KEK");
        assert_eq!(KeyDatabase::Db.to_string(), "db");
        assert_eq!(KeyDatabase::Dbx.to_string(), "dbx");
        assert_eq!(KeyDatabase::MOK.to_string(), "MOK");
    }

    #[test]
    fn test_detect_firmware_mode() {
        let mode = detect_firmware_mode();
        // Should return a valid mode on any platform
        assert!(matches!(
            mode,
            FirmwareMode::Uefi | FirmwareMode::LegacyBios | FirmwareMode::Unknown
        ));
    }

    #[test]
    fn test_detect_secure_boot() {
        let state = detect_secure_boot();
        // On most dev machines, this will be Unknown (no privileges) or one of the states
        assert!(matches!(
            state,
            SecureBootState::Enabled
                | SecureBootState::Disabled
                | SecureBootState::SetupMode
                | SecureBootState::Unknown
        ));
    }

    #[test]
    fn test_generate_report() {
        let report = generate_report();
        assert!(!report.platform.is_empty());
        assert!(!report.advice.is_empty());
        // firmware_mode should be valid
        assert!(matches!(
            report.firmware_mode,
            FirmwareMode::Uefi | FirmwareMode::LegacyBios | FirmwareMode::Unknown
        ));
    }

    #[test]
    fn test_format_report() {
        let report = generate_report();
        let text = format_report(&report);
        assert!(text.contains("Secure Boot Report"));
        assert!(text.contains("Firmware:"));
        assert!(text.contains("Advice:"));
    }

    #[test]
    fn test_report_serialization() {
        let report = generate_report();
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"firmware_mode\""));
        assert!(json.contains("\"secure_boot\""));
        let back: SecureBootReport = serde_json::from_str(&json).unwrap();
        assert_eq!(back.platform, report.platform);
    }

    #[test]
    fn test_known_signed_bootloaders() {
        let loaders = known_signed_bootloaders();
        assert!(!loaders.is_empty());
        assert!(loaders.contains(&"shimx64.efi"));
        assert!(loaders.contains(&"bootx64.efi"));
    }

    #[test]
    fn test_is_shim_bootloader() {
        assert!(is_shim_bootloader("shimx64.efi"));
        assert!(is_shim_bootloader("SHIMIA32.EFI"));
        assert!(is_shim_bootloader("shimaa64.efi"));
        assert!(!is_shim_bootloader("grubx64.efi"));
        assert!(!is_shim_bootloader("shim.txt"));
    }

    #[test]
    fn test_is_mok_manager() {
        assert!(is_mok_manager("mmx64.efi"));
        assert!(is_mok_manager("MokManager.efi"));
        assert!(is_mok_manager("MMIA32.EFI"));
        assert!(!is_mok_manager("grubx64.efi"));
        assert!(!is_mok_manager("mm.txt"));
    }

    #[test]
    fn test_key_entry_fields() {
        let key = KeyEntry {
            database: KeyDatabase::Db,
            subject: "Microsoft Windows Production PCA 2011".into(),
            issuer: "Microsoft Root Certificate Authority 2010".into(),
            fingerprint: "abc123def456".into(),
            not_before: Some("2011-10-19T00:00:00Z".into()),
            not_after: Some("2026-10-19T00:00:00Z".into()),
        };
        assert_eq!(key.database, KeyDatabase::Db);
        assert!(key.subject.contains("Microsoft"));
    }

    #[test]
    fn test_is_likely_signed_not_pe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.efi");
        std::fs::write(&path, b"not a PE file at all!").unwrap();
        assert_eq!(is_likely_signed(&path).unwrap(), false);
    }

    #[test]
    fn test_is_likely_signed_mz_but_bad_pe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.efi");
        let mut data = vec![0u8; 256];
        data[0] = b'M';
        data[1] = b'Z';
        data[0x3C] = 0x80; // PE offset points to 0x80
        // No PE signature at 0x80
        std::fs::write(&path, &data).unwrap();
        assert_eq!(is_likely_signed(&path).unwrap(), false);
    }

    #[test]
    fn test_is_likely_signed_valid_pe_unsigned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unsigned.efi");
        let mut data = vec![0u8; 512];
        data[0] = b'M';
        data[1] = b'Z';
        data[0x3C] = 0x80; // PE offset
        // PE signature at 0x80
        data[0x80] = b'P';
        data[0x81] = b'E';
        data[0x82] = 0;
        data[0x83] = 0;
        // COFF header: SizeOfOptionalHeader at offset 0x80+4+16 = 0x94
        data[0x94] = 0xF0; // large enough optional header (240 bytes)
        data[0x95] = 0x00;
        // Optional header magic at 0x80+4+20 = 0x98 → PE32+ (0x20b)
        data[0x98] = 0x0b;
        data[0x99] = 0x02;
        // Certificate table at optional header offset 144: 0x98+144 = 0x128
        // All zeros = not signed
        std::fs::write(&path, &data).unwrap();
        assert_eq!(is_likely_signed(&path).unwrap(), false);
    }

    #[test]
    fn test_report_advice_secure_boot_enabled() {
        let report = SecureBootReport {
            firmware_mode: FirmwareMode::Uefi,
            secure_boot: SecureBootState::Enabled,
            setup_mode: false,
            uefi_boot: true,
            has_pk: true,
            kek_count: 2,
            db_count: 5,
            dbx_count: 100,
            keys: vec![],
            advice: vec!["Secure Boot is ENABLED — bootable media must use a signed bootloader".into()],
            platform: "windows/x86_64".into(),
        };
        let text = format_report(&report);
        assert!(text.contains("Enabled"));
        assert!(text.contains("signed"));
    }

    #[test]
    fn test_report_with_keys() {
        let report = SecureBootReport {
            firmware_mode: FirmwareMode::Uefi,
            secure_boot: SecureBootState::Enabled,
            setup_mode: false,
            uefi_boot: true,
            has_pk: true,
            kek_count: 1,
            db_count: 1,
            dbx_count: 0,
            keys: vec![
                KeyEntry {
                    database: KeyDatabase::PK,
                    subject: "Test PK".into(),
                    issuer: "Self".into(),
                    fingerprint: "aabbcc".into(),
                    not_before: None,
                    not_after: None,
                },
            ],
            advice: vec![],
            platform: "linux/x86_64".into(),
        };
        let text = format_report(&report);
        assert!(text.contains("Enrolled Keys"));
        assert!(text.contains("Test PK"));
    }

    #[test]
    fn test_efi_global_variable_guid() {
        assert_eq!(EFI_GLOBAL_VARIABLE_GUID, "8be4df61-93ca-11d2-aa0d-00e098032b8c");
    }
}
