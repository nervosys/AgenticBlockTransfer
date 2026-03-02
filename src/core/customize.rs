// OS customization — generate firstrun.sh / cloud-init YAML for pre-configuring
// hostname, SSH keys, WiFi credentials, user accounts, timezone, locale on written images.
// Inspired by rpi-imager's OS customization dialog.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// WiFi security type.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WifiSecurity {
    Open,
    WEP,
    WPA,
    WPA2,
    WPA3,
}

impl fmt::Display for WifiSecurity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::WEP => write!(f, "WEP"),
            Self::WPA => write!(f, "WPA"),
            Self::WPA2 => write!(f, "WPA2-PSK"),
            Self::WPA3 => write!(f, "SAE"),
        }
    }
}

/// WiFi credentials for injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WifiConfig {
    pub ssid: String,
    pub psk: Option<String>,
    pub security: WifiSecurity,
    pub country_code: Option<String>,
    pub hidden: bool,
}

/// User account to create on first boot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAccount {
    pub username: String,
    pub password_hash: Option<String>,
    pub ssh_authorized_keys: Vec<String>,
    pub groups: Vec<String>,
    pub shell: String,
}

impl Default for UserAccount {
    fn default() -> Self {
        Self {
            username: String::new(),
            password_hash: None,
            ssh_authorized_keys: Vec::new(),
            groups: vec!["sudo".into()],
            shell: "/bin/bash".into(),
        }
    }
}

/// SSH configuration for injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub enable: bool,
    pub authorized_keys: Vec<String>,
    pub password_auth: bool,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            enable: true,
            authorized_keys: Vec::new(),
            password_auth: true,
        }
    }
}

/// Locale and keyboard layout settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocaleConfig {
    pub timezone: String,
    pub locale: String,
    pub keyboard_layout: String,
    pub keyboard_variant: Option<String>,
}

impl Default for LocaleConfig {
    fn default() -> Self {
        Self {
            timezone: "UTC".into(),
            locale: "en_US.UTF-8".into(),
            keyboard_layout: "us".into(),
            keyboard_variant: None,
        }
    }
}

/// Output format for customization files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CustomizationFormat {
    /// Shell script for first boot (firstrun.sh)
    FirstrunScript,
    /// cloud-init user-data YAML
    CloudInit,
    /// cloud-init network-config YAML
    NetworkConfig,
}

impl fmt::Display for CustomizationFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FirstrunScript => write!(f, "firstrun.sh"),
            Self::CloudInit => write!(f, "cloud-init user-data"),
            Self::NetworkConfig => write!(f, "cloud-init network-config"),
        }
    }
}

/// Full OS customization specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsCustomization {
    pub hostname: Option<String>,
    pub users: Vec<UserAccount>,
    pub ssh: Option<SshConfig>,
    pub wifi: Option<WifiConfig>,
    pub locale: Option<LocaleConfig>,
    pub packages: Vec<String>,
    pub run_commands: Vec<String>,
    pub extra_files: HashMap<String, String>,
}

impl Default for OsCustomization {
    fn default() -> Self {
        Self {
            hostname: None,
            users: Vec::new(),
            ssh: None,
            wifi: None,
            locale: None,
            packages: Vec::new(),
            run_commands: Vec::new(),
            extra_files: HashMap::new(),
        }
    }
}

impl OsCustomization {
    /// Create a new customization with hostname.
    pub fn with_hostname(hostname: &str) -> Self {
        Self {
            hostname: Some(hostname.to_string()),
            ..Default::default()
        }
    }

    /// Add a user account.
    pub fn add_user(&mut self, user: UserAccount) {
        self.users.push(user);
    }

    /// Set WiFi credentials.
    pub fn set_wifi(&mut self, ssid: &str, psk: Option<&str>, security: WifiSecurity) {
        self.wifi = Some(WifiConfig {
            ssid: ssid.to_string(),
            psk: psk.map(|s| s.to_string()),
            security,
            country_code: None,
            hidden: false,
        });
    }

    /// Enable SSH with optional authorized keys.
    pub fn enable_ssh(&mut self, keys: Vec<String>) {
        self.ssh = Some(SshConfig {
            enable: true,
            authorized_keys: keys,
            password_auth: true,
        });
    }

