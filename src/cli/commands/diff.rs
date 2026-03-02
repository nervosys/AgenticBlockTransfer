use anyhow::Result;

use crate::cli::DiffOpts;
use crate::core::diff::{self, DiffWriteConfig};
use crate::core::progress::Progress;

pub async fn execute(opts: DiffOpts) -> Result<()> {
    let config = DiffWriteConfig {
        source: opts.source,
        target: opts.target,
        block_size: crate::cli::parse_block_size(&opts.block_size)?,
        verify: !opts.no_verify,
        dry_run: opts.dry_run,
    };

    let progress = Progress::new(0);
    let result = diff::diff_write(&config, &progress)?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print!("{}", diff::format_diff_result(&result));
    }

    Ok(())
}
