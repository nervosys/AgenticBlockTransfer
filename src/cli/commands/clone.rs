use std::sync::Arc;

use anyhow::Result;
use log::info;

use crate::cli::{self, CloneOpts};
use crate::core::clone::{CloneConfig, clone_device};
use crate::core::progress::Progress;
use crate::core::types::HashAlgorithm;

pub async fn execute(opts: CloneOpts) -> Result<()> {
    let block_size = cli::parse_block_size(&opts.block_size)?;
    let hash_algorithm = match opts.hash_algorithm.to_lowercase().as_str() {
        "md5" => HashAlgorithm::Md5,
        "sha1" => HashAlgorithm::Sha1,
        "sha256" | "sha-256" => HashAlgorithm::Sha256,
        "sha512" | "sha-512" => HashAlgorithm::Sha512,
        "blake3" => HashAlgorithm::Blake3,
        "crc32" => HashAlgorithm::Crc32,
        other => anyhow::bail!("Unknown hash algorithm: {}", other),
    };

    let config = CloneConfig {
        source: opts.source.clone(),
        target: opts.target.clone(),
        block_size,
        count: None,
        verify: opts.verify,
        hash_algorithm,
        sparse: opts.sparse,
        direct_io: true,
        sync: true,
    };

    let progress = Arc::new(Progress::new(0));

    info!(
        "Cloning {} -> {} (block_size={}, verify={}, sparse={})",
        opts.source,
        opts.target,
        opts.block_size,
        opts.verify,
        opts.sparse
    );

    let result = clone_device(&config, &progress)?;

    println!("Clone complete:");
    println!("  Bytes copied:   {}", result.bytes_copied);
    if result.bytes_sparse_skipped > 0 {
        println!("  Sparse skipped: {}", result.bytes_sparse_skipped);
    }
    if let Some(ref hash) = result.source_hash {
        println!("  Source hash:    {}", hash);
    }
    if result.verified.unwrap_or(false) {
        println!("  Verified:       OK");
    }

    Ok(())
}
