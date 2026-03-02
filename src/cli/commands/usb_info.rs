// CLI command: abt usb-info — show USB connection speed and device info.

use anyhow::Result;

use crate::cli::UsbInfoOpts;
use crate::core::usb_speed;

pub async fn execute(opts: UsbInfoOpts) -> Result<()> {
    let device = opts.device.as_deref().unwrap_or("/dev/sda");
    let info = usb_speed::detect_usb_speed(device)?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&info)?);
        return Ok(());
    }

    println!("USB Device Information");
    println!("══════════════════════");
    println!("  Device:     {}", info.device_path);
    println!("  VID:PID:    {:04X}:{:04X}", info.vid, info.pid);
    println!("  Speed:      {}", info.current_speed);

    if info.speed_degraded {
        println!("  Capability: {} (DEGRADED)", info.device_capability.short_name());
    }

    if let Some(ref serial) = info.serial {
        println!("  Serial:     {}", serial);
    }
    if let Some(bus) = info.bus {
        println!("  Bus:        {}", bus);
    }
    if let Some(port) = info.port {
        println!("  Port:       {}", port);
    }

    if let Some(warning) = info.speed_warning() {
        println!();
        println!("⚠ {}", warning);
    } else if info.current_speed.is_slow_for_imaging() && info.current_speed != usb_speed::UsbSpeed::Unknown {
        println!();
        println!(
            "⚠ Device is connected at {} — writes may be slow for large images",
            info.current_speed
        );
    }

    // Estimate write time for common image sizes
    if info.current_speed != usb_speed::UsbSpeed::Unknown {
        println!();
        println!("Estimated write times:");
        for (label, size) in [
            ("1 GiB", 1_073_741_824u64),
            ("4 GiB", 4_294_967_296),
            ("8 GiB", 8_589_934_592),
        ] {
            let secs = info.estimated_write_secs(size);
            println!("  {} → ~{}", label, usb_speed::format_eta(secs));
        }
    }

    Ok(())
}
