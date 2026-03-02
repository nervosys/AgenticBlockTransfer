use anyhow::Result;

use crate::cli::FormatOpts;
use crate::core::format;
use crate::core::types::Filesystem;
use crate::platform;

pub async fn execute(opts: FormatOpts) -> Result<()> {
    if !platform::is_elevated() {
        eprintln!("WARNING: Formatting requires elevated privileges.");
    }

    let filesystem = match opts.filesystem.to_lowercase().as_str() {
        "fat16" => Filesystem::Fat16,
        "fat32" | "vfat" => Filesystem::Fat32,
        "exfat" => Filesystem::ExFat,
        "ntfs" => Filesystem::Ntfs,
        "ext2" => Filesystem::Ext2,
        "ext3" => Filesystem::Ext3,
        "ext4" => Filesystem::Ext4,
        "xfs" => Filesystem::Xfs,
        "btrfs" => Filesystem::Btrfs,
        "none" | "zero" => Filesystem::None,
        _ => anyhow::bail!("Unknown filesystem: {}", opts.filesystem),
    };

    if !opts.force {
        eprintln!("WARNING: This will erase ALL data on {}", opts.device);
        eprint!("Continue? [y/N] ");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    format::format_device(&opts.device, filesystem, opts.label.as_deref(), opts.quick).await?;

    println!("Device {} formatted as {}", opts.device, filesystem);
    Ok(())
}
