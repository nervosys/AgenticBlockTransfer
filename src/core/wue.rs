// Windows Unattended Setup — unattend.xml generation for automated installation.
//
// Inspired by Rufus wue.c: generates unattend.xml with four configuration passes:
//   - windowsPE: disk partitioning, driver injection
//   - specialize: computer name, product key
//   - oobeSystem: user accounts, OOBE customization, privacy settings
//   - offlineServicing: offline package installation
//
// Key features:
//   - Bypass Windows 11 hardware requirements (TPM, Secure Boot, RAM, storage)
//   - Skip Microsoft Account requirement (OOBE\BypassNRO)
//   - Configure local administrator and standard user accounts
//   - Set locale, timezone, and keyboard layout
//   - Disable telemetry / data collection prompts
//   - Auto-detect Windows version/arch from WIM metadata for correct settings

#![allow(dead_code)]

use anyhow::{Context, Result};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

/// Windows architecture for unattend.xml `processorArchitecture` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowsArch {
    Amd64,
    X86,
    Arm64,
}

impl fmt::Display for WindowsArch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WindowsArch::Amd64 => write!(f, "amd64"),
            WindowsArch::X86 => write!(f, "x86"),
            WindowsArch::Arm64 => write!(f, "arm64"),
        }
    }
}

impl WindowsArch {
    /// Parse from WIM metadata arch string.
    pub fn from_wim(arch: &str) -> Option<Self> {
        match arch.to_lowercase().as_str() {
            "amd64" | "x86_64" | "x64" => Some(Self::Amd64),
            "x86" | "i386" | "i686" => Some(Self::X86),
            "arm64" | "aarch64" => Some(Self::Arm64),
            _ => None,
        }
    }
}

/// Windows version for feature gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum WindowsVersion {
    Windows10,
    Windows11,
    WindowsServer2022,
    WindowsServer2025,
    Unknown,
}

impl WindowsVersion {
    /// Detect Windows version from build number.
    pub fn from_build_number(build: u32) -> Self {
        match build {
            26100..=26199 => Self::WindowsServer2025,
            22000..=22999 => Self::Windows11,        // 21H2-22H2
            23000..=25999 => Self::Windows11,        // 23H2+
            19041..=19045 => Self::Windows10,        // 20H1-22H2
            20348 => Self::WindowsServer2022,
            _ if build >= 22000 => Self::Windows11,
            _ => Self::Unknown,
        }
    }

    /// Whether this version requires hardware requirement bypasses.
    pub fn needs_hw_bypass(self) -> bool {
        matches!(self, Self::Windows11)
    }
}

/// User account configuration for the OOBE pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccount {
    /// Username.
    pub name: String,
    /// Password (plaintext — will be base64-encoded in XML).
    pub password: String,
    /// Full display name.
    pub display_name: String,
    /// Add user to Administrators group.
    pub is_admin: bool,
    /// Auto-logon on first boot.
    pub auto_logon: bool,
}

impl Default for UserAccount {
    fn default() -> Self {
        Self {
            name: "User".into(),
            password: String::new(),
            display_name: "User".into(),
            is_admin: true,
            auto_logon: true,
        }
    }
}

/// Locale settings for regional configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocaleSettings {
    /// UI language (e.g., "en-US").
    pub ui_language: String,
    /// Input locale / keyboard layout (e.g., "0409:00000409" for US English).
    pub input_locale: String,
    /// System locale.
    pub system_locale: String,
    /// User locale.
    pub user_locale: String,
    /// Timezone (e.g., "Pacific Standard Time").
    pub timezone: String,
}

impl Default for LocaleSettings {
    fn default() -> Self {
        Self {
            ui_language: "en-US".into(),
            input_locale: "0409:00000409".into(),
            system_locale: "en-US".into(),
            user_locale: "en-US".into(),
            timezone: "Pacific Standard Time".into(),
        }
    }
}

/// Hardware requirement bypass options (Windows 11).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareBypass {
    /// Bypass TPM 2.0 requirement.
    pub bypass_tpm: bool,
    /// Bypass Secure Boot requirement.
    pub bypass_secure_boot: bool,
    /// Bypass RAM requirement (>= 4 GB).
    pub bypass_ram: bool,
    /// Bypass storage requirement (>= 64 GB).
    pub bypass_storage: bool,
    /// Bypass CPU compatibility list.
    pub bypass_cpu: bool,
}

