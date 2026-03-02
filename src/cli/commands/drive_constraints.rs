// CLI command: drive-constraints -- Validate drive compatibility for an operation

use anyhow::Result;

use crate::cli::DriveConstraintsOpts;
use crate::core::drive_constraints;

pub async fn execute(opts: DriveConstraintsOpts) -> Result<()> {
    match opts.action.as_str() {
        "validate" => {
            let device_path = opts
                .device
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--device is required for validate"))?;
            let source_path = opts.source.clone();
            let min_size: u64 = opts.min_size.unwrap_or(0);

            let config = drive_constraints::ConstraintConfig {
                protect_system_drives: !opts.allow_system,
                minimum_size: min_size,
                source_path,
                warn_non_removable: true,
                warn_mounted: true,
                ..Default::default()
            };

            // Build a DeviceInfo from the device path for validation
            let dev = crate::core::device::DeviceInfo {
                path: device_path.to_string(),
                name: device_path.to_string(),
                vendor: String::new(),
                serial: None,
                size: std::fs::metadata(device_path)
                    .map(|m| m.len())
                    .unwrap_or(0),
                sector_size: 512,
                physical_sector_size: 512,
                removable: true,
                read_only: false,
                is_system: false,
                device_type: crate::core::types::DeviceType::Usb,
                mount_points: Vec::new(),
                transport: String::new(),
            };

            let report = drive_constraints::validate_drive(&dev, &config);

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Drive Validation Report");
                println!("  Device: {}", device_path);
                println!("  Status: {}", report.status);
                println!();
                for r in &report.constraints {
                    let icon = if r.passed { "\u{2713}" } else { "\u{2717}" };
                    let sev = match r.severity {
                        drive_constraints::ConstraintSeverity::Info => "info",
                        drive_constraints::ConstraintSeverity::Warning => "warn",
                        drive_constraints::ConstraintSeverity::Error => "error",
                    };
                    println!("  [{}] {} ({}): {}", icon, r.name, sev, r.message);
                }
                println!();
                let failures = report.failures();
                if failures.is_empty() {
                    println!("  Result: PASS -- drive is compatible");
                } else {
                    println!("  Result: FAIL -- {} constraint(s) violated", failures.len());
                }
            }
        }
        "auto-select" | "autoselect" => {
            let source_path = opts.source.clone();
            let min_size: u64 = opts.min_size.unwrap_or(0);

            let config = drive_constraints::ConstraintConfig {
                protect_system_drives: !opts.allow_system,
                minimum_size: min_size,
                source_path,
                ..Default::default()
            };

            // Enumerate all drives via the platform enumerator
            let enumerator = crate::core::device::create_enumerator();
            let all_devices = enumerator.list_devices().await?;

            let selected = drive_constraints::auto_select_drive(&all_devices, &config);
            if let Some((dev, _report)) = selected {
                if opts.json {
                    println!("{}", serde_json::to_string_pretty(&dev)?);
                } else {
                    let size_str = humansize::format_size(dev.size, humansize::BINARY);
                    println!("Auto-selected drive:");
                    println!("  Path:   {}", dev.path);
                    println!("  Name:   {}", dev.name);
                    println!("  Size:   {}", size_str);
                    if !dev.vendor.is_empty() {
                        println!("  Vendor: {}", dev.vendor);
                    }
                }
            } else if opts.json {
                println!("null");
            } else {
                println!("No suitable drive found for auto-selection.");
            }
        }
        "check-all" | "checkall" => {
            let source_path = opts.source.clone();
            let min_size: u64 = opts.min_size.unwrap_or(0);

            let config = drive_constraints::ConstraintConfig {
                protect_system_drives: !opts.allow_system,
                minimum_size: min_size,
                source_path,
                ..Default::default()
            };

            let enumerator = crate::core::device::create_enumerator();
            let all_devices = enumerator.list_devices().await?;
            let reports = drive_constraints::validate_drives(&all_devices, &config);

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&reports)?);
            } else {
                println!("Drive Compatibility Report ({} drives):", reports.len());
                println!();
                for report in &reports {
                    println!(
                        "  {} -- {}",
                        report.device_path, report.status
                    );
                    for r in report.failures() {
                        println!("    X {}: {}", r.name, r.message);
                    }
                }
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'validate', 'auto-select', or 'check-all'.",
                other
            );
        }
    }

    Ok(())
}