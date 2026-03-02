use anyhow::Result;

use crate::cli::BootOpts;
use crate::core::boot;

pub fn execute(opts: BootOpts) -> Result<()> {
    let path = std::path::Path::new(&opts.path);
    let validation = boot::validate_boot_sector(path)?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&validation)?);
    } else {
        print!("{}", boot::format_validation(&validation));
        println!("\n{}", validation.summary);
    }

    Ok(())
}
