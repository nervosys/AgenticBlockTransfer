use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;

use crate::cli::{parse_block_size, OutputFormat, WriteOpts};
use crate::core::progress::OperationPhase;
use crate::core::safety::{self, SafetyLevel};
use crate::core::types::{HashAlgorithm, ImageSource, WriteConfig, WriteMode};
use crate::core::writer::Writer;

pub async fn execute(opts: WriteOpts, output_format: &OutputFormat) -> Result<()> {
    // Parse safety level
    let safety_level: SafetyLevel = opts
        .safety_level
        .parse()
        .map_err(|e: String| anyhow::anyhow!("{}", e))?;

    // Parse source
    let source = if opts.source.starts_with("http://") || opts.source.starts_with("https://") {
        ImageSource::Url(opts.source.clone())
    } else if opts.source == "-" {
        ImageSource::Stdin
    } else {
        ImageSource::File(PathBuf::from(&opts.source))
    };

    // Parse options
    let block_size = parse_block_size(&opts.block_size)?;

    let mode = match opts.mode.to_lowercase().as_str() {
        "raw" | "dd" => WriteMode::Raw,
        "extract" => WriteMode::Extract,
        "clone" => WriteMode::Clone,
        _ => anyhow::bail!("Unknown write mode: {}", opts.mode),
    };

    let hash_algorithm = match opts.hash_algorithm.to_lowercase().as_str() {
        "md5" => Some(HashAlgorithm::Md5),
        "sha1" => Some(HashAlgorithm::Sha1),
        "sha256" => Some(HashAlgorithm::Sha256),
        "sha512" => Some(HashAlgorithm::Sha512),
        "blake3" => Some(HashAlgorithm::Blake3),
        "crc32" => Some(HashAlgorithm::Crc32),
        _ => Some(HashAlgorithm::Sha256),
    };

    let verify = opts.verify && !opts.no_verify;

    // ── Pre-flight safety checks ───────────────────────────────────────────
    let report = safety::preflight_check(
        &source,
        &opts.target,
        safety_level,
        opts.confirm_token.as_deref(),
        opts.dry_run,
    )
    .await?;

    // Output the safety report
    match output_format {
        OutputFormat::Json | OutputFormat::JsonLd => {
            println!("{}", serde_json::to_string_pretty(&report.to_json())?);
        }
        OutputFormat::Text => {
            report.print_human();
        }
    }

    // Dry-run: stop after safety report
    if opts.dry_run {
        if report.safe_to_proceed {
            eprintln!("Dry-run complete: all pre-flight checks passed.");
            std::process::exit(0);
        } else {
            eprintln!("Dry-run complete: {} blocking error(s) found.", report.errors);
            std::process::exit(safety::ExitCode::SafetyCheckFailed.code());
        }
    }

    // Block if safety checks failed
    if !report.safe_to_proceed {
        if opts.force {
            eprintln!(
                "WARNING: {} safety error(s) detected but --force is set. Proceeding anyway.",
                report.errors
            );
        } else {
            eprintln!(
                "Blocked by safety system ({} error(s)). Use --force to override or --dry-run to inspect.",
                report.errors
            );
            std::process::exit(safety::ExitCode::SafetyCheckFailed.code());
        }
    }

    // ── Interactive confirmation (unless --force or agent token provided) ──
    if !opts.force && opts.confirm_token.is_none() {
        eprintln!("  ALL DATA ON THE TARGET DEVICE WILL BE DESTROYED.");
        eprintln!();
        eprint!("  Continue? [y/N] ");

        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    // ── Partition table backup ─────────────────────────────────────────────
    if opts.backup_partition_table
        || matches!(safety_level, SafetyLevel::High)
    {
        match safety::backup_partition_table(&opts.target).await {
            Ok(path) => {
                eprintln!("  Partition table backed up: {}", path.display());
            }
            Err(e) => {
                if matches!(safety_level, SafetyLevel::High) {
                    anyhow::bail!("High safety level: partition table backup failed: {}", e);
                } else {
                    eprintln!("  Warning: partition table backup failed: {}", e);
                }
            }
        }
    }

    // ── Build write config and execute ─────────────────────────────────────
    let config = WriteConfig {
        source,
        target: opts.target.clone(),
        mode,
        block_size,
        verify,
        hash_algorithm,
        expected_hash: opts.expected_hash,
        force: opts.force,
        direct_io: opts.direct_io,
        sync: opts.sync,
        decompress: opts.decompress,
        sparse: opts.sparse,
    };

    let writer = Writer::new(config);

    // Set up progress bar
    let progress = writer.progress().clone();
    let pb = ProgressBar::new(100);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{bar:40.cyan/blue}] {percent}% | {msg} | {bytes}/{total_bytes} | {bytes_per_sec} | ETA: {eta}",
        )?
        .progress_chars("█▓▒░"),
    );

    // Spawn progress reporter
    let pb_clone = pb.clone();
    let progress_clone = progress.clone();
    let progress_task = tokio::spawn(async move {
        loop {
            let snap = progress_clone.snapshot();
            pb_clone.set_length(snap.bytes_total);
            pb_clone.set_position(snap.bytes_written);
            pb_clone.set_message(format!("{}", snap.phase));

            if matches!(snap.phase, OperationPhase::Completed | OperationPhase::Failed) {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    });

    // Execute
    let start_time = std::time::Instant::now();
    let result = writer.execute().await;
    let elapsed = start_time.elapsed();

    // Wait for progress task to finish
    let _ = progress_task.await;

    match &result {
        Ok(()) => {
            pb.finish_with_message("Complete ✓");

            // Send desktop notification
            crate::core::notify::notify_write_complete(
                &opts.target,
                progress.snapshot().bytes_written,
                elapsed.as_secs_f64(),
            );

            let success = serde_json::json!({
                "success": true,
                "exit_code": 0,
                "device_fingerprint": report.device_fingerprint.as_ref().map(|fp| &fp.token),
            });
            match output_format {
                OutputFormat::Json | OutputFormat::JsonLd => {
                    println!("{}", serde_json::to_string_pretty(&success)?);
                }
                OutputFormat::Text => {
                    eprintln!();
                    eprintln!("Write completed successfully.");
                }
            }
        }
        Err(e) => {
            pb.finish_with_message("Failed ✗");

            // Send desktop notification
            crate::core::notify::notify_write_failed(&opts.target, &e.to_string());
            let exit_code = safety::error_to_exit_code(e);
            match output_format {
                OutputFormat::Json | OutputFormat::JsonLd => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&safety::structured_error(e, exit_code))?
                    );
                }
                OutputFormat::Text => {
                    eprintln!();
                    eprintln!("Write failed: {}", e);
                }
            }
            std::process::exit(exit_code.code());
        }
    }

    result
}
