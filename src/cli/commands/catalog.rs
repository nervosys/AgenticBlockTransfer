use anyhow::Result;

use crate::cli::CatalogOpts;
use crate::core::rpicatalog;

pub async fn execute(opts: CatalogOpts) -> Result<()> {
    log::info!("Fetching Raspberry Pi OS catalog...");
    let catalog = rpicatalog::fetch_catalog().await?;

    if opts.json {
        if opts.flat {
            let flat = rpicatalog::flatten_downloadable(&catalog.os_list);
            println!("{}", serde_json::to_string_pretty(&flat)?);
        } else {
            println!("{}", serde_json::to_string_pretty(&catalog)?);
        }
        return Ok(());
    }

    if let Some(ref search) = opts.search {
        let needle = search.to_lowercase();
        let flat = rpicatalog::flatten_downloadable(&catalog.os_list);
        let matches: Vec<_> = flat
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&needle)
                    || e.description.to_lowercase().contains(&needle)
            })
            .collect();

        if matches.is_empty() {
            println!("No OS entries matching '{}'", search);
        } else {
            println!("Found {} matching entries:\n", matches.len());
            for (i, entry) in matches.iter().enumerate() {
                println!(
                    "  {}. {} [{}]",
                    i + 1,
                    entry.name,
                    entry.download_size_human()
                );
                if !entry.description.is_empty() {
                    println!("     {}", entry.description);
                }
                if !entry.url.is_empty() {
                    println!("     URL: {}", entry.url);
                }
            }
        }
    } else if opts.flat {
        let flat = rpicatalog::flatten_downloadable(&catalog.os_list);
        println!("Raspberry Pi OS Images ({} downloadable):\n", flat.len());
        for (i, entry) in flat.iter().enumerate() {
            println!(
                "  {}. {} [{}]",
                i + 1,
                entry.name,
                entry.download_size_human()
            );
        }
    } else {
        print!("{}", rpicatalog::format_catalog(&catalog));
    }

    Ok(())
}
