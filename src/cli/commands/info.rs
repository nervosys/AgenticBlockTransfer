use anyhow::Result;
use std::path::PathBuf;

use crate::cli::InfoOpts;
use crate::core::device;
use crate::core::image;
use crate::core::iso9660;
use crate::core::qcow2;
use crate::core::vhd;
use crate::core::vmdk;
use crate::core::wim;

pub async fn execute(opts: InfoOpts) -> Result<()> {
    let path = PathBuf::from(&opts.path);

    if path.is_file() {
        // Image file info
        let info = image::get_image_info(&path)?;
        println!("Image Information:");
        println!("  Path:              {}", info.path);
        println!("  Format:            {}", info.format);
        println!(
            "  Size:              {} ({})",
            humansize::format_size(info.size, humansize::BINARY),
            info.size
        );
        if let Some(inner) = info.inner_format {
            println!("  Inner Format:      {}", inner);
        }
        if info.format.is_compressed() {
            println!("  Compressed:        Yes");
            if let Some(decompressed) = info.decompressed_size {
                println!(
                    "  Decompressed Size: {}",
                    humansize::format_size(decompressed, humansize::BINARY)
                );
            }
        }

        // If it's an ISO, show ISO 9660 metadata
        if matches!(info.format, crate::core::types::ImageFormat::Iso) {
            match iso9660::read_iso9660_info(&path) {
                Ok(iso_info) => {
                    println!();
                    print!("{}", iso_info);
                }
                Err(e) => {
                    log::debug!("Could not parse ISO 9660 metadata: {}", e);
                }
            }
        }

        // If it's QCOW2, show virtual disk metadata
        if matches!(info.format, crate::core::types::ImageFormat::Qcow2) {
            match qcow2::parse_qcow2(&path) {
                Ok(header) => {
                    println!();
                    println!("QCOW2 Metadata:");
                    println!("  {}", header.summary());
                }
                Err(e) => {
                    log::debug!("Could not parse QCOW2 metadata: {}", e);
                }
            }
        }

        // If it's VHD, show virtual disk metadata
        if matches!(info.format, crate::core::types::ImageFormat::Vhd) {
            match vhd::parse_vhd(&path) {
                Ok(footer) => {
                    println!();
                    println!("VHD Metadata:");
                    println!("  {}", footer.summary());
                }
                Err(e) => {
                    log::debug!("Could not parse VHD metadata: {}", e);
                }
            }
        }

        // If it's VHDX, show virtual disk metadata
        if matches!(info.format, crate::core::types::ImageFormat::Vhdx) {
            match std::fs::File::open(&path) {
                Ok(mut f) => {
                    match vhd::parse_vhdx(&mut f) {
                        Ok(info) => {
                            println!();
                            println!("VHDX Metadata:");
                            println!("  {}", info.summary());
                        }
                        Err(e) => {
                            log::debug!("Could not parse VHDX metadata: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::debug!("Could not open VHDX file: {}", e);
                }
            }
        }

        // If it's VMDK, show virtual disk metadata
        if matches!(info.format, crate::core::types::ImageFormat::Vmdk) {
            match vmdk::parse_vmdk(&path) {
                Ok(header) => {
                    println!();
                    println!("VMDK Metadata:");
                    println!("  {}", header.summary());
                    // Try to show embedded descriptor
                    if let Ok(mut f) = std::fs::File::open(&path) {
                        if let Ok(Some(desc)) = header.read_descriptor(&mut f) {
                            println!("  Descriptor:");
                            for line in desc.lines().take(20) {
                                println!("    {}", line);
                            }
                        }
                    }
                }
                Err(e) => {
                    log::debug!("Could not parse VMDK metadata: {}", e);
                }
            }
        }

        // If it's WIM, show WIM metadata
        if matches!(info.format, crate::core::types::ImageFormat::Wim) {
            match wim::parse_wim(&path) {
                Ok(header) => {
                    println!();
                    println!("WIM Metadata:");
                    println!("  {}", header.summary());
                    println!("  GUID:  {}", header.guid_string());
                    // Try to show XML metadata
                    if let Ok(mut f) = std::fs::File::open(&path) {
                        if let Ok(Some(xml)) = header.read_xml(&mut f) {
                            println!("  XML Metadata (first 30 lines):");
                            for line in xml.lines().take(30) {
                                let trimmed = line.trim();
                                if !trimmed.is_empty() {
                                    println!("    {}", trimmed);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    log::debug!("Could not parse WIM metadata: {}", e);
                }
            }
        }
    } else {
        // Device info
        let enumerator = device::create_enumerator();
        let dev = enumerator.get_device(&opts.path).await?;

        println!("Device Information:");
        println!("  Path:            {}", dev.path);
        println!("  Name:            {}", dev.name);
        if !dev.vendor.is_empty() {
            println!("  Vendor:          {}", dev.vendor);
        }
        if let Some(ref serial) = dev.serial {
            println!("  Serial:          {}", serial);
        }
        println!(
            "  Size:            {} ({})",
            humansize::format_size(dev.size, humansize::BINARY),
            dev.size
        );
        println!("  Sector Size:     {} (physical: {})", dev.sector_size, dev.physical_sector_size);
        println!("  Type:            {}", dev.device_type);
        println!("  Transport:       {}", dev.transport);
        println!("  Removable:       {}", if dev.removable { "Yes" } else { "No" });
        println!("  Read-only:       {}", if dev.read_only { "Yes" } else { "No" });
        println!("  System Drive:    {}", if dev.is_system { "Yes" } else { "No" });
        println!("  Safe Target:     {}", if dev.is_safe_target() { "Yes" } else { "No" });
        if !dev.mount_points.is_empty() {
            println!("  Mount Points:    {}", dev.mount_points.join(", "));
        }
    }

    Ok(())
}
