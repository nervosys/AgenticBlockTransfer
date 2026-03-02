// CLI command: fleet — Multi-target fleet write

use anyhow::Result;

use crate::cli::FleetOpts;
use crate::core::fleet;

pub async fn execute(opts: FleetOpts) -> Result<()> {
    match opts.action.as_str() {
        "detect" => {
            let devices = fleet::detect_usb_devices()?;
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&devices)?);
            } else if devices.is_empty() {
                println!("No USB mass storage devices detected.");
                println!("Note: Device detection requires elevated privileges.");
            } else {
                println!("Available USB devices:");
                for d in &devices {
                    println!(
                        "  {} — {} ({})",
                        d.path,
                        d.label,
                        fleet::format_speed(d.capacity)
                    );
                }
            }
        }
        "validate" => {
            let source = opts
                .source
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--source is required for validate"))?;
            let image_size = std::fs::metadata(source)
                .map(|m| m.len())
                .unwrap_or(0);

            let targets: Vec<fleet::FleetTarget> = opts
                .targets
                .iter()
                .enumerate()
                .map(|(i, path)| fleet::FleetTarget {
                    path: path.clone(),
                    label: format!("Device {}", i + 1),
                    capacity: 0, // unknown without elevated access
                })
                .collect();

            let results = fleet::validate_targets(&targets, image_size);

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&results)?);
            } else {
                for r in &results {
                    if r.valid {
                        println!("  {} — OK", r.device_path);
                    } else {
                        println!("  {} — INVALID:", r.device_path);
                        for issue in &r.issues {
                            println!("    - {}", issue);
                        }
                    }
                }
            }
        }
        "status" => {
            // In a real implementation, this would connect to a running fleet session
            println!("No fleet write session is currently active.");
            println!("Start one with: abt fleet write --source <image> --target <dev1> --target <dev2>");
        }
        other => {
            anyhow::bail!(
                "Unknown action: {}. Use 'detect', 'validate', or 'status'.",
                other
            );
        }
    }

    Ok(())
}
