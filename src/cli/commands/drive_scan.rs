// CLI command: drive-scan -- Scan for attached drives with hot-plug detection

use anyhow::Result;

use crate::cli::DriveScanOpts;
use crate::core::drive_scan;

pub async fn execute(opts: DriveScanOpts) -> Result<()> {
    match opts.action.as_str() {
        "scan" | "list" => {
            let config = drive_scan::ScannerConfig {
                include_system_drives: opts.include_system,
                include_read_only: opts.include_readonly,
                min_size: opts.min_size.unwrap_or(0),
                max_size: opts.max_size.unwrap_or(0),
                ..Default::default()
            };
            let scanner = drive_scan::DriveScanner::new(config);
            let snapshot = scanner.snapshot()?;
            let devices = &snapshot.devices;

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&devices)?);
            } else if devices.is_empty() {
                println!("No removable drives found.");
            } else {
                println!("Detected Drives ({}):", devices.len());
                println!(
                    "{:<20} {:<12} {:<16} {:<10} {}",
                    "PATH", "SIZE", "VENDOR", "TYPE", "NAME"
                );
                println!("{}", "-".repeat(72));
                for dev in devices {
                    let size_str =
                        humansize::format_size(dev.size, humansize::BINARY);
                    let dev_type = if dev.removable {
                        "removable"
                    } else {
                        "fixed"
                    };
                    println!(
                        "{:<20} {:<12} {:<16} {:<10} {}",
                        dev.path, size_str, dev.vendor, dev_type, dev.name
                    );
                }
            }
        }
        "watch" => {
            let poll_ms = opts.poll_interval.unwrap_or(2000);
            let config = drive_scan::ScannerConfig {
                poll_interval: std::time::Duration::from_millis(poll_ms),
                include_system_drives: opts.include_system,
                include_read_only: opts.include_readonly,
                ..Default::default()
            };
            let scanner = drive_scan::DriveScanner::new(config);
            let mut rx = scanner.subscribe();
            let _handle = scanner.start().await?;

            println!("Watching for drive events (Ctrl+C to stop)...");
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if opts.json {
                            println!("{}", serde_json::to_string(&event)?);
                        } else {
                            println!(
                                "[{}ms] {:?}: {} ({})",
                                event.timestamp_ms,
                                event.event_type,
                                event.device.name,
                                event.device.path
                            );
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        "snapshot" => {
            let config = drive_scan::ScannerConfig::default();
            let scanner = drive_scan::DriveScanner::new(config);
            let snapshot = scanner.snapshot()?;

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&snapshot)?);
            } else {
                println!(
                    "Snapshot ({}ms since start):",
                    snapshot.timestamp_ms,
                );
                println!("  Drives: {}", snapshot.devices.len());
                for dev in &snapshot.devices {
                    let size_str =
                        humansize::format_size(dev.size, humansize::BINARY);
                    println!("    {} -- {} ({})", dev.path, dev.name, size_str);
                }
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'scan', 'watch', or 'snapshot'.",
                other
            );
        }
    }

    Ok(())
}