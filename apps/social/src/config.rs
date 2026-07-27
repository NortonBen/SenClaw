//! Every environment-variable read in the app, in one place.

use std::path::PathBuf;

fn env_or(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

/// App HTTP port — injected by the SenClaw daemon as PORT (manifest: 4520).
/// (4490 is taken by hub/shopee, 4491 by the standalone youtube app.)
pub fn http_port() -> String {
    env_or("PORT", "4520")
}

/// Dedicated WebSocket port the shared Chrome extension dials.
///
/// This is the app's OWN bridge port (NOT Chrome's CDP :9222). video-flow uses
/// 9222 and the standalone youtube app uses 9223, so this app uses 9224.
/// Override with `SOCIAL_EXT_WS_PORT`.
pub fn ext_ws_port() -> u16 {
    env_or("SOCIAL_EXT_WS_PORT", "9224").parse().unwrap_or(9224)
}

pub fn app_id() -> String {
    env_or("SENCLAW_SPACE_APP_ID", "social")
}

/// App data root — deliberately OUTSIDE the install directory, because a Space
/// App zip install wipes `<app_dir>` before extracting. Override with
/// `SOCIAL_DATA_DIR`.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("SOCIAL_DATA_DIR") {
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

/// Base URL of the SenClaw daemon's UI server (for llm.request / REST).
#[allow(dead_code)] // consumed once LLM-assisted drafting is wired
pub fn senclaw_base_url() -> String {
    env_or("SENCLAW_BASE_URL", "http://127.0.0.1:18788")
}
