// CLI command: abt mirror — download with mirror selection and failover.

use anyhow::Result;
use log::info;

use crate::cli::MirrorOpts;
use crate::core::mirror;
use crate::core::progress::Progress;

pub async fn execute(opts: MirrorOpts) -> Result<()> {
    match opts.action.as_str() {
        "probe" => probe_mirrors(&opts).await,
        "download" => download_from_mirrors(&opts).await,
        "list" => list_mirrors(&opts).await,
        _ => {
            anyhow::bail!(
                "Unknown mirror action '{}'. Use: probe, download, list",
                opts.action
            );
        }
    }
}

async fn probe_mirrors(opts: &MirrorOpts) -> Result<()> {
    let mut mirror_list = get_mirror_list(opts).await?;

    mirror::probe_and_sort(&mut mirror_list).await;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&mirror_list)?);
        return Ok(());
    }

    println!("Mirror probe results ({} mirrors):", mirror_list.mirrors.len());
    println!();

    for (i, m) in mirror_list.mirrors.iter().enumerate() {
        let status = if m.failed {
            "FAILED".to_string()
        } else if m.latency_ms > 0 {
            format!("{}ms", m.latency_ms)
        } else {
            "not probed".to_string()
        };

        println!(
            "  {}. {} ({}) — {}",
            i + 1,
            if m.name.is_empty() { &m.url } else { &m.name },
            if m.location.is_empty() { "??" } else { &m.location },
            status
        );
    }

    if let Some(best) = mirror_list.best_mirror() {
        println!();
        println!("Best mirror: {}", best.url);
    }

    Ok(())
}

async fn download_from_mirrors(opts: &MirrorOpts) -> Result<()> {
    let path = opts.path.as_ref().ok_or_else(|| {
        anyhow::anyhow!("--path is required for mirror download")
    })?;

    let mut mirror_list = get_mirror_list(opts).await?;
    let progress = Progress::new(0);
    let output_dir = std::path::PathBuf::from(&opts.output_dir);

    info!("Downloading {} from {} mirrors", path, mirror_list.mirrors.len());

    let result = mirror::download_with_failover(
        &mut mirror_list,
        path,
        &output_dir,
        &progress,
    )
    .await?;

    if opts.json {
        println!(
            "{}",
            serde_json::json!({
                "path": result.display().to_string(),
                "success": true
            })
        );
    } else {
        println!("Downloaded: {}", result.display());
    }

    Ok(())
}

async fn list_mirrors(opts: &MirrorOpts) -> Result<()> {
    let mirror_list = get_mirror_list(opts).await?;

    if opts.json {
        println!("{}", serde_json::to_string_pretty(&mirror_list)?);
        return Ok(());
    }

    println!("Mirrors ({}):", mirror_list.mirrors.len());
    for (i, m) in mirror_list.mirrors.iter().enumerate() {
        println!(
            "  {}. {} [priority={}] {}",
            i + 1,
            m.url,
            m.priority,
            if m.location.is_empty() {
                String::new()
            } else {
                format!("({})", m.location)
            }
        );
    }

    Ok(())
}

async fn get_mirror_list(opts: &MirrorOpts) -> Result<mirror::MirrorList> {
    if let Some(ref metalink_url) = opts.metalink {
        let client = reqwest::Client::builder()
            .user_agent(format!("abt/{}", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        let resp = client.get(metalink_url).send().await?;
        let content = resp.text().await?;
        let info = mirror::parse_metalink_urls(&content)?;

        Ok(mirror::MirrorList {
            mirrors: info.mirrors,
            ..Default::default()
        })
    } else if let Some(ref list_url) = opts.mirror_list {
        mirror::fetch_mirror_list(list_url).await
    } else {
        anyhow::bail!("Provide --mirror-list URL or --metalink URL")
    }
}
