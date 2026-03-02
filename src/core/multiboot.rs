#![allow(dead_code)]
//! Ventoy-style multi-boot — install multiple ISO images on a single USB device.
//!
//! Creates a bootable USB with a data partition containing multiple ISO files
//! and a small EFI/BIOS boot partition with GRUB configuration to boot any
//! of the installed ISOs.
//!
//! Layout:
//! ```text
//! ┌─────────────────────────────────────────────────┐
//! │ Partition 1: Data (exFAT/FAT32)                 │
//! │   /iso/                                         │
//! │     ubuntu-24.04.iso                            │
//! │     fedora-41.iso                               │
//! │     windows11.iso                               │
//! │   /abt-multiboot.json   (registry)              │
//! ├─────────────────────────────────────────────────┤
//! │ Partition 2: EFI System Partition (FAT32, 64MB) │
//! │   /EFI/BOOT/                                    │
//! │     BOOTX64.EFI   (GRUB)                        │
//! │     grub.cfg                                    │
//! └─────────────────────────────────────────────────┘
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Registry of installed ISO images on a multi-boot device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultibootRegistry {
    /// Schema version.
    pub version: u32,
    /// Device label.
    pub label: String,
    /// Creation timestamp.
    pub created: String,
    /// Last modified timestamp.
    pub modified: String,
    /// Installed ISO images.
    pub images: Vec<MultibootImage>,
}

impl Default for MultibootRegistry {
    fn default() -> Self {
        Self {
            version: 1,
            label: "ABT Multiboot".into(),
            created: chrono::Utc::now().to_rfc3339(),
            modified: chrono::Utc::now().to_rfc3339(),
            images: Vec::new(),
        }
    }
}

/// An installed ISO image entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultibootImage {
    /// Display name for the boot menu.
    pub name: String,
    /// Filename within the /iso/ directory.
    pub filename: String,
    /// SHA-256 hash of the ISO file.
    pub sha256: String,
    /// File size in bytes.
    pub size: u64,
    /// Detected OS type (for GRUB menu hints).
    pub os_type: OsType,
    /// When this image was added.
    pub added: String,
    /// Boot parameters / kernel command line (optional).
    pub boot_params: Option<String>,
}

/// Detected OS type for boot menu configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OsType {
    /// Generic Linux live ISO.
    Linux,
    /// Ubuntu / Debian family.
    Ubuntu,
    /// Fedora / Red Hat family.
    Fedora,
    /// Arch Linux family.
    Arch,
    /// Windows PE / installer.
    Windows,
    /// macOS installer (limited support).
    MacOs,
    /// FreeBSD / other BSD.
    Bsd,
    /// Unknown / generic.
    Unknown,
}

impl OsType {
    /// Detect OS type from ISO filename and metadata.
    pub fn detect(filename: &str) -> Self {
        let lower = filename.to_lowercase();
        if lower.contains("ubuntu") || lower.contains("kubuntu") || lower.contains("xubuntu")
            || lower.contains("mint") || lower.contains("debian") || lower.contains("pop_os")
        {
            Self::Ubuntu
        } else if lower.contains("fedora") || lower.contains("centos") || lower.contains("rhel")
            || lower.contains("rocky") || lower.contains("alma")
        {
            Self::Fedora
        } else if lower.contains("arch") || lower.contains("manjaro") || lower.contains("endeavour") {
            Self::Arch
        } else if lower.contains("freebsd") || lower.contains("openbsd") || lower.contains("netbsd") {
            Self::Bsd
        } else if lower.contains("windows") || lower.contains("win10") || lower.contains("win11") {
            Self::Windows
        } else if lower.contains("macos") || lower.contains("osx") || lower.contains("darwin") {
            Self::MacOs
        } else if lower.contains("linux") || lower.contains("live") || lower.contains("suse")
            || lower.contains("gentoo") || lower.contains("slackware") || lower.contains("kali")
            || lower.contains("tails") || lower.contains("void") || lower.contains("nixos")
        {
            Self::Linux
        } else {
            Self::Unknown
        }
    }

    /// Menu label prefix for this OS type.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Ubuntu => "Ubuntu",
            Self::Fedora => "Fedora",
            Self::Arch => "Arch Linux",
            Self::Windows => "Windows",
            Self::MacOs => "macOS",
            Self::Bsd => "BSD",
            Self::Unknown => "OS",
        }
    }
}

