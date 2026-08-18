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

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::{Context, Result};
use tokio::sync::Mutex;

/// Fixed loopback port. Not auto-picked: the daemon is the only client, a fixed
/// port makes the process trivially identifiable in `lsof`, and a collision is
/// reported by the bind rather than silently routing to someone else.
const PORT: u16 = 18790;

/// The sidecar's file name — and the stem of its release asset, so the CLI
/// downloader and this lookup cannot drift apart.
pub(crate) const MEDIA_BIN: &str = if cfg!(windows) {
    "senclaw-media.exe"
} else {
    "senclaw-media"
};

/// Where `senclaw web` installs a downloaded sidecar.
///
/// Not next to the daemon: a CLI install puts `senclaw` wherever the user's
/// PATH wants it, and that directory is often root-owned (`/usr/local/bin`),
/// so the sidecar cannot always land beside it.
pub(crate) fn cli_install_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".senclaw")
        .join("bin")
}

/// Where the sidecar binary is. Three places, in order:
///
/// 1. `SENCLAW_MEDIA_BIN` — what makes `cargo run` work without copying
///    anything: point it at the debug build under the cargo output directory.
/// 2. Next to the daemon's own executable — the desktop bundle, where CI and
///    `make app-build` drop it into the same directory as `senclaw`.
/// 3. [`cli_install_dir`] — where `senclaw web` downloads it.
///
/// This is the ONLY answer to "where is the sidecar": the CLI downloader calls
/// it rather than re-deriving the paths, because a second lookup disagrees
/// exactly on the edges that matter — an env override pointing at a dev build,
/// or a bundle copy that must win over a stale download.
pub(crate) fn binary_path() -> Option<PathBuf> {
    resolve(
        std::env::var("SENCLAW_MEDIA_BIN").ok().as_deref(),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(PathBuf::from))
            .as_deref(),
        &cli_install_dir(),
    )
}

/// The ordering rule of [`binary_path`], separated from where its three inputs
/// come from so the precedence can be tested without mutating process state.
fn resolve(env_override: Option<&str>, exe_dir: Option<&Path>, cli_dir: &Path) -> Option<PathBuf> {
    if let Some(p) = env_override {
        let p = PathBuf::from(p.trim());
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(dir) = exe_dir {
        let candidate = dir.join(MEDIA_BIN);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let downloaded = cli_dir.join(MEDIA_BIN);
    downloaded.is_file().then_some(downloaded)
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
        "senclaw-media binary not found — run `senclaw update` to download it into \
         ~/.senclaw/bin, or set SENCLAW_MEDIA_BIN to a local build. In a desktop install \
         it ships inside the app bundle, so a missing copy there is a broken build",
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

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, b"binary").unwrap();
        p
    }

    /// The download in `~/.senclaw/bin` must never shadow the copy shipped
    /// beside the daemon: after a desktop update those are different versions,
    /// and the bundle's copy is the one that matches the running daemon.
    #[test]
    fn a_bundled_sidecar_wins_over_a_downloaded_one() {
        let tmp = tempfile::tempdir().unwrap();
        let exe_dir = tmp.path().join("Resources");
        let cli_dir = tmp.path().join("cli-bin");
        let bundled = touch(&exe_dir, MEDIA_BIN);
        touch(&cli_dir, MEDIA_BIN);

        assert_eq!(resolve(None, Some(&exe_dir), &cli_dir), Some(bundled));
    }

    /// The gap this download exists to close: a CLI install has `senclaw` on
    /// PATH with nothing beside it, so only the downloaded copy is found.
    #[test]
    fn a_cli_install_falls_through_to_the_downloaded_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        let exe_dir = tmp.path().join("bin"); // holds `senclaw` only
        std::fs::create_dir_all(&exe_dir).unwrap();
        let cli_dir = tmp.path().join("cli-bin");
        let downloaded = touch(&cli_dir, MEDIA_BIN);

        assert_eq!(resolve(None, Some(&exe_dir), &cli_dir), Some(downloaded));
    }

    /// A dev build named by the env var outranks both, and a stale value
    /// pointing at a deleted file falls through rather than failing outright.
    #[test]
    fn the_env_override_wins_but_a_stale_one_is_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let exe_dir = tmp.path().join("Resources");
        let cli_dir = tmp.path().join("cli-bin");
        let bundled = touch(&exe_dir, MEDIA_BIN);
        let dev = touch(&tmp.path().join("debug"), MEDIA_BIN);

        let via_env = resolve(Some(dev.to_str().unwrap()), Some(&exe_dir), &cli_dir);
        assert_eq!(via_env, Some(dev));

        let stale = tmp.path().join("gone").join(MEDIA_BIN);
        let via_stale = resolve(Some(stale.to_str().unwrap()), Some(&exe_dir), &cli_dir);
        assert_eq!(via_stale, Some(bundled));
    }

    #[test]
    fn nothing_installed_resolves_to_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(resolve(None, Some(tmp.path()), tmp.path()), None);
    }
}
