//! Fetching a source video from a URL via `yt-dlp`.
//!
//! There is no dependable pure-Rust YouTube downloader — the site's signature
//! cipher changes often enough that only an actively-maintained tool like
//! yt-dlp keeps working. So this shells out to it. yt-dlp is invoked with an
//! argv array (never a shell string) and the URL is passed after `--`, so a
//! pasted link cannot be read as a flag or inject a command.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

/// yt-dlp accepts URLs for a thousand sites; we only gate on the transport so
/// the string is a real URL and not an option or a local path.
pub fn valid_url(url: &str) -> bool {
    let u = url.trim();
    (u.starts_with("http://") || u.starts_with("https://"))
        && !u.contains(char::is_whitespace)
        && u.len() < 2048
}

/// yt-dlp's reported version, or `None` if the binary is missing / not runnable.
pub async fn available() -> Option<String> {
    let out = Command::new(crate::config::ytdlp_path())
        .arg("--version")
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn missing_error() -> anyhow::Error {
    anyhow!(
        "chưa cài yt-dlp — công cụ dùng để tải video từ YouTube. Cài bằng: brew install yt-dlp"
    )
}

/// `--cookies-from-browser <b>` when a browser is configured, else nothing.
///
/// Returned as owned strings so the args outlive the borrow in the command
/// builder.
fn cookie_args() -> Vec<String> {
    let browser = crate::config::ytdlp_cookies_browser();
    if browser.trim().is_empty() {
        Vec::new()
    } else {
        vec!["--cookies-from-browser".to_string(), browser]
    }
}

#[derive(Debug, Clone, serde::Serialize, Default)]
pub struct Meta {
    pub title: String,
    pub uploader: String,
    pub duration_sec: f64,
    pub thumbnail: String,
    pub extractor: String,
}

/// Read a video's metadata without downloading it. Best-effort: used to name a
/// project and to show a preview, so a failure here should not block a download.
pub async fn probe(url: &str) -> Result<Meta> {
    if !valid_url(url) {
        bail!("URL không hợp lệ — chỉ nhận đường dẫn http/https");
    }
    if available().await.is_none() {
        return Err(missing_error());
    }

    let out = Command::new(crate::config::ytdlp_path())
        .args(["--dump-single-json", "--no-warnings", "--no-playlist"])
        .args(cookie_args())
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .await
        .context("chạy yt-dlp để lấy thông tin video")?;

    if !out.status.success() {
        bail!("{}", clean_ytdlp_error(&out.stderr));
    }

    let v: Value = serde_json::from_slice(&out.stdout)
        .context("yt-dlp trả về thông tin không đọc được")?;

    Ok(Meta {
        title: v["title"].as_str().unwrap_or("").to_string(),
        uploader: v["uploader"].as_str().unwrap_or("").to_string(),
        duration_sec: v["duration"].as_f64().unwrap_or(0.0),
        thumbnail: v["thumbnail"].as_str().unwrap_or("").to_string(),
        extractor: v["extractor_key"].as_str().unwrap_or("").to_string(),
    })
}

pub struct Downloaded {
    pub path: PathBuf,
    pub filename: String,
    pub mime: String,
    pub size: u64,
}

/// Download `url` into `dir`, capping resolution and file size. Returns the
/// resulting mp4.
///
/// The output name is fixed up front (`stem`) so the merged file can be found
/// deterministically: yt-dlp's own `--print filepath` reports the pre-merge
/// name, which is unreliable once streams are muxed.
pub async fn download(url: &str, dir: &Path, stem: &str) -> Result<Downloaded> {
    if !valid_url(url) {
        bail!("URL không hợp lệ — chỉ nhận đường dẫn http/https");
    }
    if available().await.is_none() {
        return Err(missing_error());
    }
    tokio::fs::create_dir_all(dir)
        .await
        .with_context(|| format!("tạo thư mục {}", dir.display()))?;

    let height = crate::config::youtube_max_height();
    // Prefer an mp4 pair, fall back to the best single stream within the cap;
    // `--merge-output-format mp4` remuxes to mp4 when the source is webm/mkv.
    let format = format!(
        "bestvideo[height<={h}][ext=mp4]+bestaudio[ext=m4a]/best[height<={h}][ext=mp4]/best[height<={h}]/best",
        h = height
    );
    let out_template = dir.join(format!("{stem}.%(ext)s"));

    let out = Command::new(crate::config::ytdlp_path())
        .args([
            "--no-playlist",
            "--no-warnings",
            "--no-part",
            "--merge-output-format",
            "mp4",
            "--max-filesize",
            &crate::config::youtube_max_filesize(),
            "-f",
            &format,
            "-o",
        ])
        .arg(&out_template)
        .args(cookie_args())
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .await
        .context("chạy yt-dlp để tải video")?;

    if !out.status.success() {
        bail!("{}", clean_ytdlp_error(&out.stderr));
    }

    // Find whatever extension the merge settled on (mp4 in practice).
    let path = find_downloaded(dir, stem)
        .await
        .ok_or_else(|| anyhow!("tải xong nhưng không tìm thấy file — có thể video vượt quá giới hạn dung lượng"))?;
    let size = tokio::fs::metadata(&path).await?.len();
    if size == 0 {
        let _ = tokio::fs::remove_file(&path).await;
        bail!("file tải về rỗng");
    }

    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("{stem}.mp4"));
    let mime = mime_for(&path);

    Ok(Downloaded {
        path,
        filename,
        mime,
        size,
    })
}

