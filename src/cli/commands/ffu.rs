use anyhow::Result;
use std::io::Cursor;

use crate::cli::FfuOpts;
use crate::core::ffu;

pub async fn execute(opts: FfuOpts) -> Result<()> {
    match opts.action.as_str() {
        "info" | "inspect" => {
            let path = opts.file.as_deref().unwrap_or("");
            if path.is_empty() {
                anyhow::bail!("--file is required for 'info' action");
            }

            let mut file = std::fs::File::open(path)?;
            let info = ffu::FfuInfo::parse(&mut file)?;

            if opts.json {
                let json = serde_json::json!({
                    "security": {
                        "header_size": info.security.header_size,
                        "hash_algorithm": info.security.hash_algorithm,
                        "chunk_count": info.security.chunk_count,
                        "chunk_size": info.security.chunk_size,
                        "catalog_offset": info.security.catalog_offset,
                        "catalog_size": info.security.catalog_size,
                    },
                    "image": {
                        "header_size": info.image.header_size,
                        "manifest_length": info.image.manifest.len(),
                        "platform_ids": info.image.platform_ids,
                    },
                    "stores": info.stores.iter().map(|s| {
                        serde_json::json!({
                            "version": format!("{}.{}", s.major_version, s.minor_version),
                            "update_type": s.update_type,
                            "block_size": s.block_size,
                            "sector_size": s.bytes_per_sector,
                            "sector_count": s.sector_count,
                            "disk_size": s.disk_size(),
                            "payload_size": s.payload_size(),
                            "entry_count": s.block_data_entry_count,
                        })
                    }).collect::<Vec<_>>(),
                    "total_disk_size": info.disk_size(),
                    "total_payload_size": info.total_payload_size(),
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{}", info);
            }
        }

        "detect" | "check" => {
            let path = opts.file.as_deref().unwrap_or("");
            if path.is_empty() {
                anyhow::bail!("--file is required for 'detect' action");
            }

            let mut file = std::fs::File::open(path)?;
            let is = ffu::is_ffu(&mut file)?;

            if opts.json {
                let json = serde_json::json!({
                    "file": path,
                    "is_ffu": is,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{}: {}", path, if is { "FFU image detected" } else { "Not an FFU image" });
            }
        }

        "manifest" => {
            let path = opts.file.as_deref().unwrap_or("");
            if path.is_empty() {
                anyhow::bail!("--file is required for 'manifest' action");
            }

            let mut file = std::fs::File::open(path)?;
            let info = ffu::FfuInfo::parse(&mut file)?;

            if opts.json {
                let json = serde_json::json!({
                    "manifest": info.image.manifest,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("FFU Manifest:");
                println!("{}", info.image.manifest);
            }
        }

        other => {
            anyhow::bail!(
                "Unknown FFU action '{}'. Available: info, detect, manifest",
                other
            );
        }
    }

    Ok(())
}
