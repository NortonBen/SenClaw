//! Composite download of the draw.io editor webapp.
//!
//! The editor (`draw.war`, ~53MB) is far bigger than the Space-App install-zip
//! limits (50MB local / 20MB hub), so the app zip ships without it and this
//! module downloads a pinned release on first run, verifies its sha256, unpacks
//! it (a .war is a zip) into `~/.senclaw/space-apps/drawio/editor/webapp/` and
//! the daemon serves it same-origin at `/drawio/`. After that first download the
//! app is fully offline — the iframe runs with `stealth=1` so the editor makes
//! no external calls.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::api::AppState;

pub const DRAWIO_VERSION: &str = "31.1.2";
const WAR_URL: &str = "https://github.com/jgraph/drawio/releases/download/v31.1.2/draw.war";
/// sha256 of the pinned draw.war. Overridable via SENCLAW_DRAWIO_WAR_SHA256;
/// an empty value skips verification (logged loudly).
const WAR_SHA256: &str = "05907c7d4f987673de5222350d32e64bf1a16defbf5259be3a28d156466f85c3";

#[derive(Clone)]
pub enum EditorStatus {
    Missing,
    Downloading { received: u64, total: u64 },
    Extracting,
    Ready { version: String },
    Error { message: String },
}

pub struct Editor {
    pub status: Mutex<EditorStatus>,
    running: AtomicBool,
}

impl Editor {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(EditorStatus::Missing),
            running: AtomicBool::new(false),
        }
    }

    fn set(&self, s: EditorStatus) {
        *self.status.lock().unwrap() = s;
    }

    pub fn status_json(&self) -> Value {
        match &*self.status.lock().unwrap() {
            EditorStatus::Missing => json!({ "status": "missing" }),
            EditorStatus::Downloading { received, total } => json!({
                "status": "downloading",
                "received": received,
                "total": total,
                "percent": if *total > 0 { (*received as f64 / *total as f64 * 100.0).round() } else { 0.0 },
            }),
            EditorStatus::Extracting => json!({ "status": "extracting" }),
            EditorStatus::Ready { version } => json!({ "status": "ready", "version": version }),
            EditorStatus::Error { message } => json!({ "status": "error", "message": message }),
        }
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

fn editor_root() -> PathBuf {
    crate::db::default_data_dir("drawio").join("editor")
}

/// Directory served at `/drawio/`. `SENCLAW_DRAWIO_EDITOR_DIR` points at an
/// already-unpacked webapp (dev / air-gapped installs) and wins outright.
pub fn webapp_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("SENCLAW_DRAWIO_EDITOR_DIR") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir);
        }
    }
    editor_root().join("webapp")
}

fn war_url() -> String {
    std::env::var("SENCLAW_DRAWIO_WAR_URL")
        .ok()
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| WAR_URL.to_string())
}

fn war_sha256() -> String {
    std::env::var("SENCLAW_DRAWIO_WAR_SHA256")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| WAR_SHA256.to_string())
        .to_lowercase()
}

/// Kick off the ensure flow in the background (idempotent — a second call while
/// one is already running is a no-op). Also used by POST /api/editor/retry.
pub fn spawn_ensure(state: Arc<AppState>) {
    if state.editor.running.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        let result = ensure(&state).await;
        if let Err(e) = result {
            eprintln!("[drawio] editor setup failed: {e:#}");
            state.editor.set(EditorStatus::Error {
                message: format!("{e:#}"),
            });
        }
        state.editor.running.store(false, Ordering::SeqCst);
    });
}

async fn ensure(state: &Arc<AppState>) -> anyhow::Result<()> {
    let webapp = webapp_dir();

    // Already usable? An env-override dir is trusted as-is; the managed dir
    // must also match the pinned version so upgrades re-download.
    if webapp.join("index.html").exists() {
        let overridden = std::env::var("SENCLAW_DRAWIO_EDITOR_DIR")
            .map(|d| !d.trim().is_empty())
            .unwrap_or(false);
        let version_ok = std::fs::read_to_string(editor_root().join("VERSION"))
            .map(|v| v.trim() == DRAWIO_VERSION)
            .unwrap_or(false);
        if overridden || version_ok {
            state.editor.set(EditorStatus::Ready {
                version: if overridden {
                    "custom".into()
                } else {
                    DRAWIO_VERSION.into()
                },
            });
            return Ok(());
        }
    }

    let root = editor_root();
    std::fs::create_dir_all(&root)?;
    let war_path = root.join("draw.war.part");

    // ---- Download (streaming, hashing on the fly) ----
    let url = war_url();
    println!("[drawio] downloading editor {DRAWIO_VERSION} from {url}");
    state.editor.set(EditorStatus::Downloading {
        received: 0,
        total: 0,
    });

    let resp = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(1800))
        .send()
        .await?
        .error_for_status()?;
    let total = resp.content_length().unwrap_or(0);

    let mut hasher = Sha256::new();
    let mut received: u64 = 0;
    {
        use futures_util::StreamExt;
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::File::create(&war_path).await?;
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
            received += chunk.len() as u64;
            state
                .editor
                .set(EditorStatus::Downloading { received, total });
        }
        file.flush().await?;
    }

    let digest = format!("{:x}", hasher.finalize());
    println!("[drawio] draw.war sha256 = {digest}"); // pin this on version bumps
    let expected = war_sha256();
    if expected.is_empty() {
        eprintln!("[drawio] WARNING: no pinned sha256 — skipping checksum verification");
    } else if digest != expected {
        let _ = std::fs::remove_file(&war_path);
        anyhow::bail!("draw.war checksum mismatch: got {digest}, expected {expected}");
    }

    // ---- Extract (war = zip; skip servlet metadata; guard against zip-slip) ----
    state.editor.set(EditorStatus::Extracting);
    let webapp_out = editor_root().join("webapp");
    let war_for_task = war_path.clone();
    let out_for_task = webapp_out.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let file = std::fs::File::open(&war_for_task)?;
        let mut archive = zip::ZipArchive::new(file)?;
        if out_for_task.exists() {
            std::fs::remove_dir_all(&out_for_task)?;
        }
        std::fs::create_dir_all(&out_for_task)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let Some(rel) = entry.enclosed_name() else {
                continue;
            }; // zip-slip guard
            let name = rel.to_string_lossy();
            if name.starts_with("META-INF/") || name.starts_with("WEB-INF/") {
                continue;
            }
            let dest = out_for_task.join(&rel);
            if entry.is_dir() {
                std::fs::create_dir_all(&dest)?;
            } else {
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut out = std::fs::File::create(&dest)?;
                std::io::copy(&mut entry, &mut out)?;
            }
        }
        Ok(())
    })
    .await??;

    std::fs::write(editor_root().join("VERSION"), DRAWIO_VERSION)?;
    let _ = std::fs::remove_file(&war_path);

    if !webapp_out.join("index.html").exists() {
        anyhow::bail!("extracted editor is missing index.html — unexpected draw.war layout");
    }

    println!(
        "[drawio] editor {DRAWIO_VERSION} ready at {}",
        webapp_out.display()
    );
    state.editor.set(EditorStatus::Ready {
        version: DRAWIO_VERSION.into(),
    });
    Ok(())
}
