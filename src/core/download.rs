// HTTP/HTTPS download module — streaming download with progress tracking.
//
// Enables `abt write -i https://releases.ubuntu.com/.../ubuntu.iso -o /dev/sdb`
// by downloading the image to a temp file with real-time progress, then handing
// the local path to the write engine. Supports:
//   - HTTPS (via rustls, no OpenSSL dependency)
//   - Content-Length progress tracking
//   - Cancel-safe: partial downloads cleaned up on Ctrl+C
//   - Integrity: returns path only on complete download

use anyhow::Result;
use futures::StreamExt;
use log::info;
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

use super::progress::Progress;

/// Build a shared reqwest client with sensible defaults.
fn build_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(format!("abt/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(7200)) // 2 hour max for large images
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?)
}

/// Send a GET request and validate the HTTP status.
async fn validated_get(client: &reqwest::Client, url: &str) -> Result<reqwest::Response> {
    let response = client.get(url).send().await?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!(
            "HTTP {} {} for URL: {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("Unknown"),
            url
        );
    }
    Ok(response)
}

/// Streaming download — reads chunks as they arrive from the network,
/// writing to disk incrementally. Suitable for multi-GB images.
///
/// Progress is reported through the shared `Progress` handle.
/// Cancellation (via `progress.cancel()`) aborts the download and
/// removes the partial temp file.
pub async fn download_streaming(
    url: &str,
    progress: &Progress,
) -> Result<PathBuf> {
    info!("Streaming download: {}", url);

    let client = build_client()?;
    let response = validated_get(&client, url).await?;

    let content_length = response.content_length().unwrap_or(0);
    if content_length > 0 {
        progress.set_total(content_length);
        info!("Download size: {} bytes", content_length);
    } else {
        info!("Download size: unknown (no Content-Length header)");
    }

    // Extract filename and prepare temp directory
    let filename = extract_filename(url);
    let temp_dir = std::env::temp_dir().join("abt_downloads");
    tokio::fs::create_dir_all(&temp_dir).await?;
    let temp_path = temp_dir.join(&filename);

    // Stream chunks to disk
    let mut file = tokio::fs::File::create(&temp_path).await?;
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;

    while let Some(chunk_result) = stream.next().await {
        if progress.is_cancelled() {
            drop(file);
            let _ = tokio::fs::remove_file(&temp_path).await;
            anyhow::bail!("Download cancelled by user");
        }

        let chunk = chunk_result?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress.add_bytes(chunk.len() as u64);
    }

    file.flush().await?;
    file.sync_all().await?;

    info!(
        "Download complete: {} ({} bytes)",
        temp_path.display(),
        downloaded
    );

    if content_length > 0 && downloaded != content_length {
        let _ = tokio::fs::remove_file(&temp_path).await;
        anyhow::bail!(
            "Download incomplete: expected {} bytes, got {}",
            content_length,
            downloaded
        );
    }

    Ok(temp_path)
}

/// Extract a reasonable filename from a URL.
pub fn extract_filename(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or("download")
        .split('?')
        .next()
        .unwrap_or("download")
        .to_string()
}

/// Clean up a downloaded temp file.
pub fn cleanup_download(path: &std::path::Path) {
    if path.starts_with(std::env::temp_dir().join("abt_downloads")) {
        if let Err(e) = std::fs::remove_file(path) {
            log::warn!("Failed to clean up temp download {}: {}", path.display(), e);
        } else {
            log::info!("Cleaned up temp download: {}", path.display());
        }
    }
}
