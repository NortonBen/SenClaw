//! Every environment-variable read in the app, in one place.

use std::path::PathBuf;

fn env_or(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

/// App HTTP port — injected by the SenClaw daemon as PORT (manifest: 4530).
/// 4520 is `social`, 4500 `kaen`; 4530 is the first free slot above them.
pub fn http_port() -> String {
    env_or("PORT", "4570")
}

pub fn app_id() -> String {
    env_or("SENCLAW_SPACE_APP_ID", "zeach")
}

/// Base URL of the SenClaw daemon's UI server — REST + the app bridge.
pub fn senclaw_base_url() -> String {
    env_or("SENCLAW_BASE_URL", "http://127.0.0.1:18788")
}

/// The daemon's WebSocket gateway port. `/browser-mcp` on this port is the
/// bridge to the Chrome extension — the same one `senclaw-browser`'s own MCP
/// server dials (`src/mcp/browser_server.rs:525`). NOT the UI port (18788).
pub fn senclaw_ws_port() -> u16 {
    env_or("SENCLAW_WS_PORT", "18789").parse().unwrap_or(18789)
}

pub fn browser_ws_url() -> String {
    format!("ws://127.0.0.1:{}/browser-mcp", senclaw_ws_port())
}

/// Agent identity stamped on browser commands so the extension routes them to
/// this app's own tab instead of the shared default-agent tab.
/// See [[browser-multiagent-concurrency]].
pub fn browser_agent_id() -> String {
    env_or("SENCLAW_AGENT_ID", "space-app-zeach")
}

/// App data root — deliberately OUTSIDE the install directory, because a Space
/// App zip install wipes `<app_dir>` before extracting.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("ZEACH_DATA_DIR") {
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

/// Max sources × sub-queries running concurrently in one fan-out.
pub fn fanout_concurrency() -> usize {
    env_or("ZEACH_FANOUT_CONCURRENCY", "8").parse().unwrap_or(8)
}

/// Default per-source timeout for one `search()` call.
pub fn source_timeout_ms() -> u64 {
    env_or("ZEACH_SOURCE_TIMEOUT_MS", "20000")
        .parse()
        .unwrap_or(20_000)
}