async fn find_downloaded(dir: &Path, stem: &str) -> Option<PathBuf> {
    let mut rd = tokio::fs::read_dir(dir).await.ok()?;
    // Prefer mp4 if several intermediate files linger.
    let mut fallback: Option<PathBuf> = None;
    while let Ok(Some(entry)) = rd.next_entry().await {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(stem) {
            continue;
        }
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("mp4") {
            return Some(p);
        }
        fallback = Some(p);
    }
    fallback
}

fn mime_for(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("mov") => "video/quicktime",
        _ => "video/mp4",
    }
    .to_string()
}

/// Turn yt-dlp's multi-line stderr into a single readable sentence.
fn clean_ytdlp_error(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text
        .lines()
        .rev()
        .find(|l| l.trim_start().to_uppercase().starts_with("ERROR"))
        .or_else(|| text.lines().rev().find(|l| !l.trim().is_empty()))
        .unwrap_or("yt-dlp thất bại không rõ lý do")
        .trim()
        .trim_start_matches("ERROR:")
        .trim();
    crate::scenes::truncate_chars(line, 300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_http_urls_are_accepted() {
        assert!(valid_url("https://youtu.be/abc123"));
        assert!(valid_url("http://example.com/v.mp4"));
        // An option-looking string must not slip through as a URL.
        assert!(!valid_url("-f best"));
        assert!(!valid_url("file:///etc/passwd"));
        assert!(!valid_url("ftp://x/y"));
        assert!(!valid_url(""));
        // A URL with whitespace could smuggle a second argument.
        assert!(!valid_url("https://x/y --exec rm"));
    }

    #[test]
    fn a_too_long_url_is_rejected() {
        let long = format!("https://x/{}", "a".repeat(3000));
        assert!(!valid_url(&long));
    }

    #[test]
    fn mime_follows_the_container_extension() {
        assert_eq!(mime_for(Path::new("/x/v.mp4")), "video/mp4");
        assert_eq!(mime_for(Path::new("/x/v.webm")), "video/webm");
        assert_eq!(mime_for(Path::new("/x/v")), "video/mp4");
    }

    #[test]
    fn ytdlp_error_is_reduced_to_the_error_line() {
        let stderr = b"[youtube] Extracting URL\n[info] downloading\nERROR: Video unavailable\n";
        assert_eq!(clean_ytdlp_error(stderr), "Video unavailable");
    }

    #[test]
    fn ytdlp_error_falls_back_to_the_last_nonempty_line() {
        let stderr = b"something went wrong\n\n";
        assert_eq!(clean_ytdlp_error(stderr), "something went wrong");
    }
}
