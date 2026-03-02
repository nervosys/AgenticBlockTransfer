use anyhow::Result;
use std::path::PathBuf;

use crate::cli::VerifyOpts;
use crate::core::progress::Progress;
use crate::core::types::HashAlgorithm;
use crate::core::verifier;

pub async fn execute(opts: VerifyOpts) -> Result<()> {
    let algorithm = match opts.hash_algorithm.to_lowercase().as_str() {
        "md5" => HashAlgorithm::Md5,
        "sha1" => HashAlgorithm::Sha1,
        "sha256" => HashAlgorithm::Sha256,
        "sha512" => HashAlgorithm::Sha512,
        "blake3" => HashAlgorithm::Blake3,
        "crc32" => HashAlgorithm::Crc32,
        _ => HashAlgorithm::Sha256,
    };

    let progress = Progress::new(0);

    if let Some(expected) = &opts.expected_hash {
        // Verify target against known hash
        eprintln!("Verifying {} against expected hash...", opts.target);
        let matches =
            verifier::verify_against_hash(&opts.target, expected, algorithm, None, &progress)?;

        if matches {
            println!("PASS: Hash matches");
            Ok(())
        } else {
            anyhow::bail!("FAIL: Hash does not match")
        }
    } else if let Some(source) = &opts.source {
        // Verify target against source file
        let source_path = PathBuf::from(source);
        eprintln!(
            "Verifying {} against {}...",
            opts.target,
            source_path.display()
        );
        let matches =
            verifier::verify_by_hash(&source_path, &opts.target, algorithm, &progress)?;

        if matches {
            println!("PASS: Target matches source");
            Ok(())
        } else {
            anyhow::bail!("FAIL: Target does not match source")
        }
    } else {
        anyhow::bail!("Either --source or --expected-hash must be provided")
    }
}
