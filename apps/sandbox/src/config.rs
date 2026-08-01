//! Environment-derived configuration. Every value the daemon injects is read
//! here and nowhere else.

use std::path::PathBuf;

fn env_or(key: &str, default: &str) -> String {
    match std::env::var(key) {
        Ok(v) if !v.trim().is_empty() => v,
        _ => default.to_string(),
    }
}

/// App HTTP port — injected by the SenClaw daemon as PORT (manifest: 4730).
pub fn http_port() -> String {
    env_or("PORT", "4730")
}

/// Bind address. Never default to 0.0.0.0 — a Space App on every interface is
/// how this repo previously exposed app APIs to the LAN. This app runs
/// arbitrary code on request, so a LAN-reachable port here is a remote shell.
pub fn bind_host() -> String {
    env_or("SENCLAW_BIND_HOST", "127.0.0.1")
}

pub fn app_id() -> String {
    env_or("SENCLAW_SPACE_APP_ID", "sandbox")
}

/// App data root — deliberately OUTSIDE the install directory.
///
/// Installing a Space App zip does `remove_dir_all(<app_dir>)` before
/// extracting, so anything kept next to the binary is destroyed on every
/// update. Sandbox workdirs are user data; they live under the user's home.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("SANDBOX_DATA_DIR") {
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

/// Root holding one directory per sandbox. This is the ONLY host path a
/// sandbox may write to, and each sandbox is confined to its own subdirectory.
pub fn workspaces_dir() -> PathBuf {
    data_dir().join("workspaces")
}

/// Docker binary. Overridable because Docker Desktop, Colima, OrbStack and
/// Podman-in-docker-mode all ship the CLI at different paths.
pub fn docker_bin() -> String {
    env_or("SANDBOX_DOCKER_BIN", "docker")
}

/// Default image for docker-backed sandboxes.
pub fn default_image() -> String {
    env_or("SANDBOX_DEFAULT_IMAGE", "python:3.12-slim")
}
