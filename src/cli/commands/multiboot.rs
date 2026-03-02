// CLI command: multiboot — manage multi-boot USB devices

use anyhow::Result;
use std::path::Path;

use crate::core::multiboot;

pub async fn execute(opts: crate::cli::MultibootOpts) -> Result<()> {
    match opts.action.as_str() {
        "add" => {
            let mount = opts.mount_point.as_deref().unwrap_or(".");
            let iso = opts
                .iso
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--iso is required for 'add' action"))?;

            let result =
                multiboot::add_image(Path::new(mount), Path::new(iso), opts.name.as_deref())?;

            if opts.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "action": "add",
                        "image_count": result.image_count,
                        "total_size": result.total_size,
                        "message": result.message,
                    })
                );
            } else {
                println!("{}", result.message);
            }
        }
        "remove" => {
            let mount = opts.mount_point.as_deref().unwrap_or(".");
            let iso = opts
                .iso
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("--iso is required for 'remove' action"))?;

            let filename = Path::new(iso)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let result = multiboot::remove_image(Path::new(mount), &filename)?;

            if opts.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "action": "remove",
                        "image_count": result.image_count,
                        "total_size": result.total_size,
                        "message": result.message,
                    })
                );
            } else {
                println!("{}", result.message);
            }
        }
        "list" => {
            let mount = opts.mount_point.as_deref().unwrap_or(".");
            let images = multiboot::list_images(Path::new(mount))?;

            if opts.json {
                println!("{}", serde_json::to_string_pretty(&images)?);
            } else {
                println!("{}", multiboot::format_listing(&images));
            }
        }
        "grub" => {
            let mount = opts.mount_point.as_deref().unwrap_or(".");
            let registry = multiboot::read_registry(Path::new(mount))?;
            let cfg = multiboot::generate_grub_config(&registry);
            println!("{}", cfg);
        }
        _ => {
            anyhow::bail!(
                "Unknown multiboot action: '{}'. Use add, remove, list, or grub.",
                opts.action
            );
        }
    }

    Ok(())
}
