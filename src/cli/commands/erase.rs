use std::sync::Arc;

use anyhow::Result;
use log::info;

use crate::cli::EraseOpts;
use crate::core::erase::{EraseConfig, EraseMethod, erase_device};
use crate::core::progress::Progress;

pub async fn execute(opts: EraseOpts) -> Result<()> {
    let method = EraseMethod::from_str_name(&opts.method)
        .ok_or_else(|| anyhow::anyhow!("Unknown erase method: {}", opts.method))?;

    let config = EraseConfig {
        device: opts.device.clone(),
        method,
        passes: opts.passes,
        force: opts.force,
    };

    let progress = Arc::new(Progress::new(0));

    info!(
        "Erasing {} (method={}, passes={})",
        opts.device,
        method.label(),
        opts.passes
    );

    let result = erase_device(&config, &progress)?;

    println!("Erase complete:");
    println!("  Method:          {}", result.method_used.label());
    println!("  Bytes erased:    {}", result.bytes_erased);
    println!("  Passes:          {}", result.passes_completed);
    if result.verified {
        println!("  Verified:        OK");
    }

    Ok(())
}