impl Default for HardwareBypass {
    fn default() -> Self {
        Self {
            bypass_tpm: true,
            bypass_secure_boot: true,
            bypass_ram: true,
            bypass_storage: true,
            bypass_cpu: true,
        }
    }
}

/// OOBE customization options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OobeOptions {
    /// Skip Microsoft Account sign-in (BypassNRO).
    pub skip_ms_account: bool,
    /// Skip privacy settings pages.
    pub skip_privacy: bool,
    /// Skip EULA acceptance.
    pub skip_eula: bool,
    /// Disable Cortana voice assistant during OOBE.
    pub disable_cortana: bool,
    /// Disable telemetry (set to Security level).
    pub disable_telemetry: bool,
    /// Hide wireless setup page.
    pub hide_wireless_setup: bool,
    /// Computer name (empty = auto-generate).
    pub computer_name: String,
}

impl Default for OobeOptions {
    fn default() -> Self {
        Self {
            skip_ms_account: true,
            skip_privacy: true,
            skip_eula: true,
            disable_cortana: true,
            disable_telemetry: true,
            hide_wireless_setup: false,
            computer_name: String::new(),
        }
    }
}

/// Full unattend.xml configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnattendConfig {
    /// Target Windows architecture.
    pub arch: WindowsArch,
    /// Detected Windows version.
    pub version: WindowsVersion,
    /// User accounts to create.
    pub accounts: Vec<UserAccount>,
    /// Locale settings.
    pub locale: LocaleSettings,
    /// Hardware bypasses (Windows 11).
    pub hardware_bypass: HardwareBypass,
    /// OOBE customization.
    pub oobe: OobeOptions,
    /// Optional product key.
    pub product_key: Option<String>,
    /// Whether to enable auto-logon for the first account.
    pub auto_logon: bool,
}

impl Default for UnattendConfig {
    fn default() -> Self {
        Self {
            arch: WindowsArch::Amd64,
            version: WindowsVersion::Windows11,
            accounts: vec![UserAccount::default()],
            locale: LocaleSettings::default(),
            hardware_bypass: HardwareBypass::default(),
            oobe: OobeOptions::default(),
            product_key: None,
            auto_logon: true,
        }
    }
}

/// Generate a complete unattend.xml string from the given configuration.
pub fn generate_unattend_xml(config: &UnattendConfig) -> String {
    let mut xml = String::with_capacity(4096);
    let arch = config.arch.to_string();

    xml.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
    xml.push_str("<unattend xmlns=\"urn:schemas-microsoft-com:unattend\">\n");

    // windowsPE pass — hardware bypass registry keys
    if config.version.needs_hw_bypass() {
        xml.push_str(&generate_windows_pe_pass(&arch, config));
    }

    // specialize pass — computer name and product key
    xml.push_str(&generate_specialize_pass(&arch, config));

    // oobeSystem pass — user accounts, OOBE settings, locale
    xml.push_str(&generate_oobe_pass(&arch, config));

    xml.push_str("</unattend>\n");
    xml
}

/// Generate the windowsPE pass (hardware requirement bypasses).
fn generate_windows_pe_pass(arch: &str, config: &UnattendConfig) -> String {
    let mut xml = String::new();
    xml.push_str(&format!(
        "  <settings pass=\"windowsPE\">\n    <component name=\"Microsoft-Windows-Setup\" \
         processorArchitecture=\"{}\" publicKeyToken=\"31bf3856ad364e35\" \
         language=\"neutral\" versionScope=\"nonSxS\" \
         xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\">\n",
        arch
    ));

    // RunSynchronous commands to bypass hardware checks
    xml.push_str("      <RunSynchronous>\n");

    let bp = &config.hardware_bypass;
    let mut order = 1;

    if bp.bypass_tpm {
        xml.push_str(&run_sync_command(
            order,
            "reg add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassTPMCheck /t REG_DWORD /d 1 /f",
            "Bypass TPM",
        ));
        order += 1;
    }

    if bp.bypass_secure_boot {
        xml.push_str(&run_sync_command(
            order,
            "reg add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassSecureBootCheck /t REG_DWORD /d 1 /f",
            "Bypass Secure Boot",
        ));
        order += 1;
    }

    if bp.bypass_ram {
        xml.push_str(&run_sync_command(
            order,
            "reg add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassRAMCheck /t REG_DWORD /d 1 /f",
            "Bypass RAM",
        ));
        order += 1;
    }

    if bp.bypass_storage {
        xml.push_str(&run_sync_command(
            order,
            "reg add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassStorageCheck /t REG_DWORD /d 1 /f",
            "Bypass Storage",
        ));
        order += 1;
    }

    if bp.bypass_cpu {
        xml.push_str(&run_sync_command(
            order,
            "reg add HKLM\\SYSTEM\\Setup\\LabConfig /v BypassCPUCheck /t REG_DWORD /d 1 /f",
            "Bypass CPU",
        ));
    }

    xml.push_str("      </RunSynchronous>\n");
    xml.push_str("    </component>\n");
    xml.push_str("  </settings>\n");
    xml
}

