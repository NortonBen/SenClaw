//! Every environment-variable read in the app, in one place.

use std::path::PathBuf;

fn env_or(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

/// App HTTP port — injected by the SenClaw daemon as PORT (manifest: 4480).
pub fn http_port() -> String {
    env_or("PORT", "4480")
}

pub fn app_id() -> String {
    env_or("SENCLAW_SPACE_APP_ID", "video-cloner")
}

/// App data root — deliberately OUTSIDE the install directory.
///
/// Installing a Space App zip does `remove_dir_all(<app_dir>)` before
/// extracting, so anything kept next to the binary (the SQLite DB, and here
/// also the uploaded videos) is destroyed on every update. Data therefore lives
/// in a stable per-app directory under the user's home;
/// `VIDEO_CLONER_DATA_DIR` overrides it.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("VIDEO_CLONER_DATA_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    match std::env::var("HOME").ok().filter(|h| !h.is_empty()) {
        Some(home) => PathBuf::from(home)
            .join(".senclaw")
            .join("space-app-data")
            .join(app_id()),
        None => PathBuf::from("."),
    }
}

pub fn db_path() -> String {
    data_dir().join("app.sqlite").to_string_lossy().to_string()
}

/// Where uploaded videos and character reference images are stored.
///
/// Videos are large and are re-read on every "analyse the next segment" call,
/// so they are kept on disk for the life of the project rather than held in the
/// database or in memory.
pub fn media_dir() -> PathBuf {
    data_dir().join("media")
}

/// Base URL of the SenClaw daemon's UI server.
///
/// Used to write wiki pages. Note the app talks to this REST API *directly*
/// rather than through the Space bridge: the bridge advertises a `space.rest`
/// capability but has no handler for it, so a bridge call would just error.
pub fn senclaw_base_url() -> String {
    env_or("SENCLAW_BASE_URL", "http://127.0.0.1:18788")
}

/// Where exported bundles are written for other apps to pick up.
///
/// Deliberately outside this app's own data directory — the whole point is that
/// a different Space App can read it without knowing where video-cloner keeps
/// its database.
pub fn export_dir() -> PathBuf {
    if let Ok(d) = std::env::var("VIDEO_CLONER_EXPORT_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    match std::env::var("HOME").ok().filter(|h| !h.is_empty()) {
        Some(home) => PathBuf::from(home)
            .join(".senclaw")
            .join("exports")
            .join(app_id()),
        None => PathBuf::from("exports"),
    }
}

/// Base URL of the video-flow Space App, the downstream video generator.
pub fn video_flow_url() -> String {
    env_or("VIDEO_FLOW_URL", "http://127.0.0.1:4460")
}

/// Path to the `yt-dlp` binary used to fetch videos from YouTube (and any other
/// site it supports). Overridable because Homebrew, pipx and system installs
/// land it in different places.
pub fn ytdlp_path() -> String {
    env_or("VIDEO_CLONER_YTDLP", "yt-dlp")
}

/// Cap on downloaded resolution. Gemini re-reads the whole file on every
/// segment, so pulling a 4K master when 720p analyses identically just wastes
/// disk and upload time.
pub fn youtube_max_height() -> u32 {
    std::env::var("VIDEO_CLONER_YOUTUBE_MAX_HEIGHT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|h| *h >= 144)
        .unwrap_or(720)
}

/// Hard ceiling passed to yt-dlp's `--max-filesize`, so a mis-pasted link to a
/// three-hour stream cannot fill the disk. yt-dlp size syntax (e.g. "500M").
pub fn youtube_max_filesize() -> String {
    env_or("VIDEO_CLONER_YOUTUBE_MAX_FILESIZE", "500M")
}

/// Browser to pull cookies from for yt-dlp (`--cookies-from-browser`).
///
/// YouTube throttles anonymous downloads with a "confirm you're not a bot"
/// block after a few requests from one IP. Pointing yt-dlp at the user's own
/// logged-in browser cookies clears it. Off by default (empty) because it reads
/// the browser's cookie store, which the user should opt into explicitly.
/// Values: `chrome`, `safari`, `firefox`, `edge`, `brave`, …
pub fn ytdlp_cookies_browser() -> String {
    env_or("VIDEO_CLONER_YTDLP_COOKIES", "")
}

/// Gemini API key taken from the environment.
///
/// This is only the fallback: the key is normally set through the app's own
/// Settings page and stored in `app_settings`, so the user does not have to
/// restart the daemon to change it. See `db::gemini_api_key`.
pub fn env_gemini_api_key() -> String {
    for key in [
        "VIDEO_CLONER_GEMINI_API_KEY",
        "GEMINI_API_KEY",
        "GOOGLE_API_KEY",
    ] {
        let v = env_or(key, "");
        if !v.is_empty() {
            return v;
        }
    }
    String::new()
}
