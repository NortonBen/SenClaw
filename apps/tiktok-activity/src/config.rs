//! Every environment-variable read in the app, in one place.
//! Mirrors the SenClaw App Space convention (see apps/zeach/src/config.rs).

use std::path::PathBuf;

fn env_or(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

/// App HTTP port — injected by the SenClaw daemon as PORT (manifest: 4580).
pub fn http_port() -> String {
    env_or("PORT", "4580")
}

pub fn app_id() -> String {
    env_or("SENCLAW_SPACE_APP_ID", "tiktok-activity")
}

/// Base URL of the SenClaw daemon's UI server — REST + the app bridge.
pub fn senclaw_base_url() -> String {
    env_or("SENCLAW_BASE_URL", "http://127.0.0.1:18788")
}

/// Agent identity stamped on browser commands (kept for parity with peers).
pub fn browser_agent_id() -> String {
    env_or("SENCLAW_AGENT_ID", "space-app-tiktok-activity")
}

/// App data root — deliberately OUTSIDE the install dir, because a Space App
/// zip install wipes `<app_dir>` before extracting.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("TIKTOK_DATA_DIR") {
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
    if let Ok(p) = std::env::var("TIKTOK_SQLITE_PATH") {
        if !p.trim().is_empty() {
            return p;
        }
    }
    data_dir().join("app.db").to_string_lossy().to_string()
}

/// Directory holding per-account persistent Chromium profiles when a
/// BrowserProfile does not pin its own `userDataDir`.
pub fn profiles_dir() -> PathBuf {
    data_dir().join("profiles")
}

/// Control mode: how flow actions reach a browser.
///   `extension` (default) — drive ONE logged-in TikTok tab through the browser
///                           extension over the ext-WS bridge.
///   `stub`                — no browser (simulate; for tests/flow authoring).
/// Set with `TIKTOK_CONTROL_MODE`.
pub fn control_mode() -> String {
    let m = env_or("TIKTOK_CONTROL_MODE", "extension").to_lowercase();
    match m.as_str() {
        "stub" | "extension" => m,
        _ => "extension".into(),
    }
}

/// Dedicated WS port the TikTok browser extension dials (own port per app —
/// youtube 9223, social 9224; tiktok 9225).
pub fn ext_ws_port() -> u16 {
    env_or("TIKTOK_EXT_WS_PORT", "9225").parse().unwrap_or(9225)
}
