use anyhow::{anyhow, Result};

use crate::cli::CacheOpts;
use crate::core::cache::{EvictionPolicy, ImageCache};

pub async fn execute(opts: CacheOpts) -> Result<()> {
    let mut cache = if let Some(dir) = &opts.cache_dir {
        ImageCache::open(std::path::Path::new(dir))?
    } else {
        ImageCache::open_default()?
    };

    match opts.action.as_str() {
        "list" => {
            let entries = cache.list();
            if entries.is_empty() {
                println!("Cache is empty.");
                return Ok(());
            }
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                println!("{:<60} {:>10} {}", "URL", "Size", "Cached At");
                println!("{}", "-".repeat(90));
                for entry in &entries {
                    let size = humansize::format_size(entry.size, humansize::BINARY);
                    let date = chrono::DateTime::from_timestamp(entry.cached_at as i64, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_else(|| entry.cached_at.to_string());
                    let url_display = if entry.url.len() > 58 {
                        format!("{}...", &entry.url[..57])
                    } else {
                        entry.url.clone()
                    };
                    println!("{:<60} {:>10} {}", url_display, size, &date);
                }
                println!("\n{} entries", entries.len());
            }
        }
        "stats" => {
            let stats = cache.stats();
            if opts.json {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("Cache Statistics");
                println!("========================");
                println!(
                    "Total size:  {}",
                    humansize::format_size(stats.total_size_bytes, humansize::BINARY)
                );
                println!("Entries:     {}", stats.total_entries);
                println!("Verified:    {}", stats.verified_entries);
                println!("Unverified:  {}", stats.unverified_entries);
            }
        }
        "verify" => {
            let entries = cache.list();
            let urls: Vec<String> = entries.iter().map(|e| e.url.clone()).collect();
            let mut verified = 0u32;
            let mut failed = 0u32;
            for url in &urls {
                match cache.verify(url) {
                    Ok(true) => {
                        verified += 1;
                        log::info!("OK: {}", url);
                    }
                    Ok(false) => {
                        failed += 1;
                        log::warn!("FAILED: {}", url);
                    }
                    Err(e) => {
                        failed += 1;
                        log::error!("ERROR: {}: {}", url, e);
                    }
                }
            }
            println!("{} verified, {} failed", verified, failed);
        }
        "evict" => {
            let policy = match opts.evict_policy.as_deref() {
                Some("max-age") => {
                    let days = opts.evict_threshold.unwrap_or(30);
                    EvictionPolicy::MaxAge(days * 86400)
                }
                Some("max-entries") => {
                    EvictionPolicy::MaxEntries(opts.evict_threshold.unwrap_or(50) as usize)
                }
                Some("max-size") => {
                    EvictionPolicy::MaxSize(opts.evict_threshold.unwrap_or(10) * 1024 * 1024 * 1024)
                }
                _ => EvictionPolicy::MaxSize(10 * 1024 * 1024 * 1024),
            };
            let removed = cache.evict(&policy)?;
            println!("Evicted {} entries", removed);
        }
        "clear" => {
            let count = cache.list().len();
            cache.clear()?;
            println!("Cleared {} entries", count);
        }
        _ => {
            return Err(anyhow!(
                "unknown action '{}'. Use: list, stats, verify, evict, clear",
                opts.action
            ));
        }
    }
    Ok(())
}