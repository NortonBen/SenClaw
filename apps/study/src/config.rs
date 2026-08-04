//! Environment-derived configuration. Every value the daemon injects is read
//! here and nowhere else.

use std::path::PathBuf;

fn env_or(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

/// App HTTP port — injected by the SenClaw daemon as PORT (manifest: 4720).
pub fn http_port() -> String {
    env_or("PORT", "4720")
}

/// Bind address. Never default to 0.0.0.0 — a Space App on every interface is
/// how this repo previously exposed app APIs to the LAN.
pub fn bind_host() -> String {
    env_or("SENCLAW_BIND_HOST", "127.0.0.1")
}

pub fn app_id() -> String {
    env_or("SENCLAW_SPACE_APP_ID", "study")
}

/// Base URL of the SenClaw daemon's UI server — LLM bridge, calendar REST,
/// TTS and the MCP registry all live behind it.
pub fn senclaw_base_url() -> String {
    env_or("SENCLAW_BASE_URL", "http://127.0.0.1:18788")
}

/// App data root — deliberately OUTSIDE the install directory.
///
/// Installing a Space App zip does `remove_dir_all(<app_dir>)` before
/// extracting, so anything kept next to the binary (the SQLite DB, the TTS
/// cache) is destroyed on every update. Data therefore lives in a stable
/// per-app directory under the user's home; `STUDY_DATA_DIR` overrides it.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("STUDY_DATA_DIR") {
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

/// Where synthesized speech is cached. TTS is slow enough locally that a cache
/// is a correctness requirement for hands-free mode, not an optimisation.
pub fn audio_dir() -> PathBuf {
    data_dir().join("audio")
}