    /// Generate a firstrun.sh script for first-boot customization.
    pub fn generate_firstrun_script(&self) -> Result<String> {
        let mut script = String::new();
        script.push_str("#!/bin/bash\n");
        script.push_str("# Generated by abt — AgenticBlockTransfer OS customization\n");
        script.push_str("# This script runs once on first boot and then removes itself.\n");
        script.push_str("set -e\n\n");

        // Hostname
        if let Some(ref hostname) = self.hostname {
            validate_hostname(hostname)?;
            script.push_str(&format!("# Set hostname\n"));
            script.push_str(&format!("echo '{}' > /etc/hostname\n", hostname));
            script.push_str(&format!(
                "sed -i 's/127\\.0\\.1\\.1.*/127.0.1.1\\t{}/' /etc/hosts\n",
                hostname
            ));
            script.push_str(&format!("hostnamectl set-hostname '{}' 2>/dev/null || true\n\n", hostname));
        }

        // Users
        for user in &self.users {
            validate_username(&user.username)?;
            script.push_str(&format!("# Create user: {}\n", user.username));
            let groups_str = user.groups.join(",");
            script.push_str(&format!(
                "useradd -m -s {} -G {} {} 2>/dev/null || true\n",
                user.shell, groups_str, user.username
            ));

            if let Some(ref hash) = user.password_hash {
                script.push_str(&format!(
                    "echo '{}:{}' | chpasswd -e\n",
                    user.username, hash
                ));
            }

            if !user.ssh_authorized_keys.is_empty() {
                script.push_str(&format!(
                    "mkdir -p /home/{}/.ssh\n",
                    user.username
                ));
                for key in &user.ssh_authorized_keys {
                    script.push_str(&format!(
                        "echo '{}' >> /home/{}/.ssh/authorized_keys\n",
                        key, user.username
                    ));
                }
                script.push_str(&format!(
                    "chmod 700 /home/{}/.ssh\nchmod 600 /home/{}/.ssh/authorized_keys\nchown -R {}:{} /home/{}/.ssh\n",
                    user.username, user.username, user.username, user.username, user.username
                ));
            }
            script.push('\n');
        }

        // SSH
        if let Some(ref ssh) = self.ssh {
            if ssh.enable {
                script.push_str("# Enable SSH\n");
                script.push_str("systemctl enable ssh 2>/dev/null || systemctl enable sshd 2>/dev/null || true\n");
                script.push_str("systemctl start ssh 2>/dev/null || systemctl start sshd 2>/dev/null || true\n");
                if !ssh.password_auth {
                    script.push_str("sed -i 's/^#*PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config\n");
                }
                script.push('\n');
            }
        }

        // Timezone / locale
        if let Some(ref locale) = self.locale {
            script.push_str("# Set timezone and locale\n");
            script.push_str(&format!(
                "ln -sf /usr/share/zoneinfo/{} /etc/localtime\n",
                locale.timezone
            ));
            script.push_str(&format!(
                "timedatectl set-timezone '{}' 2>/dev/null || true\n",
                locale.timezone
            ));
            script.push_str(&format!(
                "echo 'LANG={}' > /etc/locale.conf 2>/dev/null || true\n",
                locale.locale
            ));
            script.push_str(&format!(
                "localectl set-x11-keymap '{}' '' '{}' 2>/dev/null || true\n",
                locale.keyboard_layout,
                locale.keyboard_variant.as_deref().unwrap_or("")
            ));
            script.push('\n');
        }

        // WiFi
        if let Some(ref wifi) = self.wifi {
            script.push_str("# Configure WiFi\n");
            script.push_str("mkdir -p /etc/NetworkManager/system-connections\n");
            script.push_str(&format!(
                "cat > /etc/NetworkManager/system-connections/'{}.nmconnection' << 'WIFI_EOF'\n",
                wifi.ssid
            ));
            script.push_str("[connection]\n");
            script.push_str(&format!("id={}\n", wifi.ssid));
            script.push_str("type=wifi\n");
            script.push_str("autoconnect=true\n\n");
            script.push_str("[wifi]\n");
            script.push_str(&format!("ssid={}\n", wifi.ssid));
            if wifi.hidden {
                script.push_str("hidden=true\n");
            }
            script.push_str("mode=infrastructure\n\n");
            if wifi.security != WifiSecurity::Open {
                script.push_str("[wifi-security]\n");
                match wifi.security {
                    WifiSecurity::WPA2 | WifiSecurity::WPA3 => {
                        script.push_str("key-mgmt=wpa-psk\n");
                        if let Some(ref psk) = wifi.psk {
                            script.push_str(&format!("psk={}\n", psk));
                        }
                    }
                    WifiSecurity::WPA => {
                        script.push_str("key-mgmt=wpa-psk\n");
                        if let Some(ref psk) = wifi.psk {
                            script.push_str(&format!("psk={}\n", psk));
                        }
                    }
                    WifiSecurity::WEP => {
                        script.push_str("key-mgmt=none\n");
                        if let Some(ref psk) = wifi.psk {
                            script.push_str(&format!("wep-key0={}\n", psk));
                        }
                    }
                    WifiSecurity::Open => {}
                }
                script.push('\n');
            }
            script.push_str("[ipv4]\nmethod=auto\n\n");
            script.push_str("[ipv6]\nmethod=auto\n");
            script.push_str("WIFI_EOF\n");
            script.push_str(&format!(
                "chmod 600 /etc/NetworkManager/system-connections/'{}.nmconnection'\n\n",
                wifi.ssid
            ));

            // Also generate wpa_supplicant fallback
            script.push_str("# wpa_supplicant fallback\n");
            script.push_str("mkdir -p /etc/wpa_supplicant\n");
            script.push_str("cat >> /etc/wpa_supplicant/wpa_supplicant.conf << 'WPA_EOF'\n");
            if let Some(ref cc) = wifi.country_code {
                script.push_str(&format!("country={}\n", cc));
            }
            script.push_str("network={\n");
            script.push_str(&format!("    ssid=\"{}\"\n", wifi.ssid));
            if let Some(ref psk) = wifi.psk {
                script.push_str(&format!("    psk=\"{}\"\n", psk));
            }
            if wifi.hidden {
                script.push_str("    scan_ssid=1\n");
            }
            script.push_str("}\n");
            script.push_str("WPA_EOF\n\n");
        }

        // Install packages
        if !self.packages.is_empty() {
            script.push_str("# Install packages\n");
            script.push_str("export DEBIAN_FRONTEND=noninteractive\n");
            script.push_str("apt-get update -q 2>/dev/null && apt-get install -y -q ");
            for pkg in &self.packages {
                script.push_str(&format!("{} ", pkg));
            }
            script.push_str("2>/dev/null || true\n\n");
        }

        // Custom commands
        for cmd in &self.run_commands {
            script.push_str(&format!("{}\n", cmd));
        }

        // Self-remove
        script.push_str("\n# Remove this script after execution\n");
        script.push_str("rm -f \"$0\"\n");

        Ok(script)
    }

