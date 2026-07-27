//! Environment-derived configuration. Every value the daemon injects is read
//! here and nowhere else.

use std::path::PathBuf;

fn env_or(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

/// App HTTP port — injected by the SenClaw daemon as PORT (manifest: 4500).
pub fn http_port() -> String {
    env_or("PORT", "4500")
}

pub fn app_id() -> String {
    env_or("SENCLAW_SPACE_APP_ID", "kaen")
}

/// Base URL of the SenClaw daemon's UI server, used for the LLM bridge.
pub fn senclaw_base_url() -> String {
    env_or("SENCLAW_BASE_URL", "http://127.0.0.1:18788")
}

/// App data root — deliberately OUTSIDE the install directory.
///
/// Installing a Space App zip does `remove_dir_all(<app_dir>)` before extracting,
/// so anything kept next to the binary (the SQLite DB) is destroyed on every
/// update. Data therefore lives in a stable per-app directory under the user's
/// home; `KAEN_DATA_DIR` overrides it.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("KAEN_DATA_DIR") {
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
