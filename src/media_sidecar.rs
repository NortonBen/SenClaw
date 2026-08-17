//! Spawning and reaching `senclaw-media`, the MLX speech sidecar.
//!
//! The daemon does not link MLX any more. Speech-to-text lives in a separate
//! binary that ships **beside** this one, and this module is the whole contract
//! between them: find it, start it once, keep it, and hand back a base URL.
//!
//! ## Why not a Space App
//!
//! A Space App is installed and can be uninstalled, and the daemon has to treat
//! its absence as normal. Speech-to-text is not optional in that sense — it
//! backs voice chat and the transcribe endpoint — so a missing binary is a
//! broken build, not an uninstalled feature. It is spawned the way the daemon
//! already spawns its own `*-server` subcommands, and it needs no manifest, no
//! install step and no entry in the app list.
//!
//! ## Lifetime
//!
//! Started on the first transcription and kept for the life of the daemon. It
//! is not reaped when idle: the sidecar itself drops the Whisper weights after
//! use, so an idle process costs a few megabytes, and respawning would trade
//! that for a cold start on the next sentence of a voice conversation.

use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use tokio::sync::Mutex;

/// Fixed loopback port. Not auto-picked: the daemon is the only client, a fixed
/// port makes the process trivially identifiable in `lsof`, and a collision is
/// reported by the bind rather than silently routing to someone else.
const PORT: u16 = 18790;

/// Where the sidecar binary is, next to the daemon's own executable.
///
/// `SENCLAW_MEDIA_BIN` overrides it, which is what makes `cargo run` work
/// without copying anything: point it at `target/debug/senclaw-media`.
fn binary_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("SENCLAW_MEDIA_BIN") {
        let p = PathBuf::from(p.trim());
        if p.is_file() {
            return Some(p);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) {
        "senclaw-media.exe"
    } else {
        "senclaw-media"
    };
    let candidate = dir.join(name);
    candidate.is_file().then_some(candidate)
}

/// The running child, if we started one.
static CHILD: OnceLock<Mutex<Option<tokio::process::Child>>> = OnceLock::new();

fn child_slot() -> &'static Mutex<Option<tokio::process::Child>> {
    CHILD.get_or_init(|| Mutex::new(None))
}

/// Base URL of a running sidecar, starting it if necessary.
pub async fn ensure_running() -> Result<String> {
    let base = format!("http://127.0.0.1:{PORT}");

    // Serialize: two concurrent transcriptions must not both spawn.
    let mut slot = child_slot().lock().await;

    // Already ours and still alive?
    if let Some(child) = slot.as_mut() {
        if matches!(child.try_wait(), Ok(None)) {
            return Ok(base);
        }
        // It died; fall through and respawn.
        *slot = None;
    }

    // Someone else's — a previous daemon run, or a developer running it by
    // hand. Adopt it rather than fighting for the port.
    if health_ok(&base).await {
        return Ok(base);
    }

    let bin = binary_path().context(
        "senclaw-media binary not found next to the daemon — the build did not bundle it \
         (set SENCLAW_MEDIA_BIN to run it from target/)",
    )?;

    let child = tokio::process::Command::new(&bin)
        .env("PORT", PORT.to_string())
        // Never inherit a wildcard bind from the daemon's own network setting:
        // this sidecar authenticates nothing and takes file paths in its
        // requests.
        .env("SENCLAW_BIND_HOST", "127.0.0.1")
        .stdin(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("spawning {}", bin.display()))?;
    *slot = Some(child);

    // Wait for the port, not for the model: the sidecar answers /health before
    // it reads any weights, on purpose.
    for _ in 0..60 {
        if health_ok(&base).await {
            tracing::info!("[media] senclaw-media up on {base}");
            return Ok(base);
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    anyhow::bail!("senclaw-media did not answer /health within 15s")
}

async fn health_ok(base: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
    else {
        return false;
    };
    client
        .get(format!("{base}/health"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Stop the sidecar, if this daemon started it. Called on shutdown so a restart
/// does not leave an orphan holding the port.
pub async fn shutdown() {
    let mut slot = child_slot().lock().await;
    if let Some(mut child) = slot.take() {
        let _ = child.kill().await;
    }
}