    /// Generate cloud-init user-data YAML.
    pub fn generate_cloud_init(&self) -> Result<String> {
        let mut yaml = String::new();
        yaml.push_str("#cloud-config\n");
        yaml.push_str("# Generated by abt — AgenticBlockTransfer OS customization\n\n");

        // Hostname
        if let Some(ref hostname) = self.hostname {
            validate_hostname(hostname)?;
            yaml.push_str(&format!("hostname: {}\n", hostname));
            yaml.push_str("manage_etc_hosts: true\n\n");
        }

        // Users
        if !self.users.is_empty() {
            yaml.push_str("users:\n");
            for user in &self.users {
                validate_username(&user.username)?;
                yaml.push_str(&format!("  - name: {}\n", user.username));
                if !user.groups.is_empty() {
                    yaml.push_str("    groups:\n");
                    for group in &user.groups {
                        yaml.push_str(&format!("      - {}\n", group));
                    }
                }
                yaml.push_str(&format!("    shell: {}\n", user.shell));
                if user.password_hash.is_some() {
                    yaml.push_str("    lock_passwd: false\n");
                    yaml.push_str(&format!(
                        "    passwd: {}\n",
                        user.password_hash.as_ref().unwrap()
                    ));
                }
                if !user.ssh_authorized_keys.is_empty() {
                    yaml.push_str("    ssh_authorized_keys:\n");
                    for key in &user.ssh_authorized_keys {
                        yaml.push_str(&format!("      - {}\n", key));
                    }
                }
            }
            yaml.push('\n');
        }

        // SSH
        if let Some(ref ssh) = self.ssh {
            yaml.push_str("ssh_pwauth: ");
            yaml.push_str(if ssh.password_auth { "true\n" } else { "false\n" });
        }

        // Timezone / locale
        if let Some(ref locale) = self.locale {
            yaml.push_str(&format!("timezone: {}\n", locale.timezone));
            yaml.push_str(&format!("locale: {}\n", locale.locale));
            yaml.push_str("keyboard:\n");
            yaml.push_str(&format!("  layout: {}\n", locale.keyboard_layout));
            if let Some(ref variant) = locale.keyboard_variant {
                yaml.push_str(&format!("  variant: {}\n", variant));
            }
            yaml.push('\n');
        }

        // WiFi (via network-config, referenced separately)
        // cloud-init WiFi is in network-config, not user-data

        // Packages
        if !self.packages.is_empty() {
            yaml.push_str("package_update: true\n");
            yaml.push_str("packages:\n");
            for pkg in &self.packages {
                yaml.push_str(&format!("  - {}\n", pkg));
            }
            yaml.push('\n');
        }

        // Run commands
        if !self.run_commands.is_empty() {
            yaml.push_str("runcmd:\n");
            for cmd in &self.run_commands {
                yaml.push_str(&format!("  - {}\n", cmd));
            }
            yaml.push('\n');
        }

        // Extra files
        if !self.extra_files.is_empty() {
            yaml.push_str("write_files:\n");
            for (path, content) in &self.extra_files {
                yaml.push_str(&format!("  - path: {}\n", path));
                yaml.push_str("    content: |\n");
                for line in content.lines() {
                    yaml.push_str(&format!("      {}\n", line));
                }
            }
        }

        Ok(yaml)
    }

