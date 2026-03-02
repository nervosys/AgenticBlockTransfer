// CLI command: wintogo -- Create Windows To Go USB drives

use anyhow::Result;
use std::path::Path;

use crate::cli::WinToGoOpts;
use crate::core::wintogo;

pub async fn execute(opts: WinToGoOpts) -> Result<()> {
    match opts.action.as_str() {
        "analyze" => {
            let iso_path = opts
                .iso
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--iso is required for analyze"))?;
            let analysis = wintogo::analyze_iso(Path::new(iso_path))?;

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&analysis)?);
            } else {
                println!("Windows To Go ISO Analysis");
                println!("  ISO:            {}", iso_path);
                println!(
                    "  Compatible:     {}",
                    if analysis.is_compatible { "Yes" } else { "No" }
                );
                println!(
                    "  Boot Manager:   {}",
                    if analysis.has_bootmgr { "Found" } else { "Missing" }
                );
                println!(
                    "  EFI Boot:       {}",
                    if analysis.has_efi_boot { "Found" } else { "Missing" }
                );
                println!(
                    "  Install Image:  {}",
                    if analysis.has_install_image {
                        "Found"
                    } else {
                        "Missing"
                    }
                );
                if let Some(ref ver) = analysis.version {
                    println!("  Windows:        {}", ver);
                }
                if let Some(ref arch) = analysis.architecture {
                    println!("  Architecture:   {}", arch);
                }
                if !analysis.editions.is_empty() {
                    println!(
                        "  Editions:       {}",
                        analysis
                            .editions
                            .iter()
                            .map(|e| format!("{}", e))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                if let Some(img_size) = analysis.install_image_size {
                    let human = humansize::format_size(img_size, humansize::BINARY);
                    println!("  Image Size:     {}", human);
                }
                if !analysis.notes.is_empty() {
                    println!();
                    println!("Notes:");
                    for note in &analysis.notes {
                        println!("  - {}", note);
                    }
                }
            }
        }
        "plan" => {
            let iso_path = opts
                .iso
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--iso is required for plan"))?;
            let _device_path = opts
                .device
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--device is required for plan"))?;

            let scheme = match opts.scheme.as_deref().unwrap_or("gpt") {
                "gpt" => wintogo::WtgPartitionScheme::Gpt,
                "mbr" => wintogo::WtgPartitionScheme::Mbr,
                other => anyhow::bail!("Unknown partition scheme: '{}'. Use 'gpt' or 'mbr'.", other),
            };

            let config = wintogo::WtgConfig {
                iso_path: iso_path.to_string(),
                target_device: _device_path.to_string(),
                partition_scheme: scheme,
                enable_uefi_ntfs: opts.uefi_ntfs,
                apply_san_policy: !opts.no_san_policy,
                disable_recovery: opts.no_recovery,
                ..Default::default()
            };

            let analysis = wintogo::analyze_iso(Path::new(iso_path))?;
            let drive_size = 64u64 * 1024 * 1024 * 1024; // assume 64GB target
            let image_size = analysis.install_image_size.unwrap_or(4 * 1024 * 1024 * 1024);
            let plan = wintogo::plan_partitions(&config, drive_size, image_size)?;

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&plan)?);
            } else {
                let total_human =
                    humansize::format_size(plan.total_required, humansize::BINARY);
                println!("Windows To Go Partition Plan");
                println!("  Scheme:        {}", plan.scheme);
                println!("  ESP:           {} MB", plan.esp_size / (1024 * 1024));
                if plan.msr_size > 0 {
                    println!("  MSR:           {} MB", plan.msr_size / (1024 * 1024));
                }
                println!("  Windows:       {} MB", plan.windows_size / (1024 * 1024));
                if plan.recovery_size > 0 {
                    println!(
                        "  Recovery:      {} MB",
                        plan.recovery_size / (1024 * 1024)
                    );
                }
                println!("  Windows FS:    {}", plan.windows_fs);
                println!("  Total:         {}", total_human);
                println!(
                    "  UEFI:NTFS:     {}",
                    if plan.needs_uefi_ntfs { "Yes" } else { "No" }
                );
            }
        }
        "check-drive" | "check" => {
            let device_path = opts
                .device
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--device is required for check-drive"))?;

            // Get basic device info to pass as removable/size
            let meta = std::fs::metadata(device_path);
            let size = meta.map(|m| m.len()).unwrap_or(0);
            let removable = true; // assume removable for USB

            let check = wintogo::check_drive_attributes(removable, size);

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&check)?);
            } else {
                println!("Windows To Go Drive Check");
                println!("  Device:        {}", device_path);
                println!(
                    "  Fixed disk:    {}",
                    if check.is_fixed { "Yes" } else { "No" }
                );
                println!(
                    "  Large enough:  {}",
                    if check.is_large_enough { "Yes" } else { "No" }
                );
                let size_str = humansize::format_size(check.size, humansize::BINARY);
                let min_str = humansize::format_size(check.min_required_size, humansize::BINARY);
                println!("  Capacity:      {}", size_str);
                println!("  Min required:  {}", min_str);
                if !check.warnings.is_empty() {
                    println!();
                    println!("Warnings:");
                    for w in &check.warnings {
                        println!("  ! {}", w);
                    }
                }
            }
        }
        "san-policy" => {
            let policy = wintogo::generate_san_policy();
            if opts.json {
                println!(
                    "{}",
                    serde_json::json!({ "san_policy_xml": policy })
                );
            } else {
                println!("{}", policy);
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'analyze', 'plan', 'check-drive', or 'san-policy'.",
                other
            );
        }
    }

    Ok(())
}