//! Env config — port of `internal/config`. FLOWKIT_* names are kept so an
//! existing Flow Kit setup (and the unchanged Chrome extension) keep working;
//! paths default under the app dir per Space App convention.

use std::path::PathBuf;

fn env_or(k: &str, d: &str) -> String {
    std::env::var(k)
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| d.to_string())
}

fn env_u64(k: &str, d: u64) -> u64 {
    std::env::var(k)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(d)
}

/// App HTTP port — injected by the SenClaw daemon as PORT (manifest: 4460).
pub fn http_port() -> String {
    env_or("PORT", "4460")
}

/// Extension WebSocket port. The Chrome extension connects to ws://127.0.0.1:9222.
pub fn ws_port() -> u16 {
    env_u64("FLOWKIT_WS_PORT", 9222) as u16
}

pub fn worker_enabled() -> bool {
    // Default ON (the Go side needed FLOWKIT_WORKER=1; as a Space App the worker
    // is the point, so it runs unless explicitly disabled).
    env_or("FLOWKIT_WORKER", "1") == "1"
}

pub fn worker_poll_secs() -> u64 {
    env_u64("FLOWKIT_WORKER_POLL_SEC", 5)
}

pub fn worker_gen_timeout_secs() -> u64 {
    env_u64("FLOWKIT_WORKER_GEN_TIMEOUT_SEC", 300)
}

pub fn video_poll_secs() -> u64 {
    env_u64("FLOWKIT_WORKER_VIDEO_POLL_SEC", 10)
}

pub fn video_poll_timeout_secs() -> u64 {
    env_u64("FLOWKIT_WORKER_VIDEO_POLL_TIMEOUT_SEC", 420)
}

/// Minimum spacing (ms) between Flow video submits, process-wide. Bursts of
/// captcha-consuming video calls are what trip Google's `UNUSUAL_ACTIVITY`
/// anti-bot flag, so submits are spaced out even when scenes render in
/// parallel. 0 disables the throttle.
pub fn video_submit_gap_ms() -> u64 {
    env_u64("FLOWKIT_VIDEO_SUBMIT_GAP_MS", 1500)
}

pub fn google_flow_api() -> String {
    env_or("GOOGLE_FLOW_API", "https://aisandbox-pa.googleapis.com")
}

/// Google Flow API key. Unlike the Go backend there is NO hardcoded fallback —
/// the key must come from env or the extension token capture.
pub fn google_api_key() -> String {
    env_or("GOOGLE_API_KEY", "")
}

pub fn default_orientation() -> String {
    env_or("FLOWKIT_ORIENTATION", "VERTICAL")
}