    /// Generate cloud-init network-config YAML (for WiFi).
    pub fn generate_network_config(&self) -> Result<String> {
        let mut yaml = String::new();
        yaml.push_str("# cloud-init network configuration\n");
        yaml.push_str("# Generated by abt — AgenticBlockTransfer\n\n");
        yaml.push_str("version: 2\n");

        if let Some(ref wifi) = self.wifi {
            yaml.push_str("wifis:\n");
            yaml.push_str("  wlan0:\n");
            yaml.push_str("    dhcp4: true\n");
            yaml.push_str("    optional: true\n");
            yaml.push_str("    access-points:\n");
            yaml.push_str(&format!("      \"{}\":\n", wifi.ssid));
            if let Some(ref psk) = wifi.psk {
                yaml.push_str(&format!("        password: \"{}\"\n", psk));
            }
            if wifi.hidden {
                yaml.push_str("        hidden: true\n");
            }
        } else {
            yaml.push_str("ethernets:\n");
            yaml.push_str("  eth0:\n");
            yaml.push_str("    dhcp4: true\n");
            yaml.push_str("    optional: true\n");
        }

        Ok(yaml)
    }

    /// Generate all customization files and return them as (filename, content) pairs.
    pub fn generate_all(&self) -> Result<Vec<(String, String)>> {
        let mut files = Vec::new();
        files.push(("firstrun.sh".into(), self.generate_firstrun_script()?));
        files.push(("user-data".into(), self.generate_cloud_init()?));
        files.push(("network-config".into(), self.generate_network_config()?));
        Ok(files)
    }

    /// Write customization files to a directory.
    pub fn write_to_dir(&self, dir: &Path) -> Result<Vec<PathBuf>> {
        std::fs::create_dir_all(dir)?;
        let files = self.generate_all()?;
        let mut paths = Vec::new();
        for (name, content) in &files {
            let path = dir.join(name);
            std::fs::write(&path, content)?;
            paths.push(path);
        }
        Ok(paths)
    }

    /// Serialize customization to JSON (for saving/loading presets).
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// Deserialize customization from JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        Ok(serde_json::from_str(json)?)
    }
}

/// Validate hostname (RFC 952 / RFC 1123).
fn validate_hostname(hostname: &str) -> Result<()> {
    if hostname.is_empty() || hostname.len() > 253 {
        return Err(anyhow!("hostname must be 1-253 characters"));
    }
    for label in hostname.split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(anyhow!("hostname label must be 1-63 characters"));
        }
        if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(anyhow!(
                "hostname label contains invalid characters (only a-z, 0-9, -)"
            ));
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err(anyhow!("hostname label cannot start or end with a hyphen"));
        }
    }
    Ok(())
}