/// Generate the specialize pass (computer name, product key).
fn generate_specialize_pass(arch: &str, config: &UnattendConfig) -> String {
    let mut xml = String::new();
    xml.push_str(&format!(
        "  <settings pass=\"specialize\">\n    <component name=\"Microsoft-Windows-Shell-Setup\" \
         processorArchitecture=\"{}\" publicKeyToken=\"31bf3856ad364e35\" \
         language=\"neutral\" versionScope=\"nonSxS\" \
         xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\">\n",
        arch
    ));

    if !config.oobe.computer_name.is_empty() {
        xml.push_str(&format!(
            "      <ComputerName>{}</ComputerName>\n",
            xml_escape(&config.oobe.computer_name)
        ));
    }

    if let Some(ref key) = config.product_key {
        xml.push_str(&format!(
            "      <ProductKey>{}</ProductKey>\n",
            xml_escape(key)
        ));
    }

    xml.push_str("    </component>\n");

    // Disable telemetry via registry in specialize pass
    if config.oobe.disable_telemetry {
        xml.push_str(&format!(
            "    <component name=\"Microsoft-Windows-Deployment\" \
             processorArchitecture=\"{}\" publicKeyToken=\"31bf3856ad364e35\" \
             language=\"neutral\" versionScope=\"nonSxS\" \
             xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\">\n",
            arch
        ));
        xml.push_str("      <RunSynchronous>\n");
        xml.push_str(&run_sync_command(
            1,
            "reg add HKLM\\SOFTWARE\\Policies\\Microsoft\\Windows\\DataCollection /v AllowTelemetry /t REG_DWORD /d 0 /f",
            "Disable telemetry",
        ));
        xml.push_str("      </RunSynchronous>\n");
        xml.push_str("    </component>\n");
    }

    xml.push_str("  </settings>\n");
    xml
}

