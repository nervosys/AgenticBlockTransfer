// CLI command: wim-extract — Extract files from WIM archives

use anyhow::Result;
use std::path::Path;

use crate::cli::WimExtractOpts;
use crate::core::wim_extract;

pub async fn execute(opts: WimExtractOpts) -> Result<()> {
    match opts.action.as_str() {
        "info" | "header" => {
            let summary = wim_extract::read_wim_header(Path::new(&opts.wim_file))?;
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else {
                println!("WIM File: {}", summary.wim_path);
                println!("  Size:        {} bytes", summary.file_size);
                println!("  Compression: {}", summary.compression);
                println!("  Version:     0x{:X}", summary.version);
                println!("  Images:      {}", summary.image_count);
                if summary.total_parts > 1 {
                    println!("  Part:        {} of {}", summary.part_number, summary.total_parts);
                }
                for img in &summary.images {
                    println!("  Image {}:", img.index);
                    println!("    Name: {}", img.name);
                    if let Some(ref ed) = img.edition {
                        println!("    Edition: {}", ed);
                    }
                    if let Some(ref arch) = img.arch {
                        println!("    Arch: {}", arch);
                    }
                }
            }
        }
        "list" | "ls" => {
            let options = wim_extract::ExtractOptions {
                image_index: opts.image_index,
                include_patterns: opts.include.clone(),
                exclude_patterns: opts.exclude.clone(),
                dry_run: true, // listing only
                ..Default::default()
            };
            let files = wim_extract::list_files(Path::new(&opts.wim_file), &options)?;
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&files)?);
            } else if files.is_empty() {
                println!("No files found (WIM metadata extraction not yet implemented).");
                println!("Use 'info' to view WIM header details.");
            } else {
                for entry in &files {
                    let kind = if entry.is_dir { "DIR " } else { "FILE" };
                    println!("{} {:>12} {}", kind, entry.size_human(), entry.path);
                }
            }
        }
        "extract" | "x" => {
            let output_dir = opts.output_dir.as_deref().unwrap_or(".");
            let options = wim_extract::ExtractOptions {
                image_index: opts.image_index,
                include_patterns: opts.include.clone(),
                exclude_patterns: opts.exclude.clone(),
                overwrite: opts.overwrite,
                flatten: opts.flatten,
                max_files: opts.max_files,
                dry_run: opts.dry_run,
                ..Default::default()
            };

            let result = wim_extract::extract(
                Path::new(&opts.wim_file),
                Path::new(output_dir),
                &options,
            )?;

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                if result.dry_run {
                    println!("DRY RUN — no files extracted");
                }
                println!("Extracted from: {}", result.wim_path);
                println!("  Image index:  {}", result.image_index);
                println!("  Output:       {}", result.output_dir);
                println!("  Files:        {}", result.files_extracted);
                println!("  Directories:  {}", result.dirs_created);
                println!("  Bytes:        {}", result.bytes_extracted);
                println!("  Duration:     {:.2}s", result.duration_secs);
                if !result.skipped.is_empty() {
                    println!("  Skipped:      {}", result.skipped.len());
                }
                if !result.errors.is_empty() {
                    println!("  Errors:");
                    for e in &result.errors {
                        println!("    - {}", e);
                    }
                }
            }
        }
        "check" => {
            let path = Path::new(&opts.wim_file);
            let is_wim = wim_extract::is_wim_file(path)?;
            if opts.json {
                println!("{{\"is_wim\": {}, \"path\": \"{}\"}}", is_wim, opts.wim_file);
            } else if is_wim {
                println!("{} is a valid WIM file", opts.wim_file);
            } else {
                println!("{} is NOT a WIM file", opts.wim_file);
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'info', 'list', 'extract', or 'check'.",
                other
            );
        }
    }

    Ok(())
}
