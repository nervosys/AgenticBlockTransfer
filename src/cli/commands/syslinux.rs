// CLI command: syslinux — Detect and plan bootloader installation

use anyhow::Result;

use crate::cli::SyslinuxOpts;
use crate::core::syslinux;

pub async fn execute(opts: SyslinuxOpts) -> Result<()> {
    match opts.action.as_str() {
        "detect" => {
            let paths: Vec<&str> = opts.files.iter().map(|s| s.as_str()).collect();
            if paths.is_empty() {
                anyhow::bail!("Provide file paths with --files to detect bootloaders");
            }
            let detections = syslinux::detect_bootloaders(&paths);

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&detections)?);
            } else if detections.is_empty() {
                println!("No bootloaders detected in the provided files.");
            } else {
                println!("Detected Bootloaders:");
                for det in &detections {
                    println!("  {}", det);
                    if !det.files_found.is_empty() {
                        println!("    Files: {}", det.files_found.join(", "));
                    }
                    if !det.notes.is_empty() {
                        for note in &det.notes {
                            println!("    Note: {}", note);
                        }
                    }
                }
            }
        }
        "version" => {
            let file_path = opts
                .file
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--file is required for version detection"))?;
            let data = std::fs::read(file_path)?;
            let version = syslinux::parse_syslinux_version(&data);

            if opts.json {
                if let Some(ref v) = version {
                    println!("{}", serde_json::to_string_pretty(v)?);
                } else {
                    println!("null");
                }
            } else if let Some(v) = version {
                println!("Syslinux Version: {}", v);
                println!("  Major:      {}", v.major);
                println!("  Minor:      {}", v.minor);
                if let Some(p) = v.patch {
                    println!("  Patch:      {}", p);
                }
                if let Some(ref pre) = v.pre_release {
                    println!("  Pre:        {}", pre);
                }
                println!("  v6+:        {}", if v.is_v6_or_later() { "Yes" } else { "No" });
                println!("  Needs .c32: {}", if v.needs_c32_modules() { "Yes" } else { "No" });
            } else {
                println!("No Syslinux version string found in {}", file_path);
            }
        }
        "plan" => {
            let bootloader_str = opts
                .bootloader
                .as_deref()
                .unwrap_or("syslinux-v6");
            let fs_str = opts.filesystem.as_deref().unwrap_or("FAT32");

            let bootloader = match bootloader_str {
                "syslinux-v4" | "syslinux4" => syslinux::BootloaderType::SyslinuxV4,
                "syslinux-v6" | "syslinux6" | "syslinux" => syslinux::BootloaderType::SyslinuxV6,
                "isolinux" => syslinux::BootloaderType::Isolinux,
                "extlinux" => syslinux::BootloaderType::Extlinux,
                "grub2" | "grub" => syslinux::BootloaderType::Grub2,
                "grub4dos" => syslinux::BootloaderType::Grub4dos,
                other => anyhow::bail!(
                    "Unknown bootloader: '{}'. Use: syslinux-v4, syslinux-v6, isolinux, extlinux, grub2, grub4dos",
                    other
                ),
            };

            let plan = syslinux::plan_installation(bootloader, fs_str)?;

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                println!("Bootloader Installation Plan");
                println!("  Bootloader: {}", plan.bootloader_type);
                println!("  Target FS:  {}", plan.target_fs);
                println!("  MBR Action: {}", plan.boot_record_action);
                println!(
                    "  Download:   {}",
                    if plan.needs_download { "Required" } else { "Not needed" }
                );
                if !plan.files_to_copy.is_empty() {
                    println!();
                    println!("Files to install:");
                    for f in &plan.files_to_copy {
                        let flags = if f.hidden_system { " [hidden+system]" } else { "" };
                        println!("  {} → {}{}", f.source, f.destination, flags);
                    }
                }
            }
        }
        "config" => {
            let label = opts.label.as_deref().unwrap_or("linux");
            let kernel = opts.kernel.as_deref().unwrap_or("/vmlinuz");
            let initrd = opts.initrd.as_deref().unwrap_or("/initrd.img");
            let append = opts
                .append
                .as_deref()
                .unwrap_or("root=/dev/sda1 ro quiet splash");
            let title = opts.title.as_deref().unwrap_or("Boot Menu");

            let config = syslinux::SyslinuxConfig {
                default_label: label.to_string(),
                timeout: opts.timeout.unwrap_or(100),
                prompt: true,
                menu_title: Some(title.to_string()),
                splash: None,
                entries: vec![syslinux::BootEntry {
                    label: label.to_string(),
                    menu_label: format!("Boot {}", label),
                    kernel: kernel.to_string(),
                    append: append.to_string(),
                    initrd: Some(initrd.to_string()),
                }],
            };

            let output = syslinux::generate_syslinux_config(&config);
            if opts.json {
                println!(
                    "{}",
                    serde_json::json!({ "syslinux_cfg": output })
                );
            } else {
                print!("{}", output);
            }
        }
        "types" => {
            let types = [
                syslinux::BootloaderType::SyslinuxV4,
                syslinux::BootloaderType::SyslinuxV6,
                syslinux::BootloaderType::Isolinux,
                syslinux::BootloaderType::Extlinux,
                syslinux::BootloaderType::Grub2,
                syslinux::BootloaderType::Grub4dos,
                syslinux::BootloaderType::Bootmgr,
                syslinux::BootloaderType::Freeldr,
            ];

            if opts.json {
                let entries: Vec<serde_json::Value> = types
                    .iter()
                    .map(|t| {
                        serde_json::json!({
                            "name": format!("{}", t),
                            "fat": t.supports_fat(),
                            "ntfs": t.supports_ntfs(),
                            "ext": t.supports_ext(),
                            "uefi": t.supports_uefi(),
                            "bios": t.supports_bios(),
                        })
                    })
                    .collect();
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                println!(
                    "{:<24} {:<6} {:<6} {:<6} {:<6} {:<6}",
                    "BOOTLOADER", "FAT", "NTFS", "EXT", "UEFI", "BIOS"
                );
                println!("{}", "-".repeat(54));
                for t in &types {
                    let yn = |b: bool| if b { "Yes" } else { "-" };
                    println!(
                        "{:<24} {:<6} {:<6} {:<6} {:<6} {:<6}",
                        format!("{}", t),
                        yn(t.supports_fat()),
                        yn(t.supports_ntfs()),
                        yn(t.supports_ext()),
                        yn(t.supports_uefi()),
                        yn(t.supports_bios()),
                    );
                }
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'detect', 'version', 'plan', 'config', or 'types'.",
                other
            );
        }
    }

    Ok(())
}