/// Configuration for multi-boot device creation.
#[derive(Debug, Clone)]
pub struct MultibootConfig {
    /// Device path to set up (e.g., /dev/sdb).
    pub device: String,
    /// Volume label for the data partition.
    pub label: String,
    /// Size of the EFI system partition in MiB (default: 64).
    pub efi_size_mib: u32,
    /// Filesystem for data partition (exfat or fat32).
    pub data_fs: String,
    /// ISO directory name on the data partition.
    pub iso_dir: String,
    /// Registry filename.
    pub registry_file: String,
}

impl Default for MultibootConfig {
    fn default() -> Self {
        Self {
            device: String::new(),
            label: "ABT_MULTIBOOT".into(),
            efi_size_mib: 64,
            data_fs: "exfat".into(),
            iso_dir: "iso".into(),
            registry_file: "abt-multiboot.json".into(),
        }
    }
}

/// Result of an add/remove operation.
#[derive(Debug, Clone)]
pub struct MultibootResult {
    /// Number of images currently installed.
    pub image_count: usize,
    /// Total size of all images.
    pub total_size: u64,
    /// Free space remaining on the data partition (if known).
    pub free_space: Option<u64>,
    /// Human-readable message.
    pub message: String,
}

/// Read the multi-boot registry from a mounted data partition.
pub fn read_registry(mount_point: &Path) -> Result<MultibootRegistry> {
    let registry_path = mount_point.join("abt-multiboot.json");

    if !registry_path.exists() {
        return Ok(MultibootRegistry::default());
    }

    let data = std::fs::read_to_string(&registry_path)
        .with_context(|| format!("Failed to read registry: {}", registry_path.display()))?;

    let registry: MultibootRegistry = serde_json::from_str(&data)
        .with_context(|| "Failed to parse multiboot registry")?;

    Ok(registry)
}

/// Write the multi-boot registry to a mounted data partition.
pub fn write_registry(mount_point: &Path, registry: &MultibootRegistry) -> Result<()> {
    let registry_path = mount_point.join("abt-multiboot.json");
    let data = serde_json::to_string_pretty(registry)
        .context("Failed to serialize registry")?;

    std::fs::write(&registry_path, data)
        .with_context(|| format!("Failed to write registry: {}", registry_path.display()))?;

    Ok(())
}

/// Add an ISO image to the multi-boot device.
///
/// Copies the ISO to the `/iso/` directory, updates the registry, and
/// regenerates the GRUB configuration.
pub fn add_image(
    mount_point: &Path,
    iso_path: &Path,
    name: Option<&str>,
) -> Result<MultibootResult> {
    // Validate ISO exists
    if !iso_path.exists() {
        anyhow::bail!("ISO file not found: {}", iso_path.display());
    }

    let filename = iso_path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Invalid ISO path"))?
        .to_string_lossy()
        .to_string();

    // Read existing registry
    let mut registry = read_registry(mount_point)?;

    // Check for duplicate
    if registry.images.iter().any(|img| img.filename == filename) {
        anyhow::bail!("Image '{}' is already installed", filename);
    }

    // Create iso directory if needed
    let iso_dir = mount_point.join("iso");
    std::fs::create_dir_all(&iso_dir)
        .with_context(|| format!("Failed to create ISO directory: {}", iso_dir.display()))?;

    // Copy ISO file
    let dest = iso_dir.join(&filename);
    let size = std::fs::metadata(iso_path)?.len();
    std::fs::copy(iso_path, &dest)
        .with_context(|| format!("Failed to copy ISO to {}", dest.display()))?;

    // Compute SHA-256 hash
    let hash = compute_file_hash(&dest)?;

    // Detect OS type
    let os_type = OsType::detect(&filename);

    // Display name
    let display_name = name
        .map(|n| n.to_string())
        .unwrap_or_else(|| {
            let stem = iso_path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            format!("{} ({})", os_type.label(), stem)
        });

    // Add to registry
    registry.images.push(MultibootImage {
        name: display_name.clone(),
        filename: filename.clone(),
        sha256: hash,
        size,
        os_type,
        added: chrono::Utc::now().to_rfc3339(),
        boot_params: None,
    });
    registry.modified = chrono::Utc::now().to_rfc3339();

    // Write updated registry
    write_registry(mount_point, &registry)?;

    // Regenerate GRUB config
    let grub_cfg = generate_grub_config(&registry);
    let grub_dir = mount_point.join("EFI").join("BOOT");
    std::fs::create_dir_all(&grub_dir)?;
    std::fs::write(grub_dir.join("grub.cfg"), grub_cfg)?;

    let total_size: u64 = registry.images.iter().map(|img| img.size).sum();

    Ok(MultibootResult {
        image_count: registry.images.len(),
        total_size,
        free_space: None,
        message: format!("Added '{}' ({} images total)", display_name, registry.images.len()),
    })
}

