use anyhow::{anyhow, Result};
use std::path::PathBuf;

use crate::cli::{parse_block_size, BackupOpts};
use crate::core::backup::{self, BackupCompression, BackupConfig};

pub async fn execute(opts: BackupOpts) -> Result<()> {
    let block_size = parse_block_size(&opts.block_size)?;

    let compression = match opts.compression.as_str() {
        "none" => BackupCompression::None,
        "gzip" | "gz" => BackupCompression::Gzip,
        "zstd" | "zst" => BackupCompression::Zstd,
        "bzip2" | "bz2" => BackupCompression::Bzip2,
        "xz" => BackupCompression::Xz,
        other => {
            return Err(anyhow!(
                "unknown compression '{}'. Use: none, gzip, zstd, bzip2, xz",
                other
            ))
        }
    };

    let output = opts
        .output
        .map(PathBuf::from)
        .unwrap_or_else(|| backup::suggest_output_name(&opts.source, compression));

    let config = BackupConfig {
        source: opts.source.clone(),
        output: output.clone(),
        compression,
        block_size,
        size: None,
        compute_hash: opts.compute_hash,
        sparse: opts.sparse,
        compression_level: opts.compression_level.unwrap_or(3),
        gzip_level: 6,
    };

    println!(
        "Backing up {} -> {} ({})",
        config.source, config.output.display(), config.compression
    );

    let result = backup::backup_drive(&config, |progress, total| {
        if total > 0 {
            let pct = (progress as f64 / total as f64) * 100.0;
            eprint!("\r{:.1}%", pct);
        }
    })?;
    eprintln!();

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print!("{}", result.format_text());
    }

    Ok(())
}