// Resumable HTTP download module — download large images with resume support.
//
// Uses HTTP Range headers to resume interrupted downloads, keeping `.part` files
// until the download is complete. This avoids re-downloading multi-GB images when
// the connection drops or the user cancels and retries.
//
// Inspired by MediaWriter's DownloadManager and rpi-imager's downloadthread.

use anyhow::{Context, Result};
use futures::StreamExt;
use log::{info, warn};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use super::progress::Progress;

/// Metadata for a partial download, stored alongside the `.part` file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PartialDownloadMeta {
    /// Original URL being downloaded.
    pub url: String,
    /// Expected total size (from Content-Length), 0 if unknown.
    pub total_size: u64,
    /// Bytes downloaded so far.
    pub downloaded: u64,
    /// ETag from the server (used to detect file changes between resume attempts).
    pub etag: Option<String>,
    /// Last-Modified header value.
    pub last_modified: Option<String>,
    /// Timestamp when the download was started.
    pub started_at: u64,
    /// Timestamp of the last successful chunk write.
    pub last_chunk_at: u64,
}

/// Options for resumable download.
#[derive(Debug, Clone)]
pub struct ResumeDownloadOpts {
    /// Download URL.
    pub url: String,
    /// Directory to store the downloaded file (and its .part file during download).
    pub output_dir: PathBuf,
    /// Optional filename override (default: extracted from URL).
    pub filename: Option<String>,
    /// Maximum number of retry attempts on transient failure.
    pub max_retries: u32,
    /// Delay between retries in seconds (exponential backoff base).
    pub retry_delay_secs: u64,
    /// Whether to force a fresh download (ignore existing .part files).
    pub force_fresh: bool,
}

impl Default for ResumeDownloadOpts {
    fn default() -> Self {
        Self {
            url: String::new(),
            output_dir: std::env::temp_dir().join("abt_downloads"),
            filename: None,
            max_retries: 3,
            retry_delay_secs: 2,
            force_fresh: false,
        }
    }
}

/// Result of a completed download.
#[derive(Debug, Clone)]
pub struct DownloadResult {
    /// Path to the completed file.
    pub path: PathBuf,
    /// Total bytes downloaded (across all resume attempts).
    pub total_bytes: u64,
    /// Number of resume attempts that were needed.
    pub resume_count: u32,
    /// Whether the download was resumed from a partial file.
    pub was_resumed: bool,
}