pub fn exec_allowlist() -> Vec<String> {
    env_or("FLOWKIT_EXEC_ALLOWLIST", "ffmpeg,ffprobe")
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn exec_timeout_secs() -> u64 {
    env_u64("FLOWKIT_EXEC_TIMEOUT_SEC", 300)
}

pub fn http_tools_allow_private() -> bool {
    env_or("FLOWKIT_TOOL_HTTP_ALLOW_PRIVATE", "0") == "1"
}

/// App data root — deliberately OUTSIDE the install directory.
///
/// Installing a Space App zip does `remove_dir_all(<app_dir>)` before
/// extracting, so anything kept next to the binary (the SQLite DB, downloaded
/// media) is destroyed on every update. Data therefore lives in a stable
/// per-app directory under the user's home; `FLOWKIT_DATA_DIR` overrides it.
pub fn data_dir() -> PathBuf {
    if let Ok(d) = std::env::var("FLOWKIT_DATA_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    let app_id = std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "video-flow".to_string());
    match std::env::var("HOME").ok().filter(|h| !h.is_empty()) {
        Some(home) => PathBuf::from(home)
            .join(".senclaw")
            .join("space-app-data")
            .join(app_id),
        // No HOME (unusual): fall back to the cwd so the app still runs.
        None => PathBuf::from("."),
    }
}

/// Move a pre-existing DB + media from the install dir into `data_dir()`.
///
/// Upgrade path for installs that predate the move. Runs once: it only acts
/// when the new location has no DB yet, and never overwrites newer data.
pub fn migrate_legacy_data_dir() {
    let target = data_dir();
    if target == PathBuf::from(".") {
        return;
    }
    let legacy = PathBuf::from(".");
    let legacy_db = legacy.join("app.sqlite");
    let target_db = target.join("app.sqlite");
    if !legacy_db.is_file() || target_db.exists() {
        return;
    }
    if std::fs::create_dir_all(&target).is_err() {
        return;
    }
    // Carry the WAL/SHM sidecars too, or SQLite loses committed transactions.
    let mut moved = Vec::new();
    for name in ["app.sqlite", "app.sqlite-wal", "app.sqlite-shm"] {
        let from = legacy.join(name);
        if from.is_file() && std::fs::rename(&from, target.join(name)).is_ok() {
            moved.push(name);
        }
    }
    let legacy_media = legacy.join("media");
    if legacy_media.is_dir() {
        let target_media = legacy_media_target(&target);
        let _ = std::fs::create_dir_all(&target_media);
        if let Ok(entries) = std::fs::read_dir(&legacy_media) {
            for e in entries.flatten() {
                let dest = target_media.join(e.file_name());
                if !dest.exists() {
                    let _ = std::fs::rename(e.path(), dest);
                }
            }
        }
    }
    if !moved.is_empty() {
        println!(
            "[config] migrated existing data into {} ({}) — survives future installs",
            target.display(),
            moved.join(", ")
        );
    }
}

fn legacy_media_target(data: &PathBuf) -> PathBuf {
    match std::env::var("FLOWKIT_MEDIA_DIR") {
        Ok(d) if !d.trim().is_empty() => PathBuf::from(d),
        _ => data.join("media"),
    }
}

pub fn db_path() -> String {
    env_or(
        "FLOWKIT_DB_PATH",
        data_dir().join("app.sqlite").to_string_lossy().as_ref(),
    )
}

pub fn media_dir() -> PathBuf {
    PathBuf::from(env_or(
        "FLOWKIT_MEDIA_DIR",
        data_dir().join("media").to_string_lossy().as_ref(),
    ))
}

/// Souls (sub-agent system prompts). Checked in candidate order so both the dev
/// layout (cargo run from repo root) and the packaged layout work.
pub fn souls_dir() -> PathBuf {
    if let Ok(d) = std::env::var("FLOWKIT_SOULS_DIR") {
        return PathBuf::from(d);
    }
    first_existing(&["apps/video-flow/souls", "souls"], "souls")
}

/// Flow playbooks (the Go backend's `skills/*.md` — internal skill-agent
/// prompt playbooks, NOT SenClaw skills; those live in `skills/`).
pub fn playbooks_dir() -> PathBuf {
    if let Ok(d) = std::env::var("FLOWKIT_SKILLS_DIR") {
        return PathBuf::from(d);
    }
    first_existing(&["apps/video-flow/playbooks", "playbooks"], "playbooks")
}

fn first_existing(cands: &[&str], fallback: &str) -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    for c in cands {
        let p = PathBuf::from(c);
        if p.is_dir() {
            return p;
        }
        let p = exe_dir.join(c);
        if p.is_dir() {
            return p;
        }
    }
    PathBuf::from(fallback)
}

/// Minimum number of per-scene slots a generated workflow provisions.
///
/// `script_parser` runs inside the workflow, so the scene count at launch is
/// only a lower bound. Slots without a scene behind them skip cleanly, so the
/// floor costs nothing but avoids under-rendering a freshly parsed script.
///
/// Anything past the last slot is still rendered by the `catchup` node, but
/// serially — so the floor should cover a typical script to keep the fan-out
/// doing the work.
pub fn scene_slots_min() -> usize {
    env_u64("FLOWKIT_SCENE_SLOTS_MIN", 20) as usize
}

/// How many LLM calls a per-scene stage may have in flight.
///
/// The pipeline's per-scene stages (screenplay, block parsing, continuity
/// bridges) are independent, so they run concurrently rather than one after
/// another. Kept modest because providers throttle — and the daemon bridge is
/// shared with everything else on this machine.
pub fn llm_concurrency() -> usize {
    env_u64("FLOWKIT_LLM_CONCURRENCY", 4).max(1) as usize
}

/// Concurrency for pure I/O fan-out (downloading rendered assets).
pub fn io_concurrency() -> usize {
    env_u64("FLOWKIT_IO_CONCURRENCY", 8).max(1) as usize
}
