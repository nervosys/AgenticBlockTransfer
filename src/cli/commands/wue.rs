// CLI command: wue — Windows Unattended Setup (unattend.xml generation)

use anyhow::Result;
use log::info;

use crate::cli::WueOpts;
use crate::core::wue;

pub async fn execute(opts: WueOpts) -> Result<()> {
    let arch = match opts.arch.to_lowercase().as_str() {
        "amd64" | "x64" | "x86_64" => wue::WindowsArch::Amd64,
        "x86" | "i386" => wue::WindowsArch::X86,
        "arm64" | "aarch64" => wue::WindowsArch::Arm64,
        other => anyhow::bail!("Unknown architecture: {}. Use amd64, x86, or arm64.", other),
    };

    let version = if opts.win10 {
        wue::WindowsVersion::Windows10
    } else {
        wue::WindowsVersion::Windows11
    };

    let mut config = if opts.win10 {
        wue::windows10_minimal_config(&opts.username)
    } else {
        wue::windows11_bypass_config(&opts.username, &opts.password.unwrap_or_default())
    };

    config.arch = arch;
    config.version = version;

    if let Some(ref tz) = opts.timezone {
        config.locale.timezone = tz.clone();
    }
    if let Some(ref locale) = opts.locale {
        config.locale.ui_language = locale.clone();
        config.locale.system_locale = locale.clone();
        config.locale.user_locale = locale.clone();
    }
    if let Some(ref computer_name) = opts.computer_name {
        config.oobe.computer_name = computer_name.clone();
    }
    if let Some(ref key) = opts.product_key {
        config.product_key = Some(key.clone());
    }

    if opts.no_bypass {
        config.hardware_bypass = wue::HardwareBypass {
            bypass_tpm: false,
            bypass_secure_boot: false,
            bypass_ram: false,
            bypass_storage: false,
            bypass_cpu: false,
        };
    }

    let xml = wue::generate_unattend_xml(&config);

    if let Some(ref output) = opts.output {
        std::fs::write(output, &xml)?;
        info!("Wrote unattend.xml to {}", output);
        println!("Generated: {}", output);
    } else {
        // Print to stdout
        print!("{}", xml);
    }

    Ok(())
}
