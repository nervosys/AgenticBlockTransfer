// CLI command: abt checksum-file — parse and verify against checksum files.

use anyhow::Result;
use log::info;

use crate::cli::ChecksumFileOpts;
use crate::core::checksum_file;

pub async fn execute(opts: ChecksumFileOpts) -> Result<()> {
    // Load the checksum file (from URL or local path)
    let cf = if opts.checksum_file.starts_with("http://") || opts.checksum_file.starts_with("https://") {
        checksum_file::fetch_checksum_file(&opts.checksum_file).await?
    } else {
        checksum_file::load_checksum_file(std::path::Path::new(&opts.checksum_file))?
    };

    info!(
        "Loaded {} entries from {} ({})",
        cf.len(),
        cf.source_file,
        cf.algorithm
            .map(|a| a.name())
            .unwrap_or("unknown algorithm")
    );

    // Lookup mode: just find and print a specific entry
    if let Some(ref lookup) = opts.lookup {
        if let Some(entry) = cf.find_hash(lookup) {
            if opts.json {
                println!("{}", serde_json::to_string_pretty(entry)?);
            } else {
                println!("{}  {}", entry.hash, entry.filename);
            }
        } else {
            anyhow::bail!("No entry found for '{}' in {}", lookup, opts.checksum_file);
        }
        return Ok(());
    }

    // If no files specified and no lookup, just list the checksum file contents
    if opts.files.is_empty() {
        if opts.json {
            println!("{}", serde_json::to_string_pretty(&cf)?);
        } else {
            println!(
                "{} ({} entries, {}):",
                cf.source_file,
                cf.len(),
                cf.algorithm
                    .map(|a| a.name())
                    .unwrap_or("unknown")
            );
            for entry in &cf.entries {
                println!("  {}  {}", entry.hash, entry.filename);
            }
        }
        return Ok(());
    }

    // Verify mode: check each file against the checksum file
    let mut all_passed = true;
    let mut results = Vec::new();

    for file_path in &opts.files {
        let path = std::path::Path::new(file_path);
        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(file_path);

        if let Some(entry) = cf.find_hash(filename) {
            match checksum_file::verify_file_checksum(path, entry).await {
                Ok(true) => {
                    if opts.json {
                        results.push(serde_json::json!({
                            "file": file_path,
                            "expected": entry.hash,
                            "algorithm": entry.algorithm.name(),
                            "status": "OK"
                        }));
                    } else {
                        println!("✓ {} — {} OK", filename, entry.algorithm.name());
                    }
                }
                Ok(false) => {
                    all_passed = false;
                    if opts.json {
                        results.push(serde_json::json!({
                            "file": file_path,
                            "expected": entry.hash,
                            "algorithm": entry.algorithm.name(),
                            "status": "MISMATCH"
                        }));
                    } else {
                        println!("✗ {} — {} MISMATCH", filename, entry.algorithm.name());
                    }
                }
                Err(e) => {
                    all_passed = false;
                    if opts.json {
                        results.push(serde_json::json!({
                            "file": file_path,
                            "status": "ERROR",
                            "error": e.to_string()
                        }));
                    } else {
                        println!("✗ {} — ERROR: {}", filename, e);
                    }
                }
            }
        } else {
            all_passed = false;
            if opts.json {
                results.push(serde_json::json!({
                    "file": file_path,
                    "status": "NOT_FOUND",
                    "error": "No matching entry in checksum file"
                }));
            } else {
                println!("? {} — not found in {}", filename, cf.source_file);
            }
        }
    }

    if opts.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "checksum_file": cf.source_file,
                "algorithm": cf.algorithm.map(|a| a.name()),
                "results": results,
                "all_passed": all_passed
            }))?
        );
    }

    if !all_passed {
        anyhow::bail!("One or more files failed verification");
    }

    Ok(())
}
