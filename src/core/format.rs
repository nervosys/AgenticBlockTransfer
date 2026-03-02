use anyhow::Result;
use super::types::Filesystem;

/// Format a device with the specified filesystem.
/// This dispatches to platform-specific formatting utilities.
pub async fn format_device(
    device_path: &str,
    filesystem: Filesystem,
    label: Option<&str>,
    quick: bool,
) -> Result<()> {
    log::info!(
        "Formatting {} as {} (label: {:?}, quick: {})",
        device_path,
        filesystem,
        label,
        quick
    );

    #[cfg(target_os = "linux")]
    {
        format_linux(device_path, filesystem, label, quick).await
    }

    #[cfg(target_os = "macos")]
    {
        format_macos(device_path, filesystem, label, quick).await
    }

    #[cfg(target_os = "windows")]
    {
        format_windows(device_path, filesystem, label, quick).await
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        anyhow::bail!("Formatting not supported on this platform")
    }
}

#[cfg(target_os = "linux")]
async fn format_linux(
    device_path: &str,
    filesystem: Filesystem,
    label: Option<&str>,
    _quick: bool,
) -> Result<()> {
    // Validate inputs to prevent path traversal or unexpected arguments.
    // Device paths must be absolute and within /dev/.
    if !device_path.starts_with("/dev/") {
        anyhow::bail!(
            "Refusing to format non-device path: {}. Only /dev/* paths are allowed.",
            device_path
        );
    }

    // Reject paths containing suspicious components
    if device_path.contains("..") || device_path.contains('\0') {
        anyhow::bail!("Invalid device path: {}", device_path);
    }

    let (program, base_args): (&str, Vec<&str>) = match filesystem {
        Filesystem::Fat16 => ("mkfs.fat", vec!["-F", "16"]),
        Filesystem::Fat32 => ("mkfs.fat", vec!["-F", "32"]),
        Filesystem::ExFat => ("mkfs.exfat", vec![]),
        Filesystem::Ntfs => ("mkfs.ntfs", vec!["-f"]),
        Filesystem::Ext2 => ("mkfs.ext2", vec![]),
        Filesystem::Ext3 => ("mkfs.ext3", vec![]),
        Filesystem::Ext4 => ("mkfs.ext4", vec![]),
        Filesystem::Xfs => ("mkfs.xfs", vec!["-f"]),
        Filesystem::Btrfs => ("mkfs.btrfs", vec!["-f"]),
        Filesystem::None => return zero_device(device_path).await,
    };

    // Build args list directly — NO shell interpolation, prevents injection
    let mut args: Vec<String> = base_args.iter().map(|a| a.to_string()).collect();

    if let Some(l) = label {
        // Validate label doesn't contain null bytes or command-injection chars
        if l.contains('\0') || l.len() > 255 {
            anyhow::bail!("Invalid label: must be ≤ 255 chars with no null bytes");
        }
        let label_flag = match filesystem {
            Filesystem::Fat16 | Filesystem::Fat32 | Filesystem::ExFat => "-n",
            Filesystem::Ntfs
            | Filesystem::Ext2
            | Filesystem::Ext3
            | Filesystem::Ext4
            | Filesystem::Xfs
            | Filesystem::Btrfs => "-L",
            Filesystem::None => unreachable!(),
        };
        args.push(label_flag.to_string());
        args.push(l.to_string());
    }
    args.push(device_path.to_string());

    let output = tokio::process::Command::new(program)
        .args(&args)
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!(
            "Format failed ({}): {}",
            program,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

#[cfg(target_os = "macos")]
async fn format_macos(
    device_path: &str,
    filesystem: Filesystem,
    label: Option<&str>,
    _quick: bool,
) -> Result<()> {
    let fs_name = match filesystem {
        Filesystem::Fat32 => "MS-DOS",
        Filesystem::ExFat => "ExFAT",
        Filesystem::Ntfs => anyhow::bail!("NTFS formatting not supported on macOS"),
        Filesystem::Ext2 | Filesystem::Ext3 | Filesystem::Ext4 => {
            anyhow::bail!("ext formatting requires additional tools on macOS")
        }
        _ => anyhow::bail!("Filesystem {:?} not supported on macOS", filesystem),
    };

    let vol_label = label.unwrap_or("ABT");
    let output = tokio::process::Command::new("diskutil")
        .args(["eraseDisk", fs_name, vol_label, device_path])
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!(
            "Format failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

#[cfg(target_os = "windows")]
async fn format_windows(
    device_path: &str,
    filesystem: Filesystem,
    label: Option<&str>,
    quick: bool,
) -> Result<()> {
    let fs_name = match filesystem {
        Filesystem::Fat32 => "FAT32",
        Filesystem::ExFat => "exFAT",
        Filesystem::Ntfs => "NTFS",
        _ => anyhow::bail!("Filesystem {:?} not directly supported on Windows", filesystem),
    };

    let vol_label = label.unwrap_or("ABT");
    let mut args = vec![
        "format".to_string(),
        device_path.to_string(),
        format!("/FS:{}", fs_name),
        format!("/V:{}", vol_label),
        "/Y".to_string(),
    ];
    if quick {
        args.push("/Q".to_string());
    }

    let output = tokio::process::Command::new("cmd")
        .args(["/C"])
        .args(&args)
        .output()
        .await?;

    if !output.status.success() {
        anyhow::bail!(
            "Format failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

#[allow(dead_code)]
async fn zero_device(device_path: &str) -> Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().write(true).open(device_path)?;
    let zeros = vec![0u8; 1024 * 1024]; // 1 MiB of zeros
    // Write 1 MiB to wipe partition table/header
    file.write_all(&zeros)?;
    file.flush()?;
    Ok(())
}