/// Remove an ISO image from the multi-boot device.
pub fn remove_image(mount_point: &Path, filename: &str) -> Result<MultibootResult> {
    let mut registry = read_registry(mount_point)?;

    let idx = registry
        .images
        .iter()
        .position(|img| img.filename == filename)
        .ok_or_else(|| anyhow::anyhow!("Image '{}' not found in registry", filename))?;

    let removed = registry.images.remove(idx);
    registry.modified = chrono::Utc::now().to_rfc3339();

    // Delete the ISO file
    let iso_path = mount_point.join("iso").join(filename);
    if iso_path.exists() {
        std::fs::remove_file(&iso_path)
            .with_context(|| format!("Failed to delete {}", iso_path.display()))?;
    }

    // Write updated registry
    write_registry(mount_point, &registry)?;

    // Regenerate GRUB config
    let grub_cfg = generate_grub_config(&registry);
    let grub_dir = mount_point.join("EFI").join("BOOT");
    std::fs::create_dir_all(&grub_dir)?;
    std::fs::write(grub_dir.join("grub.cfg"), grub_cfg)?;

    let total_size: u64 = registry.images.iter().map(|img| img.size).sum();

    Ok(MultibootResult {
        image_count: registry.images.len(),
        total_size,
        free_space: None,
        message: format!(
            "Removed '{}' ({} images remaining)",
            removed.name,
            registry.images.len()
        ),
    })
}

/// List installed images on a multi-boot device.
pub fn list_images(mount_point: &Path) -> Result<Vec<MultibootImage>> {
    let registry = read_registry(mount_point)?;
    Ok(registry.images)
}

/// Generate a GRUB configuration file for all installed images.
pub fn generate_grub_config(registry: &MultibootRegistry) -> String {
    let mut cfg = String::new();

    cfg.push_str("# ABT Multiboot — Auto-generated GRUB configuration\n");
    cfg.push_str(&format!(
        "# Generated: {}\n",
        chrono::Utc::now().to_rfc3339()
    ));
    cfg.push_str(&format!("# Images: {}\n\n", registry.images.len()));

    // GRUB preamble
    cfg.push_str("set default=0\n");
    cfg.push_str("set timeout=10\n");
    cfg.push_str("set menu_color_normal=white/black\n");
    cfg.push_str("set menu_color_highlight=black/light-gray\n\n");

    cfg.push_str("insmod all_video\n");
    cfg.push_str("insmod gfxterm\n");
    cfg.push_str("insmod loopback\n");
    cfg.push_str("insmod iso9660\n");
    cfg.push_str("insmod ntfs\n");
    cfg.push_str("insmod fat\n");
    cfg.push_str("insmod exfat\n\n");

    cfg.push_str(&format!(
        "menuentry \"{}\" {{\n  echo \"Select an operating system to boot...\"\n}}\n\n",
        registry.label
    ));

    // Generate a menu entry for each ISO
    for (i, image) in registry.images.iter().enumerate() {
        cfg.push_str(&generate_menu_entry(i, image));
        cfg.push('\n');
    }

    // Power off / reboot entries
    cfg.push_str("menuentry \"Reboot\" {\n  reboot\n}\n\n");
    cfg.push_str("menuentry \"Power Off\" {\n  halt\n}\n");

    cfg
}

/// Generate a GRUB menu entry for a single ISO image.
fn generate_menu_entry(index: usize, image: &MultibootImage) -> String {
    let iso_path = format!("/iso/{}", image.filename);

    match image.os_type {
        OsType::Ubuntu | OsType::Linux | OsType::Fedora | OsType::Arch => {
            format!(
                "menuentry \"[{}] {}\" {{\n\
                 \tset isofile=\"{}\"\n\
                 \tloopback loop $isofile\n\
                 \tlinux (loop)/casper/vmlinuz boot=casper iso-scan/filename=$isofile quiet splash {}\n\
                 \tinitrd (loop)/casper/initrd\n\
                 }}\n",
                index + 1,
                image.name,
                iso_path,
                image.boot_params.as_deref().unwrap_or(""),
            )
        }
        OsType::Windows => {
            format!(
                "menuentry \"[{}] {}\" {{\n\
                 \tset isofile=\"{}\"\n\
                 \tloopback loop $isofile\n\
                 \tchainloader (loop)+1\n\
                 }}\n",
                index + 1,
                image.name,
                iso_path,
            )
        }
        OsType::Bsd => {
            format!(
                "menuentry \"[{}] {}\" {{\n\
                 \tset isofile=\"{}\"\n\
                 \tloopback loop $isofile\n\
                 \tkfreebsd (loop)/boot/kernel/kernel\n\
                 \tkfreebsd_loadenv (loop)/boot/device.hints\n\
                 }}\n",
                index + 1,
                image.name,
                iso_path,
            )
        }
        _ => {
            // Generic loopback boot
            format!(
                "menuentry \"[{}] {}\" {{\n\
                 \tset isofile=\"{}\"\n\
                 \tloopback loop $isofile\n\
                 \tlinux (loop)/boot/vmlinuz boot=live iso-scan/filename=$isofile {}\n\
                 \tinitrd (loop)/boot/initrd\n\
                 }}\n",
                index + 1,
                image.name,
                iso_path,
                image.boot_params.as_deref().unwrap_or(""),
            )
        }
    }
}

