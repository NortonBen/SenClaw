//! Every `env::var()` in the app lives here.

use std::path::PathBuf;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// 4540 is `json`, 4530 `search` — 4550 is the first free slot above them.
pub fn http_port() -> String {
    env_or("PORT", "4550")
}

pub fn app_id() -> String {
    env_or("SENCLAW_SPACE_APP_ID", "rule-engine")
}

/// Daemon UI server. NOT the WS gateway (18789).
pub fn senclaw_base_url() -> String {
    env_or("SENCLAW_BASE_URL", "http://127.0.0.1:18788")
}

/// Deliberately OUTSIDE the install directory: a Space App zip install wipes
/// `<app_dir>` before extracting, which would take the database with it.
pub fn data_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RULE_ENGINE_DATA_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".senclaw")
        .join("space-app-data")
        .join(app_id())
}

pub fn db_path() -> String {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    dir.join("app.sqlite").to_string_lossy().to_string()
}

/// How long a run may sit with nothing in flight but a join still waiting.
pub fn default_join_timeout_ms() -> u64 {
    env_or("RULE_ENGINE_JOIN_TIMEOUT_MS", "60000")
        .parse()
        .unwrap_or(60_000)
}

/// Hard stop for runaway cycles. A run that exceeds this many hops is failed.
pub fn max_hops_per_run() -> u64 {
    env_or("RULE_ENGINE_MAX_HOPS", "10000")
        .parse()
        .unwrap_or(10_000)
}

/// Runs older than this are reaped even if something is still parked.
pub fn run_ttl_secs() -> i64 {
    env_or("RULE_ENGINE_RUN_TTL_SECS", "900")
        .parse()
        .unwrap_or(900)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_or_ignores_blank() {
        std::env::set_var("RULE_ENGINE_TEST_BLANK", "   ");
        assert_eq!(env_or("RULE_ENGINE_TEST_BLANK", "fallback"), "fallback");
        std::env::remove_var("RULE_ENGINE_TEST_BLANK");
    }

    #[test]
    fn data_dir_is_outside_the_install_dir() {
        std::env::remove_var("RULE_ENGINE_DATA_DIR");
        let dir = data_dir();
        assert!(dir.to_string_lossy().contains(".senclaw"));
    }
}