/// Validate username (POSIX).
fn validate_username(username: &str) -> Result<()> {
    if username.is_empty() || username.len() > 32 {
        return Err(anyhow!("username must be 1-32 characters"));
    }
    if !username.starts_with(|c: char| c.is_ascii_lowercase() || c == '_') {
        return Err(anyhow!("username must start with a lowercase letter or underscore"));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(anyhow!(
            "username may only contain lowercase letters, digits, underscore, or hyphen"
        ));
    }
    Ok(())
}

/// Detect current WiFi SSID from the host OS (best-effort).
pub fn detect_current_wifi() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("iwgetid")
            .arg("-r")
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                } else {
                    None
                }
            })
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("networksetup")
            .args(["-getairportnetwork", "en0"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .and_then(|s| s.strip_prefix("Current Wi-Fi Network: ").map(|n| n.trim().to_string()))
                } else {
                    None
                }
            })
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("netsh")
            .args(["wlan", "show", "interfaces"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout).ok().and_then(|s| {
                        s.lines()
                            .find(|l| l.trim().starts_with("SSID") && !l.contains("BSSID"))
                            .and_then(|l| l.split(':').nth(1))
                            .map(|s| s.trim().to_string())
                    })
                } else {
                    None
                }
            })
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// Detect default SSH public key from ~/.ssh/.
pub fn detect_ssh_public_key() -> Option<String> {
    let home = dirs::home_dir()?;
    let candidates = ["id_ed25519.pub", "id_rsa.pub", "id_ecdsa.pub"];
    for name in &candidates {
        let path = home.join(".ssh").join(name);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let key = content.trim().to_string();
                if !key.is_empty() {
                    return Some(key);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hostname_validation_valid() {
        assert!(validate_hostname("my-pi").is_ok());
        assert!(validate_hostname("host123").is_ok());
        assert!(validate_hostname("a.b.c").is_ok());
    }

    #[test]
    fn test_hostname_validation_invalid() {
        assert!(validate_hostname("").is_err());
        assert!(validate_hostname("-start").is_err());
        assert!(validate_hostname("end-").is_err());
        assert!(validate_hostname("bad!char").is_err());
    }

    #[test]
    fn test_username_validation_valid() {
        assert!(validate_username("pi").is_ok());
        assert!(validate_username("user_name").is_ok());
        assert!(validate_username("_daemon").is_ok());
    }

    #[test]
    fn test_username_validation_invalid() {
        assert!(validate_username("").is_err());
        assert!(validate_username("Root").is_err());
        assert!(validate_username("1user").is_err());
        assert!(validate_username("bad user").is_err());
    }

    #[test]
    fn test_default_customization() {
        let c = OsCustomization::default();
        assert!(c.hostname.is_none());
        assert!(c.users.is_empty());
        assert!(c.wifi.is_none());
    }

    #[test]
    fn test_with_hostname() {
        let c = OsCustomization::with_hostname("my-pi");
        assert_eq!(c.hostname.as_deref(), Some("my-pi"));
    }

    #[test]
    fn test_set_wifi() {
        let mut c = OsCustomization::default();
        c.set_wifi("MyNetwork", Some("password123"), WifiSecurity::WPA2);
        assert!(c.wifi.is_some());
        let w = c.wifi.unwrap();
        assert_eq!(w.ssid, "MyNetwork");
        assert_eq!(w.psk.as_deref(), Some("password123"));
    }

    #[test]
    fn test_enable_ssh() {
        let mut c = OsCustomization::default();
        c.enable_ssh(vec!["ssh-ed25519 AAAA...".into()]);
        assert!(c.ssh.is_some());
        assert!(c.ssh.as_ref().unwrap().enable);
        assert_eq!(c.ssh.as_ref().unwrap().authorized_keys.len(), 1);
    }

    #[test]
    fn test_generate_firstrun_script() {
        let mut c = OsCustomization::with_hostname("test-host");
        c.enable_ssh(vec![]);
        let script = c.generate_firstrun_script().unwrap();
        assert!(script.contains("#!/bin/bash"));
        assert!(script.contains("test-host"));
        assert!(script.contains("hostnamectl"));
        assert!(script.contains("ssh"));
    }

    #[test]
    fn test_generate_cloud_init() {
        let mut c = OsCustomization::with_hostname("cloud-host");
        c.locale = Some(LocaleConfig {
            timezone: "America/New_York".into(),
            locale: "en_US.UTF-8".into(),
            keyboard_layout: "us".into(),
            keyboard_variant: None,
        });
        let yaml = c.generate_cloud_init().unwrap();
        assert!(yaml.contains("#cloud-config"));
        assert!(yaml.contains("hostname: cloud-host"));
        assert!(yaml.contains("timezone: America/New_York"));
    }

    #[test]
    fn test_generate_network_config_wifi() {
        let mut c = OsCustomization::default();
        c.set_wifi("TestSSID", Some("testpass"), WifiSecurity::WPA2);
        let yaml = c.generate_network_config().unwrap();
        assert!(yaml.contains("wifis:"));
        assert!(yaml.contains("TestSSID"));
        assert!(yaml.contains("testpass"));
    }

    #[test]
    fn test_generate_network_config_ethernet() {
        let c = OsCustomization::default();
        let yaml = c.generate_network_config().unwrap();
        assert!(yaml.contains("ethernets:"));
        assert!(yaml.contains("eth0"));
    }

    #[test]
    fn test_generate_all() {
        let c = OsCustomization::with_hostname("pi");
        let files = c.generate_all().unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].0, "firstrun.sh");
        assert_eq!(files[1].0, "user-data");
        assert_eq!(files[2].0, "network-config");
    }

    #[test]
    fn test_firstrun_with_user_and_wifi() {
        let mut c = OsCustomization::with_hostname("pi4");
        c.add_user(UserAccount {
            username: "admin".into(),
            password_hash: Some("$6$salt$hash".into()),
            ssh_authorized_keys: vec!["ssh-ed25519 AAAA key".into()],
            groups: vec!["sudo".into(), "adm".into()],
            shell: "/bin/bash".into(),
        });
        c.set_wifi("Home", Some("secret"), WifiSecurity::WPA2);
        c.locale = Some(LocaleConfig::default());
        c.packages = vec!["vim".into(), "htop".into()];
        let script = c.generate_firstrun_script().unwrap();
        assert!(script.contains("useradd"));
        assert!(script.contains("admin"));
        assert!(script.contains("NetworkManager"));
        assert!(script.contains("Home"));
        assert!(script.contains("apt-get"));
        assert!(script.contains("vim"));
    }

    #[test]
    fn test_cloud_init_with_user() {
        let mut c = OsCustomization::default();
        c.add_user(UserAccount {
            username: "deploy".into(),
            password_hash: None,
            ssh_authorized_keys: vec!["ssh-rsa BBBB key".into()],
            groups: vec!["docker".into()],
            shell: "/bin/zsh".into(),
        });
        let yaml = c.generate_cloud_init().unwrap();
        assert!(yaml.contains("name: deploy"));
        assert!(yaml.contains("shell: /bin/zsh"));
        assert!(yaml.contains("ssh_authorized_keys:"));
    }

    #[test]
    fn test_json_round_trip() {
        let mut c = OsCustomization::with_hostname("test");
        c.packages = vec!["git".into()];
        let json = c.to_json().unwrap();
        let c2 = OsCustomization::from_json(&json).unwrap();
        assert_eq!(c2.hostname.as_deref(), Some("test"));
        assert_eq!(c2.packages, vec!["git"]);
    }

    #[test]
    fn test_wifi_security_display() {
        assert_eq!(format!("{}", WifiSecurity::WPA2), "WPA2-PSK");
        assert_eq!(format!("{}", WifiSecurity::Open), "open");
        assert_eq!(format!("{}", WifiSecurity::WPA3), "SAE");
    }

    #[test]
    fn test_customization_format_display() {
        assert_eq!(format!("{}", CustomizationFormat::FirstrunScript), "firstrun.sh");
        assert_eq!(format!("{}", CustomizationFormat::CloudInit), "cloud-init user-data");
    }

    #[test]
    fn test_extra_files() {
        let mut c = OsCustomization::default();
        c.extra_files.insert("/etc/motd".into(), "Welcome!".into());
        let yaml = c.generate_cloud_init().unwrap();
        assert!(yaml.contains("write_files:"));
        assert!(yaml.contains("/etc/motd"));
        assert!(yaml.contains("Welcome!"));
    }
}