/// Compute SHA-256 hash of a file.
fn compute_file_hash(path: &Path) -> Result<String> {
    use sha2::Digest;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];

    loop {
        let n = std::io::Read::read(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Format a multi-boot listing for display.
pub fn format_listing(images: &[MultibootImage]) -> String {
    if images.is_empty() {
        return "No images installed on this multi-boot device.".into();
    }

    let mut out = String::new();
    out.push_str(&format!(
        "Multi-boot Device — {} image(s)\n\n",
        images.len()
    ));
    out.push_str(&format!(
        "{:<4} {:<30} {:<10} {:<12} {}\n",
        "#", "Name", "Type", "Size", "SHA-256"
    ));
    out.push_str(&"-".repeat(80));
    out.push('\n');

    for (i, img) in images.iter().enumerate() {
        let size = humansize::format_size(img.size, humansize::BINARY);
        let hash_short = if img.sha256.len() >= 12 {
            &img.sha256[..12]
        } else {
            &img.sha256
        };
        out.push_str(&format!(
            "{:<4} {:<30} {:<10} {:<12} {}…\n",
            i + 1,
            truncate_str(&img.name, 28),
            img.os_type.label(),
            size,
            hash_short,
        ));
    }

    let total: u64 = images.iter().map(|img| img.size).sum();
    out.push_str(&format!(
        "\nTotal: {}\n",
        humansize::format_size(total, humansize::BINARY)
    ));

    out
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_os_type_detection() {
        assert_eq!(OsType::detect("ubuntu-24.04-desktop-amd64.iso"), OsType::Ubuntu);
        assert_eq!(OsType::detect("Fedora-Workstation-41.iso"), OsType::Fedora);
        assert_eq!(OsType::detect("archlinux-2024.01.01-x86_64.iso"), OsType::Arch);
        assert_eq!(OsType::detect("FreeBSD-14.0-RELEASE-amd64-disc1.iso"), OsType::Bsd);
        assert_eq!(OsType::detect("Windows11_23H2.iso"), OsType::Windows);
        assert_eq!(OsType::detect("random-utility.iso"), OsType::Unknown);
        assert_eq!(OsType::detect("kali-linux-2024.1-live-amd64.iso"), OsType::Linux);
        assert_eq!(OsType::detect("linuxmint-21.3-cinnamon-64bit.iso"), OsType::Ubuntu);
    }

    #[test]
    fn test_os_type_labels() {
        assert_eq!(OsType::Linux.label(), "Linux");
        assert_eq!(OsType::Windows.label(), "Windows");
        assert_eq!(OsType::Bsd.label(), "BSD");
        assert_eq!(OsType::Unknown.label(), "OS");
    }

    #[test]
    fn test_default_config() {
        let cfg = MultibootConfig::default();
        assert_eq!(cfg.label, "ABT_MULTIBOOT");
        assert_eq!(cfg.efi_size_mib, 64);
        assert_eq!(cfg.data_fs, "exfat");
        assert_eq!(cfg.iso_dir, "iso");
    }

    #[test]
    fn test_registry_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();

        let mut registry = MultibootRegistry::default();
        registry.images.push(MultibootImage {
            name: "Test Image".into(),
            filename: "test.iso".into(),
            sha256: "abc123".into(),
            size: 1024,
            os_type: OsType::Linux,
            added: chrono::Utc::now().to_rfc3339(),
            boot_params: None,
        });

        write_registry(tmp.path(), &registry).unwrap();
        let loaded = read_registry(tmp.path()).unwrap();

        assert_eq!(loaded.images.len(), 1);
        assert_eq!(loaded.images[0].name, "Test Image");
        assert_eq!(loaded.images[0].filename, "test.iso");
    }

    #[test]
    fn test_empty_registry() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = read_registry(tmp.path()).unwrap();
        assert!(registry.images.is_empty());
        assert_eq!(registry.version, 1);
    }

    #[test]
    fn test_generate_grub_config() {
        let mut registry = MultibootRegistry::default();
        registry.images.push(MultibootImage {
            name: "Ubuntu 24.04".into(),
            filename: "ubuntu-24.04.iso".into(),
            sha256: "abc".into(),
            size: 1024 * 1024 * 1024,
            os_type: OsType::Ubuntu,
            added: chrono::Utc::now().to_rfc3339(),
            boot_params: None,
        });
        registry.images.push(MultibootImage {
            name: "Windows 11".into(),
            filename: "win11.iso".into(),
            sha256: "def".into(),
            size: 5 * 1024 * 1024 * 1024,
            os_type: OsType::Windows,
            added: chrono::Utc::now().to_rfc3339(),
            boot_params: None,
        });

        let cfg = generate_grub_config(&registry);
        assert!(cfg.contains("set default=0"));
        assert!(cfg.contains("set timeout=10"));
        assert!(cfg.contains("Ubuntu 24.04"));
        assert!(cfg.contains("Windows 11"));
        assert!(cfg.contains("loopback loop"));
        assert!(cfg.contains("Reboot"));
        assert!(cfg.contains("Power Off"));
        assert!(cfg.contains("/iso/ubuntu-24.04.iso"));
        assert!(cfg.contains("chainloader")); // Windows uses chainloader
    }

    #[test]
    fn test_add_and_remove_image() {
        let tmp = tempfile::tempdir().unwrap();
        let iso_dir = tmp.path().join("iso");
        std::fs::create_dir_all(&iso_dir).unwrap();

        // Create a fake ISO
        let iso_tmp = tempfile::tempdir().unwrap();
        let iso_path = iso_tmp.path().join("test-linux.iso");
        let mut iso_file = std::fs::File::create(&iso_path).unwrap();
        iso_file.write_all(&[0u8; 4096]).unwrap();

        // Add image
        let result = add_image(tmp.path(), &iso_path, Some("Test Linux")).unwrap();
        assert_eq!(result.image_count, 1);
        assert!(result.message.contains("Test Linux"));

        // List images
        let images = list_images(tmp.path()).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].name, "Test Linux");
        assert_eq!(images[0].os_type, OsType::Linux);

        // Verify GRUB config was generated
        let grub_path = tmp.path().join("EFI").join("BOOT").join("grub.cfg");
        assert!(grub_path.exists());

        // Remove image
        let result = remove_image(tmp.path(), "test-linux.iso").unwrap();
        assert_eq!(result.image_count, 0);

        let images = list_images(tmp.path()).unwrap();
        assert!(images.is_empty());
    }

    #[test]
    fn test_duplicate_add_rejected() {
        let tmp = tempfile::tempdir().unwrap();

        let iso_tmp = tempfile::tempdir().unwrap();
        let iso_path = iso_tmp.path().join("test.iso");
        std::fs::write(&iso_path, &[0u8; 1024]).unwrap();

        add_image(tmp.path(), &iso_path, None).unwrap();
        let result = add_image(tmp.path(), &iso_path, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already installed"));
    }

    #[test]
    fn test_format_listing() {
        let images = vec![
            MultibootImage {
                name: "Ubuntu 24.04".into(),
                filename: "ubuntu.iso".into(),
                sha256: "abcdef123456789abcdef".into(),
                size: 4 * 1024 * 1024 * 1024,
                os_type: OsType::Ubuntu,
                added: "2024-01-01T00:00:00Z".into(),
                boot_params: None,
            },
            MultibootImage {
                name: "Fedora 41".into(),
                filename: "fedora.iso".into(),
                sha256: "123456789abcdef01234".into(),
                size: 2 * 1024 * 1024 * 1024,
                os_type: OsType::Fedora,
                added: "2024-01-01T00:00:00Z".into(),
                boot_params: None,
            },
        ];

        let output = format_listing(&images);
        assert!(output.contains("2 image(s)"));
        assert!(output.contains("Ubuntu 24.04"));
        assert!(output.contains("Fedora 41"));
    }

    #[test]
    fn test_format_listing_empty() {
        let output = format_listing(&[]);
        assert!(output.contains("No images installed"));
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("short", 10), "short");
        assert_eq!(truncate_str("a very long string here", 10), "a very lo…");
    }
}