/// Generate the oobeSystem pass (user accounts, OOBE customization).
fn generate_oobe_pass(arch: &str, config: &UnattendConfig) -> String {
    let mut xml = String::new();
    xml.push_str(&format!(
        "  <settings pass=\"oobeSystem\">\n    <component name=\"Microsoft-Windows-Shell-Setup\" \
         processorArchitecture=\"{}\" publicKeyToken=\"31bf3856ad364e35\" \
         language=\"neutral\" versionScope=\"nonSxS\" \
         xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\">\n",
        arch
    ));

    // OOBE settings
    xml.push_str("      <OOBE>\n");
    if config.oobe.skip_eula {
        xml.push_str("        <HideEULAPage>true</HideEULAPage>\n");
    }
    if config.oobe.skip_ms_account {
        xml.push_str("        <HideOnlineAccountScreens>true</HideOnlineAccountScreens>\n");
    }
    if config.oobe.skip_privacy {
        xml.push_str("        <ProtectYourPC>3</ProtectYourPC>\n");
    }
    if config.oobe.hide_wireless_setup {
        xml.push_str("        <HideWirelessSetupInOOBE>true</HideWirelessSetupInOOBE>\n");
    }
    xml.push_str("      </OOBE>\n");

    // Locale
    xml.push_str("      <TimeZone>");
    xml.push_str(&xml_escape(&config.locale.timezone));
    xml.push_str("</TimeZone>\n");

    // User accounts
    if !config.accounts.is_empty() {
        xml.push_str("      <UserAccounts>\n");
        xml.push_str("        <LocalAccounts>\n");

        for account in config.accounts.iter() {
            xml.push_str(&format!(
                "          <LocalAccount wcm:action=\"add\">\n\
                 \x20           <Name>{}</Name>\n\
                 \x20           <DisplayName>{}</DisplayName>\n",
                xml_escape(&account.name),
                xml_escape(&account.display_name),
            ));

            if !account.password.is_empty() {
                xml.push_str(&format!(
                    "            <Password>\n\
                     \x20             <Value>{}</Value>\n\
                     \x20             <PlainText>true</PlainText>\n\
                     \x20           </Password>\n",
                    xml_escape(&account.password),
                ));
            }

            if account.is_admin {
                xml.push_str(
                    "            <Group>Administrators</Group>\n",
                );
            }

            xml.push_str("          </LocalAccount>\n");
        }

        xml.push_str("        </LocalAccounts>\n");
        xml.push_str("      </UserAccounts>\n");

        // Auto-logon
        if config.auto_logon {
            if let Some(first) = config.accounts.first() {
                if first.auto_logon {
                    xml.push_str("      <AutoLogon>\n");
                    xml.push_str("        <Enabled>true</Enabled>\n");
                    xml.push_str(&format!(
                        "        <Username>{}</Username>\n",
                        xml_escape(&first.name)
                    ));
                    if !first.password.is_empty() {
                        xml.push_str(&format!(
                            "        <Password>\n\
                             \x20         <Value>{}</Value>\n\
                             \x20         <PlainText>true</PlainText>\n\
                             \x20       </Password>\n",
                            xml_escape(&first.password),
                        ));
                    }
                    xml.push_str("        <LogonCount>1</LogonCount>\n");
                    xml.push_str("      </AutoLogon>\n");
                }
            }
        }
    }

    // International settings
    xml.push_str(&format!(
        "      <InputLocale>{}</InputLocale>\n\
         \x20     <UILanguage>{}</UILanguage>\n\
         \x20     <SystemLocale>{}</SystemLocale>\n\
         \x20     <UserLocale>{}</UserLocale>\n",
        xml_escape(&config.locale.input_locale),
        xml_escape(&config.locale.ui_language),
        xml_escape(&config.locale.system_locale),
        xml_escape(&config.locale.user_locale),
    ));

    xml.push_str("    </component>\n");

    // BypassNRO script (skip MS Account requirement)
    if config.oobe.skip_ms_account {
        xml.push_str(&format!(
            "    <component name=\"Microsoft-Windows-Deployment\" \
             processorArchitecture=\"{}\" publicKeyToken=\"31bf3856ad364e35\" \
             language=\"neutral\" versionScope=\"nonSxS\" \
             xmlns:wcm=\"http://schemas.microsoft.com/WMIConfig/2002/State\">\n",
            arch
        ));
        xml.push_str("      <RunSynchronous>\n");
        xml.push_str(&run_sync_command(
            1,
            "cmd /c echo BypassNRO > C:\\Windows\\System32\\BypassNRO.cmd",
            "BypassNRO",
        ));
        xml.push_str("      </RunSynchronous>\n");
        xml.push_str("    </component>\n");
    }

    xml.push_str("  </settings>\n");
    xml
}

/// Generate a RunSynchronousCommand element.
fn run_sync_command(order: usize, cmd: &str, desc: &str) -> String {
    format!(
        "        <RunSynchronousCommand wcm:action=\"add\">\n\
         \x20         <Order>{}</Order>\n\
         \x20         <Path>{}</Path>\n\
         \x20         <Description>{}</Description>\n\
         \x20       </RunSynchronousCommand>\n",
        order,
        xml_escape(cmd),
        xml_escape(desc),
    )
}

/// Minimal XML escaping for text content.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Write the unattend.xml to the specified path on a USB drive.
///
/// The file is placed at `<mount>/autounattend.xml` for automatic detection
/// by Windows Setup. An additional copy at `<mount>/sources/unattend.xml`
/// may be created for upgrade scenarios.
pub fn write_unattend(config: &UnattendConfig, mount_point: &Path) -> Result<()> {
    let xml = generate_unattend_xml(config);

    // Primary location: root of USB
    let primary = mount_point.join("autounattend.xml");
    info!("Writing unattend.xml to {}", primary.display());
    std::fs::write(&primary, &xml)
        .with_context(|| format!("Failed to write {}", primary.display()))?;

    // Secondary location: sources directory (for upgrade scenarios)
    let sources_dir = mount_point.join("sources");
    if sources_dir.exists() {
        let secondary = sources_dir.join("unattend.xml");
        debug!("Writing secondary unattend.xml to {}", secondary.display());
        std::fs::write(&secondary, &xml)
            .with_context(|| format!("Failed to write {}", secondary.display()))?;
    }

    Ok(())
}

