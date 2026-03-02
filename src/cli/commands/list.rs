use anyhow::Result;
use serde_json::json;

use crate::cli::{ListOpts, OutputFormat};
use crate::core::device;
use crate::core::safety::DeviceFingerprint;
use crate::core::types::DeviceType;

pub async fn execute(opts: ListOpts, output_format: &OutputFormat) -> Result<()> {
    let enumerator = device::create_enumerator();
    let mut devices = enumerator.list_devices().await?;

    // Filter
    if !opts.all {
        devices.retain(|d| !d.is_system);
    }

    if opts.removable {
        devices.retain(|d| d.is_removable_media());
    }

    if let Some(ref dtype) = opts.device_type {
        let filter_type = match dtype.to_lowercase().as_str() {
            "usb" => Some(DeviceType::Usb),
            "sd" => Some(DeviceType::Sd),
            "nvme" => Some(DeviceType::Nvme),
            "sata" => Some(DeviceType::Sata),
            "scsi" => Some(DeviceType::Scsi),
            "mmc" | "emmc" => Some(DeviceType::Mmc),
            "virtual" => Some(DeviceType::Virtual),
            _ => None,
        };

        if let Some(ft) = filter_type {
            devices.retain(|d| d.device_type == ft);
        }
    }

    // JSON output with fingerprints (for agent consumption)
    if opts.json || matches!(output_format, OutputFormat::Json | OutputFormat::JsonLd) {
        let device_list: Vec<_> = devices
            .iter()
            .map(|dev| {
                let fp = DeviceFingerprint::from_device(dev);
                json!({
                    "path": dev.path,
                    "name": dev.name,
                    "vendor": dev.vendor,
                    "serial": dev.serial,
                    "size": dev.size,
                    "size_human": humansize::format_size(dev.size, humansize::BINARY),
                    "sector_size": dev.sector_size,
                    "removable": dev.removable,
                    "read_only": dev.read_only,
                    "is_system": dev.is_system,
                    "device_type": format!("{}", dev.device_type),
                    "mount_points": dev.mount_points,
                    "transport": dev.transport,
                    "safe_target": dev.is_safe_target(),
                    "removable_media": dev.is_removable_media(),
                    "confirm_token": fp.token,
                })
            })
            .collect();

        let output = json!({
            "devices": device_list,
            "count": device_list.len(),
            "filters": {
                "all": opts.all,
                "removable": opts.removable,
                "device_type": opts.device_type,
            }
        });

        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    // Text output
    if devices.is_empty() {
        eprintln!("No devices found.");
        if !opts.all {
            eprintln!("Hint: Use --all to include system drives.");
        }
        return Ok(());
    }

    // Print header
    println!(
        "{:<25} {:<30} {:>12} {:<8} {:<10} {}",
        "DEVICE", "NAME", "SIZE", "TYPE", "REMOVABLE", "MOUNT POINTS"
    );
    println!("{}", "-".repeat(100));

    for dev in &devices {
        let size = humansize::format_size(dev.size, humansize::BINARY);
        let removable = if dev.removable { "Yes" } else { "No" };
        let mounts = if dev.mount_points.is_empty() {
            "-".to_string()
        } else {
            dev.mount_points.join(", ")
        };
        let system_marker = if dev.is_system { " [SYSTEM]" } else { "" };
        let fp = DeviceFingerprint::from_device(dev);

        println!(
            "{:<25} {:<30} {:>12} {:<8} {:<10} {}{}",
            dev.path, dev.name, size, dev.device_type, removable, mounts, system_marker
        );
        println!(
            "  └─ confirm-token: {}",
            fp.token
        );
    }

    println!();
    println!("{} device(s) found", devices.len());
    println!("Hint: Use --json for machine-readable output with device fingerprints.");

    Ok(())
}
