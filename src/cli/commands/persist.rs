use anyhow::{anyhow, Result};

use crate::cli::PersistOpts;
use crate::core::persist::{self, PersistConfig, PersistFs, PersistenceMode};

pub async fn execute(opts: PersistOpts) -> Result<()> {
    let filesystem = match opts.filesystem.as_str() {
        "ext4" => PersistFs::Ext4,
        "ext3" => PersistFs::Ext3,
        "btrfs" => PersistFs::Btrfs,
        "fat32" | "vfat" => PersistFs::Fat32,
        "ntfs" => PersistFs::Ntfs,
        "exfat" => PersistFs::ExFat,
        other => {
            return Err(anyhow!(
                "unknown filesystem '{}'. Use: ext4, ext3, btrfs, fat32, ntfs, exfat",
                other
            ))
        }
    };

    let mode = match opts.mode.as_str() {
        "casper" | "ubuntu" => PersistenceMode::Casper,
        "fedora" => PersistenceMode::FedoraOverlay,
        "generic" | "persistence" => PersistenceMode::PersistenceConf,
        "ventoy" => PersistenceMode::VentoyDat,
        other => {
            return Err(anyhow!(
                "unknown mode '{}'. Use: casper, fedora, generic, ventoy",
                other
            ))
        }
    };

    let label = opts
        .label
        .unwrap_or_else(|| mode.expected_label().to_string());

    // Parse size (e.g., "4G", "1G", "512M") — 0 = all remaining
    let size = if opts.size == "0" {
        0u64
    } else {
        let s = opts.size.trim().to_uppercase();
        if let Some(num) = s.strip_suffix('G') {
            num.parse::<u64>()? * 1024 * 1024 * 1024
        } else if let Some(num) = s.strip_suffix('M') {
            num.parse::<u64>()? * 1024 * 1024
        } else {
            s.parse::<u64>()?
        }
    };

    // Handle image file mode (Ventoy-style file-based persistence)
    if let Some(img_path) = &opts.image_file {
        let img_size = if size == 0 {
            // Default to 4 GiB for file-based persistence
            4 * 1024 * 1024 * 1024
        } else {
            size
        };
        let min = persist::recommended_min_size(&mode);
        if img_size < min {
            return Err(anyhow!(
                "persistence image too small. Minimum for {}: {}",
                mode,
                humansize::format_size(min, humansize::BINARY)
            ));
        }
        println!(
            "Creating persistence image: {} ({})",
            img_path,
            humansize::format_size(img_size, humansize::BINARY)
        );
        persist::create_persistence_image(
            std::path::Path::new(img_path),
            img_size,
            filesystem,
        )?;
        println!("Done. Persistence image created.");
        if let Some(conf) = persist::generate_persistence_conf(&mode) {
            println!("Note: This mode requires a persistence.conf with:\n{}", conf);
        }
        return Ok(());
    }

    let _config = PersistConfig {
        device: opts.device.clone(),
        size,
        filesystem,
        label: label.clone(),
        mode,
        encrypt: opts.encrypt,
    };

    // For partition-based persistence, we need root/admin privileges
    // and platform-specific partition tools (fdisk/gdisk/diskpart)
    println!("Persistent storage configuration:");
    println!("  Device:     {}", opts.device);
    println!("  Filesystem: {}", filesystem);
    println!("  Label:      {}", label);
    println!("  Mode:       {}", mode);
    println!(
        "  Size:       {}",
        if size == 0 {
            "all remaining space".to_string()
        } else {
            humansize::format_size(size, humansize::BINARY)
        }
    );
    if opts.encrypt {
        println!("  Encrypted:  yes (LUKS)");
    }

    if !opts.force {
        return Err(anyhow!(
            "Creating a partition on {} requires --force. This operation is irreversible.",
            opts.device
        ));
    }

    // Check free space
    let meta = std::fs::metadata(&opts.device)?;
    let device_size = meta.len();
    let (start, available) = persist::find_free_space(&opts.device, device_size)?;
    let actual_size = if size == 0 { available } else { size };
    if actual_size > available {
        return Err(anyhow!(
            "requested {} but only {} available after existing partitions",
            humansize::format_size(actual_size, humansize::BINARY),
            humansize::format_size(available, humansize::BINARY)
        ));
    }

    println!(
        "Creating {} partition at offset {} ({})...",
        filesystem,
        start,
        humansize::format_size(actual_size, humansize::BINARY)
    );

    // This would call platform-specific partitioning tools
    // For now, report what would be done
    println!("Partition creation requires platform-specific tools (fdisk/gdisk/diskpart).");
    println!("Use 'abt persist --image-file <path>' for file-based persistence instead.");

    Ok(())
}
