use anyhow::Result;

use crate::cli::BenchOpts;
use crate::core::bench::{self, BenchConfig};

pub async fn execute(opts: BenchOpts) -> Result<()> {
    let block_sizes = if opts.block_sizes.is_empty() {
        Vec::new()
    } else {
        opts.block_sizes
            .iter()
            .map(|s| crate::cli::parse_block_size(s))
            .collect::<Result<Vec<_>>>()?
    };

    let config = BenchConfig {
        target: opts.target,
        test_size: opts.test_size * 1024 * 1024, // Convert MiB to bytes
        block_sizes,
        iterations: opts.iterations,
        bench_write: !opts.read_only,
        bench_read: !opts.write_only,
        direct_io: opts.direct_io,
    };

    let report = bench::run_benchmark(&config)?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", bench::format_report(&report));
    }

    Ok(())
}
