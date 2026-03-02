// CLI command: signature — verify signed downloads

use anyhow::Result;
use log::info;

use crate::cli::SignatureOpts;
use crate::core::signature;

pub async fn execute(opts: SignatureOpts) -> Result<()> {
    if let Some(ref sig_path) = opts.signature {
        // Verify a local file against a local signature
        info!("Verifying {} with signature {}", opts.file, sig_path);

        let key_ring = if let Some(ref kr_path) = opts.keyring {
            signature::KeyRing::load(std::path::Path::new(kr_path))?
        } else {
            signature::KeyRing::new()
        };

        let result = signature::verify_local_file(
            std::path::Path::new(&opts.file),
            std::path::Path::new(sig_path),
            &key_ring,
        )?;

        if opts.json {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else if result.valid {
            println!("Signature VALID");
            println!("  File hash: {}", result.file_hash);
            if let Some(ref key_id) = result.key_id {
                println!("  Verified with key: {}", key_id);
            }
        } else {
            println!("Signature INVALID");
            println!("  File hash: {}", result.file_hash);
            if let Some(ref err) = result.error {
                println!("  Error: {}", err);
            }
        }
    } else {
        // Hash a file and display the result
        let hash = signature::hash_file_sha256(std::path::Path::new(&opts.file))?;
        let hex_str = hex::encode(&hash);
        if opts.json {
            println!(
                "{}",
                serde_json::json!({"file": opts.file, "sha256": hex_str})
            );
        } else {
            println!("{}  {}", hex_str, opts.file);
        }
    }

    Ok(())
}