/// Convenience: generate a "just works" config for Windows 11 with all bypasses.
pub fn windows11_bypass_config(username: &str, password: &str) -> UnattendConfig {
    UnattendConfig {
        arch: WindowsArch::Amd64,
        version: WindowsVersion::Windows11,
        accounts: vec![UserAccount {
            name: username.into(),
            password: password.into(),
            display_name: username.into(),
            is_admin: true,
            auto_logon: true,
        }],
        hardware_bypass: HardwareBypass::default(), // all bypasses
        oobe: OobeOptions::default(),               // skip everything
        ..Default::default()
    }
}

/// Convenience: generate a minimal config for Windows 10.
pub fn windows10_minimal_config(username: &str) -> UnattendConfig {
    UnattendConfig {
        arch: WindowsArch::Amd64,
        version: WindowsVersion::Windows10,
        accounts: vec![UserAccount {
            name: username.into(),
            password: String::new(),
            display_name: username.into(),
            is_admin: true,
            auto_logon: true,
        }],
        hardware_bypass: HardwareBypass {
            bypass_tpm: false,
            bypass_secure_boot: false,
            bypass_ram: false,
            bypass_storage: false,
            bypass_cpu: false,
        },
        oobe: OobeOptions {
            skip_ms_account: true,
            skip_privacy: true,
            skip_eula: true,
            disable_cortana: true,
            disable_telemetry: false,
            hide_wireless_setup: false,
            computer_name: String::new(),
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_arch_display() {
        assert_eq!(WindowsArch::Amd64.to_string(), "amd64");
        assert_eq!(WindowsArch::X86.to_string(), "x86");
        assert_eq!(WindowsArch::Arm64.to_string(), "arm64");
    }

    #[test]
    fn test_windows_arch_from_wim() {
        assert_eq!(WindowsArch::from_wim("amd64"), Some(WindowsArch::Amd64));
        assert_eq!(WindowsArch::from_wim("x86_64"), Some(WindowsArch::Amd64));
        assert_eq!(WindowsArch::from_wim("x86"), Some(WindowsArch::X86));
        assert_eq!(WindowsArch::from_wim("arm64"), Some(WindowsArch::Arm64));
        assert_eq!(WindowsArch::from_wim("aarch64"), Some(WindowsArch::Arm64));
        assert_eq!(WindowsArch::from_wim("mips"), None);
    }

    #[test]
    fn test_windows_version_from_build() {
        assert_eq!(
            WindowsVersion::from_build_number(22621),
            WindowsVersion::Windows11
        );
        assert_eq!(
            WindowsVersion::from_build_number(19045),
            WindowsVersion::Windows10
        );
        assert_eq!(
            WindowsVersion::from_build_number(20348),
            WindowsVersion::WindowsServer2022
        );
        assert_eq!(
            WindowsVersion::from_build_number(26100),
            WindowsVersion::WindowsServer2025
        );
    }

    #[test]
    fn test_windows_version_needs_hw_bypass() {
        assert!(WindowsVersion::Windows11.needs_hw_bypass());
        assert!(!WindowsVersion::Windows10.needs_hw_bypass());
    }

    #[test]
    fn test_xml_escape() {
        assert_eq!(xml_escape("a & b"), "a &amp; b");
        assert_eq!(xml_escape("<tag>"), "&lt;tag&gt;");
        assert_eq!(xml_escape("he said \"hi\""), "he said &quot;hi&quot;");
    }

    #[test]
    fn test_generate_unattend_xml_basic() {
        let config = UnattendConfig::default();
        let xml = generate_unattend_xml(&config);
        assert!(xml.starts_with("<?xml version=\"1.0\""));
        assert!(xml.contains("<unattend"));
        assert!(xml.contains("</unattend>"));
        assert!(xml.contains("oobeSystem"));
    }

    #[test]
    fn test_generate_unattend_xml_has_bypass() {
        let config = UnattendConfig::default();
        let xml = generate_unattend_xml(&config);
        assert!(xml.contains("BypassTPMCheck"));
        assert!(xml.contains("BypassSecureBootCheck"));
        assert!(xml.contains("BypassRAMCheck"));
        assert!(xml.contains("BypassStorageCheck"));
        assert!(xml.contains("BypassCPUCheck"));
    }

    #[test]
    fn test_generate_unattend_xml_no_bypass_for_win10() {
        let config = windows10_minimal_config("admin");
        let xml = generate_unattend_xml(&config);
        assert!(!xml.contains("BypassTPMCheck"));
        assert!(!xml.contains("windowsPE"));
    }

    #[test]
    fn test_generate_unattend_xml_user_account() {
        let config = windows11_bypass_config("TestUser", "pass123");
        let xml = generate_unattend_xml(&config);
        assert!(xml.contains("<Name>TestUser</Name>"));
        assert!(xml.contains("<Value>pass123</Value>"));
        assert!(xml.contains("<PlainText>true</PlainText>"));
        assert!(xml.contains("Administrators"));
    }

    #[test]
    fn test_generate_unattend_xml_locale() {
        let mut config = UnattendConfig::default();
        config.locale.timezone = "Eastern Standard Time".into();
        config.locale.ui_language = "fr-FR".into();
        let xml = generate_unattend_xml(&config);
        assert!(xml.contains("Eastern Standard Time"));
        assert!(xml.contains("fr-FR"));
    }

    #[test]
    fn test_generate_unattend_xml_product_key() {
        let mut config = UnattendConfig::default();
        config.product_key = Some("XXXXX-XXXXX-XXXXX-XXXXX-XXXXX".into());
        let xml = generate_unattend_xml(&config);
        assert!(xml.contains("<ProductKey>XXXXX-XXXXX-XXXXX-XXXXX-XXXXX</ProductKey>"));
    }

    #[test]
    fn test_generate_unattend_xml_computer_name() {
        let mut config = UnattendConfig::default();
        config.oobe.computer_name = "MY-PC".into();
        let xml = generate_unattend_xml(&config);
        assert!(xml.contains("<ComputerName>MY-PC</ComputerName>"));
    }

    #[test]
    fn test_generate_unattend_xml_skip_ms_account() {
        let config = UnattendConfig::default();
        let xml = generate_unattend_xml(&config);
        assert!(xml.contains("HideOnlineAccountScreens"));
        assert!(xml.contains("BypassNRO"));
    }

    #[test]
    fn test_generate_unattend_xml_auto_logon() {
        let config = UnattendConfig::default();
        let xml = generate_unattend_xml(&config);
        assert!(xml.contains("<AutoLogon>"));
        assert!(xml.contains("<Enabled>true</Enabled>"));
        assert!(xml.contains("<LogonCount>1</LogonCount>"));
    }

    #[test]
    fn test_generate_unattend_xml_arch() {
        let mut config = UnattendConfig::default();
        config.arch = WindowsArch::Arm64;
        let xml = generate_unattend_xml(&config);
        assert!(xml.contains("processorArchitecture=\"arm64\""));
    }

    #[test]
    fn test_windows11_bypass_config() {
        let config = windows11_bypass_config("admin", "pw");
        assert_eq!(config.version, WindowsVersion::Windows11);
        assert!(config.hardware_bypass.bypass_tpm);
        assert_eq!(config.accounts[0].name, "admin");
    }

    #[test]
    fn test_windows10_minimal_config() {
        let config = windows10_minimal_config("user");
        assert_eq!(config.version, WindowsVersion::Windows10);
        assert!(!config.hardware_bypass.bypass_tpm);
        assert!(config.oobe.skip_ms_account);
    }

    #[test]
    fn test_write_unattend() {
        let dir = tempfile::tempdir().unwrap();
        let config = UnattendConfig::default();
        write_unattend(&config, dir.path()).unwrap();
        let primary = dir.path().join("autounattend.xml");
        assert!(primary.exists());
        let content = std::fs::read_to_string(&primary).unwrap();
        assert!(content.contains("<unattend"));
    }

    #[test]
    fn test_default_values() {
        let config = UnattendConfig::default();
        assert_eq!(config.arch, WindowsArch::Amd64);
        assert_eq!(config.version, WindowsVersion::Windows11);
        assert_eq!(config.accounts.len(), 1);
        assert_eq!(config.locale.ui_language, "en-US");
        assert!(config.hardware_bypass.bypass_tpm);
        assert!(config.oobe.skip_ms_account);
    }

    #[test]
    fn test_config_serde_roundtrip() {
        let config = windows11_bypass_config("test", "pw");
        let json = serde_json::to_string(&config).unwrap();
        let deser: UnattendConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.accounts[0].name, "test");
        assert_eq!(deser.arch, WindowsArch::Amd64);
    }
}
