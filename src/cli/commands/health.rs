use anyhow::{anyhow, Result};

use crate::cli::{parse_block_size, HealthOpts};
use crate::core::health::{self, HealthCheckConfig, TestPattern};

pub async fn execute(opts: HealthOpts) -> Result<()> {
    let block_size = parse_block_size(&opts.block_size)?;

    let pattern = match opts.pattern.as_str() {
        "quick" => TestPattern::Quick,
        "standard" => TestPattern::Standard,
        "slc" => TestPattern::Slc,
        "mlc" => TestPattern::Mlc,
        "tlc" => TestPattern::Tlc,
        other => return Err(anyhow!("unknown pattern '{}'. Use: quick, standard, slc, mlc, tlc", other)),
    };

    match opts.test_type.as_str() {
        "quick" => {
            println!("Running quick read check on {}...", opts.device);
            let meta = std::fs::metadata(&opts.device)?;
            let size = meta.len();
            let (bytes_read, errors, speed) = health::quick_read_check(&opts.device, size, block_size)?;
            if opts.json {
                let result = serde_json::json!({
                    "device": opts.device,
                    "bytes_read": bytes_read,
                    "errors": errors,
                    "speed_bytes_per_sec": speed,
                });
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Bytes read: {}", humansize::format_size(bytes_read, humansize::BINARY));
                println!("Errors:     {}", errors);
                println!("Speed:      {}/s", humansize::format_size(speed as u64, humansize::BINARY));
                if errors == 0 {
                    println!("Result:     HEALTHY");
                } else {
                    println!("Result:     DEGRADED ({} read errors)", errors);
                }
            }
        }
        "badblocks" | "full" => {
            if !opts.force {
                return Err(anyhow!(
                    "Bad block test is DESTRUCTIVE and will erase all data on {}. Use --force to proceed.",
                    opts.device
                ));
            }
            println!("Running {} bad block test on {}...", pattern, opts.device);
            let meta = std::fs::metadata(&opts.device)?;
            let size = meta.len();
            let config = HealthCheckConfig {
                pattern,
                block_size,
                max_bad_blocks: 1000,
                detect_fake: true,
                test_region: None,
            };
            let report = health::check_bad_blocks(
                &opts.device,
                size,
                &config,
                |progress, total, pass, total_passes| {
                    let pct = if total > 0 {
                        (progress as f64 / total as f64) * 100.0
                    } else {
                        0.0
                    };
                    eprint!("\rPass {}/{}: {:.1}%", pass, total_passes, pct);
                },
            )?;
            eprintln!();
            if opts.json {
                println!("{}", report.to_json()?);
            } else {
                print!("{}", report.format_text());
            }
        }
        other => {
            return Err(anyhow!(
                "unknown test type '{}'. Use: quick, badblocks, full",
                other
            ));
        }
    }
    Ok(())
}