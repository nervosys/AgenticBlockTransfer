use anyhow::Result;

use crate::cli::OpticalOpts;
use crate::core::optical;

pub async fn execute(opts: OpticalOpts) -> Result<()> {
    match opts.action.as_str() {
        "list" | "detect" | "drives" => {
            let drives = optical::detect_drives()?;

            if opts.json {
                let json = serde_json::json!({
                    "drives": drives,
                    "count": drives.len(),
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else if drives.is_empty() {
                println!("No optical drives detected.");
            } else {
                println!("Detected optical drives:");
                for drive in &drives {
                    println!("  • {}", drive);
                }
            }
        }

        "info" | "inspect" => {
            let device = opts.device.as_deref().unwrap_or("");
            if device.is_empty() {
                anyhow::bail!("--device is required for 'info' action");
            }

            let info = optical::get_disc_info(device)?;

            if opts.json {
                let json = serde_json::json!({
                    "device": info.device_path,
                    "type": format!("{}", info.disc_type),
                    "sectors": info.sector_count,
                    "sector_size": info.sector_size,
                    "total_size": info.total_size,
                    "volume_label": info.volume_label,
                    "is_blank": info.is_blank,
                    "is_multi_session": info.is_multi_session,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{}", info);
            }
        }

        "read" | "save" | "rip" => {
            let device = opts.device.as_deref().unwrap_or("");
            if device.is_empty() {
                anyhow::bail!("--device is required for 'read' action");
            }

            let output = opts.output.as_deref().unwrap_or("disc.iso");

            let config = optical::ReadConfig {
                device: device.to_string(),
                output: std::path::PathBuf::from(output),
                buffer_sectors: opts.buffer_sectors.unwrap_or(64) as usize,
                max_retries: opts.retries.unwrap_or(3),
                skip_errors: opts.skip_errors,
                verify: !opts.no_verify,
                overwrite: opts.overwrite,
            };

            println!("Reading disc from {} → {}", device, output);

            let progress_cb: Option<optical::ProgressCallback> = if !opts.json {
                Some(Box::new(|p: optical::ReadProgress| {
                    eprint!(
                        "\r  {:.1}% — {} / {} MiB — {:.1} MiB/s — ETA {:.0}s — {} errors",
                        p.percent(),
                        p.bytes_read / (1024 * 1024),
                        p.bytes_total / (1024 * 1024),
                        p.speed_bps as f64 / (1024.0 * 1024.0),
                        p.eta_seconds,
                        p.error_count,
                    );
                }))
            } else {
                None
            };

            let result = optical::read_disc(&config, progress_cb)?;

            if !opts.json {
                eprintln!(); // newline after progress
            }

            if opts.json {
                let json = serde_json::json!({
                    "output": result.output_path.display().to_string(),
                    "bytes_read": result.bytes_read,
                    "sectors_read": result.sectors_read,
                    "error_sectors": result.error_sectors,
                    "success": result.success,
                    "duration_seconds": result.duration_seconds,
                    "avg_speed_bps": result.avg_speed_bps,
                    "sha256": result.sha256,
                });
                println!("{}", serde_json::to_string_pretty(&json)?);
            } else {
                println!("{}", result);
            }
        }

        other => {
            anyhow::bail!(
                "Unknown optical action '{}'. Available: list, info, read",
                other
            );
        }
    }

    Ok(())
}
