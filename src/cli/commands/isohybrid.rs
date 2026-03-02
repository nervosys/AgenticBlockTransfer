use anyhow::Result;

use crate::cli::IsoHybridOpts;
use crate::core::isohybrid;

pub async fn execute(opts: IsoHybridOpts) -> Result<()> {
    match opts.action.as_str() {
        "detect" | "check" | "analyze" => {
            let path = opts.file.as_deref().unwrap_or("");
            if path.is_empty() {
                anyhow::bail!("--file is required for 'detect' action");
            }

            let mut file = std::fs::File::open(path)?;
            let info = isohybrid::detect(&mut file)?;

            if opts.json {
                let json = serde_json::json!({
                    "file": path,
                    "is_hybrid": info.is_hybrid,
                    "hybrid_type": format!("{}", info.hybrid_type),
                    "recommended_mode": format!("{}", info.recommended_mode),
                    "has_mbr_signature": info.has_mbr_signature,
                    "has_isolinux_magic": info.has_isolinux_magic,
                    "has_gpt_protective": info.has_gpt_protective,
                    "has_gpt_header": info.has_gpt_header,
                    "has_el_torito": info.has_el_torito,
                    "is_windows_iso": info.is_windows_iso,
                    "active_partitions": info.active_partition_count,
                    "partitions": info.partitions.iter().enumerate()
                        .filter(|(_, p)| !p.is_empty())
                        .map(|(i, p)| serde_json::json!({
                            "index": i,
                            "type": format!("{:#04x}", p.partition_type),
                            "bootable": p.is_bootable(),
                            "start_lba": p.start_lba,
                            "sectors": p.sector_count,
                            "size_bytes": p.size_bytes(),
                        }))
                        .collect::<Vec<_>>(),
                    "file_size": info.file_size,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{}", info);
                println!();
                println!("Summary: {}", info.summary());
            }
        }

        "mode" | "recommend" => {
            let path = opts.file.as_deref().unwrap_or("");
            if path.is_empty() {
                anyhow::bail!("--file is required for 'mode' action");
            }

            let mut file = std::fs::File::open(path)?;
            let info = isohybrid::detect(&mut file)?;

            if opts.json {
                let json = serde_json::json!({
                    "file": path,
                    "recommended_mode": format!("{}", info.recommended_mode),
                    "is_hybrid": info.is_hybrid,
                    "hybrid_type": format!("{}", info.hybrid_type),
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("File: {}", path);
                println!("Write mode recommendation: {}", info.recommended_mode);
                if info.is_hybrid {
                    println!("Hybrid type: {}", info.hybrid_type);
                    println!();
                    println!("This ISO can be written directly as a raw disk image (dd mode).");
                } else {
                    println!();
                    println!("This ISO should be written by extracting files to a bootable partition.");
                }
            }
        }

        other => {
            anyhow::bail!(
                "Unknown isohybrid action '{}'. Available: detect, mode",
                other
            );
        }
    }

    Ok(())
}
