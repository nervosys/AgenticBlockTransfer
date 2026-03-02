use anyhow::Result;
use std::path::PathBuf;

use crate::cli::ChecksumOpts;
use crate::core::hasher;
use crate::core::progress::Progress;
use crate::core::types::HashAlgorithm;

pub async fn execute(opts: ChecksumOpts) -> Result<()> {
    let path = PathBuf::from(&opts.path);

    for alg_str in &opts.algorithm {
        let algorithm = match alg_str.to_lowercase().as_str() {
            "md5" => HashAlgorithm::Md5,
            "sha1" => HashAlgorithm::Sha1,
            "sha256" => HashAlgorithm::Sha256,
            "sha512" => HashAlgorithm::Sha512,
            "blake3" => HashAlgorithm::Blake3,
            "crc32" => HashAlgorithm::Crc32,
            _ => {
                eprintln!("Unknown algorithm: {}", alg_str);
                continue;
            }
        };

        let progress = Progress::new(0);
        let hash = hasher::hash_file(&path, algorithm, &progress)?;
        println!("{} ({}) = {}", path.display(), algorithm, hash);
    }

    Ok(())
}
