// CLI command: abt update — check for new versions of abt.

use anyhow::Result;

use crate::cli::UpdateOpts;
use crate::core::update;

pub async fn execute(opts: UpdateOpts) -> Result<()> {
    if opts.dismiss {
        let check_opts = update::UpdateCheckOpts {
            repo: opts.repo.clone(),
            ..Default::default()
        };
        let result = update::check_for_updates(&update::UpdateCheckOpts {
            force: true,
            ..check_opts
        })
        .await?;
        if result.update_available {
            update::dismiss_update(&result.latest_version, &update::UpdateCheckOpts::default().state_path)?;
            println!("Dismissed update notification for v{}", result.latest_version);
        } else {
            println!("No update to dismiss — abt is up to date (v{})", result.current_version);
        }
        return Ok(());
    }

    let check_opts = update::UpdateCheckOpts {
        repo: opts.repo,
        include_prerelease: opts.prerelease,
        force: opts.force,
        ..Default::default()
    };

    let result = update::check_for_updates(&check_opts).await?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    println!("Current version: v{}", result.current_version);
    println!("Latest version:  v{}", result.latest_version);

    if result.update_available {
        println!();
        println!("📦 Update available: v{} → v{}", result.current_version, result.latest_version);
        println!("   Release: {}", result.release_url);

        if let Some(ref dl) = result.download_url {
            println!("   Download: {}", dl);
        }

        if !result.release_notes.is_empty() {
            println!();
            println!("Release notes:");
            for line in result.release_notes.lines().take(10) {
                println!("  {}", line);
            }
        }
    } else {
        println!("✓ abt is up to date");
    }

    Ok(())
}