/// Build a shared reqwest client with sensible defaults.
fn build_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(format!("abt/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(7200))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?)
}

/// Extract a reasonable filename from a URL.
fn extract_filename(url: &str) -> String {
    url.rsplit('/')
        .next()
        .unwrap_or("download")
        .split('?')
        .next()
        .unwrap_or("download")
        .to_string()
}

/// Path for the `.part` (partial download) file.
fn part_path(final_path: &Path) -> PathBuf {
    let mut p = final_path.as_os_str().to_os_string();
    p.push(".part");
    PathBuf::from(p)
}

/// Path for the download metadata JSON file.
fn meta_path(final_path: &Path) -> PathBuf {
    let mut p = final_path.as_os_str().to_os_string();
    p.push(".meta.json");
    PathBuf::from(p)
}

/// Load partial download metadata if it exists and is valid.
async fn load_meta(path: &Path) -> Option<PartialDownloadMeta> {
    let data = tokio::fs::read_to_string(path).await.ok()?;
    serde_json::from_str(&data).ok()
}

/// Save partial download metadata.
async fn save_meta(path: &Path, meta: &PartialDownloadMeta) -> Result<()> {
    let data = serde_json::to_string_pretty(meta)?;
    tokio::fs::write(path, data).await?;
    Ok(())
}

/// Check if the server supports Range requests by sending a HEAD request.
async fn check_range_support(client: &reqwest::Client, url: &str) -> Result<(bool, u64, Option<String>, Option<String>)> {
    let resp = client.head(url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("HEAD request failed: HTTP {}", resp.status().as_u16());
    }

    let accepts_ranges = resp
        .headers()
        .get("accept-ranges")
        .and_then(|v| v.to_str().ok())
        .map(|v| v != "none")
        .unwrap_or(false);

    let content_length = resp.content_length().unwrap_or(0);

    let etag = resp
        .headers()
        .get("etag")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let last_modified = resp
        .headers()
        .get("last-modified")
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    Ok((accepts_ranges, content_length, etag, last_modified))
}

/// Check if the server file has changed since our partial download.
fn server_file_changed(meta: &PartialDownloadMeta, etag: &Option<String>, last_modified: &Option<String>) -> bool {
    if let (Some(meta_etag), Some(server_etag)) = (&meta.etag, etag) {
        if meta_etag != server_etag {
            warn!("Server ETag changed: {} -> {}", meta_etag, server_etag);
            return true;
        }
    }
    if let (Some(meta_lm), Some(server_lm)) = (&meta.last_modified, last_modified) {
        if meta_lm != server_lm {
            warn!("Server Last-Modified changed: {} -> {}", meta_lm, server_lm);
            return true;
        }
    }
    false
}

/// Download a file with resume support.
///
/// If a `.part` file exists from a previous interrupted download, and the server
/// supports HTTP Range requests, the download will resume from where it left off.
///
/// # Arguments
/// * `opts` — Download options (URL, output directory, retry settings).
/// * `progress` — Shared progress handle for UI reporting and cancellation.
///
/// # Returns
/// A `DownloadResult` with the path to the completed file and transfer stats.
pub async fn download_with_resume(
    opts: &ResumeDownloadOpts,
    progress: &Progress,
) -> Result<DownloadResult> {
    let client = build_client()?;

    // Determine filenames and paths
    let filename = opts.filename.clone().unwrap_or_else(|| extract_filename(&opts.url));
    let final_path = opts.output_dir.join(&filename);
    let part = part_path(&final_path);
    let meta = meta_path(&final_path);

    tokio::fs::create_dir_all(&opts.output_dir).await?;

    // If final file already exists, skip download
    if final_path.exists() && !opts.force_fresh {
        info!("File already exists: {}", final_path.display());
        let file_size = tokio::fs::metadata(&final_path).await?.len();
        return Ok(DownloadResult {
            path: final_path,
            total_bytes: file_size,
            resume_count: 0,
            was_resumed: false,
        });
    }

    // Check server capabilities
    let (supports_range, total_size, server_etag, server_lm) =
        check_range_support(&client, &opts.url).await?;

    if total_size > 0 {
        progress.set_total(total_size);
        info!("Download size: {} bytes", total_size);
    }

    // Determine resume point
    let mut resume_offset: u64 = 0;
    let mut resume_count: u32 = 0;
    let mut was_resumed = false;

    if !opts.force_fresh && supports_range && part.exists() {
        if let Some(existing_meta) = load_meta(&meta).await {
            if !server_file_changed(&existing_meta, &server_etag, &server_lm) {
                let part_size = tokio::fs::metadata(&part).await?.len();
                if part_size <= total_size || total_size == 0 {
                    resume_offset = part_size;
                    was_resumed = true;
                    info!("Resuming download from byte {}", resume_offset);
                    progress.add_bytes(resume_offset);
                }
            } else {
                info!("Server file changed — restarting download from scratch");
                let _ = tokio::fs::remove_file(&part).await;
                let _ = tokio::fs::remove_file(&meta).await;
            }
        }
    } else if opts.force_fresh {
        let _ = tokio::fs::remove_file(&part).await;
        let _ = tokio::fs::remove_file(&meta).await;
    }

    // Retry loop
    let mut attempt = 0;
    let mut current_offset = resume_offset;

    loop {
        match download_chunk(&client, &opts.url, &part, &meta, current_offset, total_size, &server_etag, &server_lm, progress).await {
            Ok(bytes_total) => {
                // Download complete — rename .part to final
                tokio::fs::rename(&part, &final_path)
                    .await
                    .context("Failed to rename .part file to final destination")?;
                let _ = tokio::fs::remove_file(&meta).await;

                info!("Download complete: {} ({} bytes)", final_path.display(), bytes_total);

                return Ok(DownloadResult {
                    path: final_path,
                    total_bytes: bytes_total,
                    resume_count,
                    was_resumed,
                });
            }
            Err(e) => {
                if progress.is_cancelled() {
                    anyhow::bail!("Download cancelled by user");
                }

                attempt += 1;
                if attempt > opts.max_retries {
                    anyhow::bail!(
                        "Download failed after {} attempts: {}",
                        opts.max_retries,
                        e
                    );
                }

                // Update offset to current part file size for resume
                if let Ok(m) = tokio::fs::metadata(&part).await {
                    current_offset = m.len();
                }
                resume_count += 1;

                let delay = opts.retry_delay_secs * (1 << attempt.min(5));
                warn!(
                    "Download attempt {} failed ({}), retrying in {}s from byte {}...",
                    attempt, e, delay, current_offset
                );
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
            }
        }
    }
}

/// Download a chunk (or the full file) starting from `offset`.
async fn download_chunk(
    client: &reqwest::Client,
    url: &str,
    part_path: &Path,
    meta_path: &Path,
    offset: u64,
    total_size: u64,
    etag: &Option<String>,
    last_modified: &Option<String>,
    progress: &Progress,
) -> Result<u64> {
    let mut request = client.get(url);

    // Add Range header if resuming
    if offset > 0 {
        request = request.header("Range", format!("bytes={}-", offset));
    }

    let response = request.send().await?;
    let status = response.status();

    // 206 = Partial Content (resume), 200 = full download
    if status != reqwest::StatusCode::PARTIAL_CONTENT && status != reqwest::StatusCode::OK {
        anyhow::bail!("HTTP {} for {}", status.as_u16(), url);
    }

    // If server returned 200 instead of 206 when we requested a range,
    // it doesn't support resume — start from scratch
    let actual_offset = if offset > 0 && status == reqwest::StatusCode::OK {
        warn!("Server returned 200 instead of 206 — restarting from beginning");
        0
    } else {
        offset
    };

    // Open part file: append if resuming, create if fresh
    let mut file = if actual_offset > 0 {
        let f = tokio::fs::OpenOptions::new()
            .write(true)
            .open(part_path)
            .await?;
        f.set_len(actual_offset).await?; // truncate to known-good offset
        let mut f = f;
        f.seek(std::io::SeekFrom::Start(actual_offset)).await?;
        f
    } else {
        tokio::fs::File::create(part_path).await?
    };

    // Save metadata
    let now = chrono::Utc::now().timestamp() as u64;
    let dl_meta = PartialDownloadMeta {
        url: url.to_string(),
        total_size,
        downloaded: actual_offset,
        etag: etag.clone(),
        last_modified: last_modified.clone(),
        started_at: now,
        last_chunk_at: now,
    };
    save_meta(meta_path, &dl_meta).await?;

    // Stream chunks
    let mut stream = response.bytes_stream();
    let mut downloaded = actual_offset;
    let mut last_meta_update = std::time::Instant::now();

    while let Some(chunk_result) = stream.next().await {
        if progress.is_cancelled() {
            file.flush().await?;
            // Update metadata so we can resume later.
            let updated_meta = PartialDownloadMeta {
                downloaded,
                last_chunk_at: chrono::Utc::now().timestamp() as u64,
                ..dl_meta.clone()
            };
            let _ = save_meta(meta_path, &updated_meta).await;
            anyhow::bail!("Download cancelled");
        }

        let chunk = chunk_result?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        progress.add_bytes(chunk.len() as u64);

        // Periodically update metadata (every 5 seconds) so we can resume
        if last_meta_update.elapsed() > std::time::Duration::from_secs(5) {
            let updated_meta = PartialDownloadMeta {
                downloaded,
                last_chunk_at: chrono::Utc::now().timestamp() as u64,
                ..dl_meta.clone()
            };
            let _ = save_meta(meta_path, &updated_meta).await;
            last_meta_update = std::time::Instant::now();
        }
    }

    file.flush().await?;
    file.sync_all().await?;

    // Verify size
    if total_size > 0 && downloaded != total_size {
        anyhow::bail!(
            "Incomplete download: expected {} bytes, got {}",
            total_size,
            downloaded
        );
    }

    Ok(downloaded)
}

/// Get the status of a partial download (if any).
pub async fn get_partial_status(output_dir: &Path, url: &str) -> Option<PartialDownloadMeta> {
    let filename = extract_filename(url);
    let final_path = output_dir.join(&filename);
    let meta = meta_path(&final_path);
    load_meta(&meta).await
}

/// Clean up partial download files.
pub async fn cleanup_partial(output_dir: &Path, url: &str) -> Result<()> {
    let filename = extract_filename(url);
    let final_path = output_dir.join(&filename);
    let part = part_path(&final_path);
    let meta = meta_path(&final_path);

    if part.exists() {
        tokio::fs::remove_file(&part).await?;
        info!("Removed partial download: {}", part.display());
    }
    if meta.exists() {
        tokio::fs::remove_file(&meta).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_filename() {
        assert_eq!(
            extract_filename("https://example.com/images/ubuntu.iso"),
            "ubuntu.iso"
        );
        assert_eq!(
            extract_filename("https://example.com/dl?file=image.img&v=2"),
            "dl"
        );
        assert_eq!(extract_filename("https://example.com/"), "");
    }

    #[test]
    fn test_part_path() {
        let p = PathBuf::from("/tmp/abt/ubuntu.iso");
        assert_eq!(part_path(&p), PathBuf::from("/tmp/abt/ubuntu.iso.part"));
    }

    #[test]
    fn test_meta_path() {
        let p = PathBuf::from("/tmp/abt/ubuntu.iso");
        assert_eq!(
            meta_path(&p),
            PathBuf::from("/tmp/abt/ubuntu.iso.meta.json")
        );
    }

    #[test]
    fn test_partial_download_meta_serde() {
        let meta = PartialDownloadMeta {
            url: "https://example.com/image.iso".into(),
            total_size: 1_000_000,
            downloaded: 500_000,
            etag: Some("\"abc123\"".into()),
            last_modified: Some("Thu, 01 Jan 2025 00:00:00 GMT".into()),
            started_at: 1700000000,
            last_chunk_at: 1700001000,
        };

        let json = serde_json::to_string(&meta).unwrap();
        let deserialized: PartialDownloadMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.url, meta.url);
        assert_eq!(deserialized.total_size, meta.total_size);
        assert_eq!(deserialized.downloaded, meta.downloaded);
        assert_eq!(deserialized.etag, meta.etag);
    }

    #[test]
    fn test_server_file_changed_etag() {
        let meta = PartialDownloadMeta {
            url: "https://example.com/image.iso".into(),
            total_size: 1_000_000,
            downloaded: 500_000,
            etag: Some("\"v1\"".into()),
            last_modified: None,
            started_at: 0,
            last_chunk_at: 0,
        };

        // Same ETag
        assert!(!server_file_changed(
            &meta,
            &Some("\"v1\"".into()),
            &None
        ));
        // Different ETag
        assert!(server_file_changed(
            &meta,
            &Some("\"v2\"".into()),
            &None
        ));
        // No server ETag (can't tell if changed)
        assert!(!server_file_changed(&meta, &None, &None));
    }

    #[test]
    fn test_server_file_changed_last_modified() {
        let meta = PartialDownloadMeta {
            url: "https://example.com/image.iso".into(),
            total_size: 0,
            downloaded: 0,
            etag: None,
            last_modified: Some("Mon, 01 Jan 2024 00:00:00 GMT".into()),
            started_at: 0,
            last_chunk_at: 0,
        };

        assert!(!server_file_changed(
            &meta,
            &None,
            &Some("Mon, 01 Jan 2024 00:00:00 GMT".into())
        ));
        assert!(server_file_changed(
            &meta,
            &None,
            &Some("Tue, 02 Jan 2024 00:00:00 GMT".into())
        ));
    }

    #[test]
    fn test_resume_download_opts_default() {
        let opts = ResumeDownloadOpts::default();
        assert_eq!(opts.max_retries, 3);
        assert_eq!(opts.retry_delay_secs, 2);
        assert!(!opts.force_fresh);
        assert!(opts.filename.is_none());
    }

    #[test]
    fn test_download_result_fields() {
        let result = DownloadResult {
            path: PathBuf::from("/tmp/test.iso"),
            total_bytes: 1024,
            resume_count: 2,
            was_resumed: true,
        };
        assert!(result.was_resumed);
        assert_eq!(result.resume_count, 2);
    }

    #[tokio::test]
    async fn test_save_and_load_meta() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.meta.json");
        let meta = PartialDownloadMeta {
            url: "https://example.com/test.img".into(),
            total_size: 2048,
            downloaded: 1024,
            etag: Some("\"etag1\"".into()),
            last_modified: None,
            started_at: 1700000000,
            last_chunk_at: 1700000500,
        };

        save_meta(&path, &meta).await.unwrap();
        let loaded = load_meta(&path).await.unwrap();
        assert_eq!(loaded.url, "https://example.com/test.img");
        assert_eq!(loaded.downloaded, 1024);
        assert_eq!(loaded.total_size, 2048);
    }

    #[tokio::test]
    async fn test_load_meta_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.meta.json");
        assert!(load_meta(&path).await.is_none());
    }

    #[tokio::test]
    async fn test_cleanup_partial() {
        let dir = tempfile::tempdir().unwrap();
        let url = "https://example.com/test.iso";
        let filename = extract_filename(url);
        let final_path = dir.path().join(&filename);
        let part = part_path(&final_path);
        let meta = meta_path(&final_path);

        tokio::fs::write(&part, b"partial data").await.unwrap();
        tokio::fs::write(&meta, b"{}").await.unwrap();

        cleanup_partial(dir.path(), url).await.unwrap();
        assert!(!part.exists());
        assert!(!meta.exists());
    }
}
