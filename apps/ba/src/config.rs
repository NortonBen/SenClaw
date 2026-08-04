//! Runtime configuration — mirrors apps/study: every knob is an env var read
//! on demand, nothing cached, so tests can override freely.

use std::path::PathBuf;

pub fn app_id() -> String {
    std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "ba".to_string())
}

/// Loopback by default. A Space App authenticates nothing of its own — the
/// daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
/// a wildcard hands the whole REST + MCP surface to anyone on the LAN. Set
/// SENCLAW_BIND_HOST explicitly to opt out of loopback.
pub fn bind_host() -> String {
    std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string())
}

pub fn http_port() -> String {
    std::env::var("PORT").unwrap_or_else(|_| "4740".to_string())
}

pub fn senclaw_base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}

/// App data root — deliberately OUTSIDE the install directory. Installing a
/// Space App zip does `remove_dir_all(<app_dir>)` before extracting, so
/// anything kept next to the binary (the SQLite DB, exports) is destroyed on
/// every update. Data therefore lives in a stable per-app directory under the
/// user's home; `BA_DATA_DIR` overrides it (tests point it at a tempdir).
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("BA_DATA_DIR") {
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

pub fn db_path() -> PathBuf {
    data_dir().join("app.sqlite")
}

pub fn exports_dir() -> PathBuf {
    data_dir().join("exports")
}
