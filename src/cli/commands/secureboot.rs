// CLI command: secureboot — Check Secure Boot status and manage keys

use anyhow::Result;

use crate::cli::SecureBootOpts;
use crate::core::secureboot;

pub async fn execute(opts: SecureBootOpts) -> Result<()> {
    match opts.action.as_str() {
        "status" | "check" => {
            let report = secureboot::generate_report();
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", secureboot::format_report(&report));
            }
        }
        "check-file" | "verify" => {
            let file = opts
                .file
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("--file is required for check-file/verify"))?;
            let path = std::path::Path::new(file);

            if !path.exists() {
                anyhow::bail!("File not found: {}", file);
            }

            let signed = secureboot::is_likely_signed(path)?;
            let is_shim = secureboot::is_shim_bootloader(
                path.file_name().unwrap_or_default().to_str().unwrap_or(""),
            );
            let is_mok = secureboot::is_mok_manager(
                path.file_name().unwrap_or_default().to_str().unwrap_or(""),
            );

            if opts.json {
                println!(
                    "{{\"file\":\"{}\",\"likely_signed\":{},\"is_shim\":{},\"is_mok_manager\":{}}}",
                    file, signed, is_shim, is_mok
                );
            } else {
                println!("File: {}", file);
                println!("  Likely signed: {}", if signed { "Yes" } else { "No" });
                println!("  Is shim:       {}", if is_shim { "Yes" } else { "No" });
                println!("  Is MOK manager:{}", if is_mok { "Yes" } else { "No" });

                if signed {
                    println!("\n  ✓ This bootloader appears to be Authenticode-signed.");
                    println!("    It should work with Secure Boot enabled.");
                } else {
                    println!("\n  ✗ This bootloader does NOT appear to be signed.");
                    println!("    Secure Boot must be disabled, or enroll via MOK.");
                }
            }
        }
        "bootloaders" | "list" => {
            let loaders = secureboot::known_signed_bootloaders();
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&loaders)?);
            } else {
                println!("Known Secure Boot signed bootloader filenames:");
                for name in &loaders {
                    let kind = if secureboot::is_shim_bootloader(name) {
                        "shim"
                    } else if secureboot::is_mok_manager(name) {
                        "MOK manager"
                    } else {
                        "bootloader"
                    };
                    println!("  {} ({})", name, kind);
                }
            }
        }
        other => {
            anyhow::bail!(
                "Unknown action: '{}'. Use 'status', 'check-file', or 'bootloaders'.",
                other
            );
        }
    }

    Ok(())
}
