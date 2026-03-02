// CLI command: uefi-ntfs — UEFI:NTFS dual-partition layout analysis and planning

use anyhow::Result;

use crate::cli::UefiNtfsOpts;
use crate::core::uefi_ntfs;

pub async fn execute(opts: UefiNtfsOpts) -> Result<()> {
    match opts.action.as_str() {
        "analyze" => {
            let path = opts
                .path
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--path is required for analyze"))?;
            let analysis = uefi_ntfs::analyze_directory(std::path::Path::new(path))?;

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&analysis)?);
            } else {
                println!("Layout Analysis:");
                println!("  Files: {}", analysis.file_count);
                println!("  Total size: {}", uefi_ntfs::format_size(analysis.total_size));
                println!(
                    "  Largest file: {} ({})",
                    analysis.largest_file_name,
                    uefi_ntfs::format_size(analysis.largest_file_size)
                );
                println!("  Has files > 4 GB: {}", analysis.has_large_files);
                println!("  Needs UEFI:NTFS: {}", analysis.needs_uefi_ntfs);
                println!("  Recommended FS: {}", analysis.recommended_filesystem);
                println!("  Recommended boot: {}", analysis.recommended_boot_mode);
            }
        }
        "plan" => {
            let size_gb = opts.disk_size_gb.unwrap_or(16);
            let disk_size = size_gb * 1024 * 1024 * 1024;
            let boot_mode = match opts.boot_mode.as_deref().unwrap_or("uefi") {
                "bios" => uefi_ntfs::BootMode::Bios,
                "uefi" => uefi_ntfs::BootMode::Uefi,
                "dual" => uefi_ntfs::BootMode::Dual,
                other => anyhow::bail!("Unknown boot mode: {}", other),
            };
            let large_files = opts.large_files;

            let layout = uefi_ntfs::choose_layout(disk_size, boot_mode, large_files, opts.wtg)?;

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&layout)?);
            } else {
                println!("Disk Layout Plan:");
                println!("  Scheme: {}", layout.scheme);
                println!("  Boot mode: {}", layout.boot_mode);
                println!("  UEFI:NTFS: {}", layout.uses_uefi_ntfs);
                println!("  Data FS: {}", layout.data_filesystem);
                println!("  Windows-To-Go: {}", layout.windows_to_go);
                println!("  Partitions:");
                for p in &layout.partitions {
                    println!(
                        "    [{}] {} — {} @ offset 0x{:X}, size {}",
                        p.index,
                        p.label,
                        p.filesystem,
                        p.offset,
                        uefi_ntfs::format_size(p.size)
                    );
                }
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: {}. Use 'analyze' or 'plan'.",
                other
            );
        }
    }

    Ok(())
}
