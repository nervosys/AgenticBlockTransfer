// CLI command: fs-detect — Detect filesystem type from device/image superblock

use anyhow::Result;

use std::path::Path;
use crate::cli::FsDetectOpts;
use crate::core::fs_detect;

pub async fn execute(opts: FsDetectOpts) -> Result<()> {
    match opts.action.as_str() {
        "detect" => {
            let result = fs_detect::detect_filesystem_path(Path::new(&opts.device))?;
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Filesystem Detection");
                println!("  Device:     {}", opts.device);
                println!("  Type:       {}", result.fs_type);
                if let Some(ref label) = result.volume_label {
                    println!("  Label:      {}", label);
                }
                if let Some(ref oem) = result.oem_name {
                    println!("  OEM:        {}", oem);
                }
                if let Some(ref id) = result.volume_id {
                    println!("  Volume ID:  {}", id);
                }
                if let Some(bs) = result.block_size {
                    println!("  Block Size: {} bytes", bs);
                }
                if let Some(ts) = result.total_size {
                    let human = humansize::format_size(ts, humansize::BINARY);
                    println!("  Total Size: {}", human);
                }
                if !result.magic_hex.is_empty() {
                    println!("  Magic:      {}", result.magic_hex);
                }
                println!("  Confidence: {:.0}%", result.confidence * 100.0);
                println!();

                // Properties
                let ft = &result.fs_type;
                println!("Properties:");
                println!("  Writable:       {}", if ft.is_writable() { "Yes" } else { "No" });
                println!("  Windows native: {}", if ft.is_windows_native() { "Yes" } else { "No" });
                println!("  Linux native:   {}", if ft.is_linux_native() { "Yes" } else { "No" });
                println!("  macOS native:   {}", if ft.is_macos_native() { "Yes" } else { "No" });
                if let Some(max) = ft.max_file_size() {
                    let human = humansize::format_size(max, humansize::BINARY);
                    println!("  Max file size:  {}", human);
                }
            }
        }
        "probe" => {
            // Quick probe that just prints the type
            let result = fs_detect::detect_filesystem_path(Path::new(&opts.device))?;
            if opts.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "device": opts.device,
                        "type": format!("{}", result.fs_type),
                        "confidence": result.confidence,
                    })
                );
            } else {
                println!("{}", result.fs_type);
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'detect' or 'probe'.",
                other
            );
        }
    }

    Ok(())
}
