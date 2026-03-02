use anyhow::{anyhow, Result};

use crate::cli::CustomizeOpts;
use crate::core::customize::{
    CustomizationFormat, OsCustomization, SshConfig, UserAccount, WifiConfig, WifiSecurity, LocaleConfig,
};

pub async fn execute(opts: CustomizeOpts) -> Result<()> {
    match opts.action.as_str() {
        "generate" => {
            let mut customization = OsCustomization::default();

            if let Some(hostname) = &opts.hostname {
                customization.hostname = Some(hostname.clone());
            }

            if let Some(username) = &opts.username {
                let mut user = UserAccount::default();
                user.username = username.clone();
                if let Some(pw) = &opts.password {
                    user.password_hash = Some(pw.clone());
                }
                customization.users.push(user);
            }

            if opts.enable_ssh || opts.ssh_key.is_some() {
                let mut ssh = SshConfig::default();
                ssh.enable = true;
                ssh.password_auth = opts.enable_ssh;
                if let Some(key) = &opts.ssh_key {
                    ssh.authorized_keys = vec![key.clone()];
                }
                customization.ssh = Some(ssh);
            }

            if let Some(ssid) = &opts.wifi_ssid {
                customization.wifi = Some(WifiConfig {
                    ssid: ssid.clone(),
                    psk: opts.wifi_password.clone(),
                    security: if opts.wifi_password.is_some() {
                        WifiSecurity::WPA2
                    } else {
                        WifiSecurity::Open
                    },
                    country_code: opts.wifi_country.clone(),
                    hidden: false,
                });
            }

            if opts.timezone.is_some() || opts.locale.is_some() {
                let mut loc = LocaleConfig::default();
                if let Some(tz) = &opts.timezone {
                    loc.timezone = tz.clone();
                }
                if let Some(l) = &opts.locale {
                    loc.locale = l.clone();
                }
                customization.locale = Some(loc);
            }

            let format = match opts.format.as_str() {
                "cloud-init" => CustomizationFormat::CloudInit,
                "network-config" => CustomizationFormat::NetworkConfig,
                _ => CustomizationFormat::FirstrunScript,
            };

            let files = customization.generate_all()?;
            let target_files: Vec<_> = files
                .iter()
                .filter(|(name, _)| match format {
                    CustomizationFormat::FirstrunScript => name == "firstrun.sh",
                    CustomizationFormat::CloudInit => {
                        name == "user-data" || name == "meta-data"
                    }
                    CustomizationFormat::NetworkConfig => name == "network-config",
                })
                .collect();

            if target_files.is_empty() {
                return Err(anyhow!("no files generated for format: {}", opts.format));
            }

            customization.write_to_dir(std::path::Path::new(&opts.output_dir))?;
            for (name, _) in &target_files {
                log::info!("Generated: {}/{}", opts.output_dir, name);
            }
            println!(
                "Generated {} file(s) in {}",
                target_files.len(),
                opts.output_dir
            );
            Ok(())
        }
        "detect-wifi" => {
            match crate::core::customize::detect_current_wifi() {
                Some(ssid) => println!("Current WiFi: {}", ssid),
                None => println!("No WiFi connection detected"),
            }
            Ok(())
        }
        "detect-ssh" => {
            match crate::core::customize::detect_ssh_public_key() {
                Some(key) => {
                    let preview = if key.len() > 60 {
                        format!("{}...", &key[..60])
                    } else {
                        key.clone()
                    };
                    println!("SSH public key: {}", preview);
                }
                None => println!("No SSH public key found"),
            }
            Ok(())
        }
        "save-preset" => {
            let preset_path = opts
                .preset
                .ok_or_else(|| anyhow!("--preset is required for save-preset"))?;
            let customization = OsCustomization::default();
            let json = customization.to_json()?;
            std::fs::write(&preset_path, json)?;
            println!("Saved preset to {}", preset_path);
            Ok(())
        }
        "load-preset" => {
            let preset_path = opts
                .preset
                .ok_or_else(|| anyhow!("--preset is required for load-preset"))?;
            let json = std::fs::read_to_string(&preset_path)?;
            let customization = OsCustomization::from_json(&json)?;
            println!("Loaded preset from {}", preset_path);
            if let Some(h) = &customization.hostname {
                println!("  Hostname: {}", h);
            }
            for u in &customization.users {
                println!("  User: {}", u.username);
            }
            Ok(())
        }
        _ => Err(anyhow!(
            "unknown action '{}'. Use: generate, detect-wifi, detect-ssh, save-preset, load-preset",
            opts.action
        )),
    }
}