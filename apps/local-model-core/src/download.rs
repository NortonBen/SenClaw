//! Pulling a checkpoint from HuggingFace, with progress and resume.
//!
//! Ported from the daemon's `local_models.rs`. The behaviour that matters is not
//! the HTTP: it is that a 14 GB download survives being interrupted, reports
//! enough for a progress bar to be honest, and can be cancelled without leaving
//! a half-written shard that later looks like a complete model.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Result;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

const HF_BASE: &str = "https://huggingface.co";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Listing,
    Downloading,
    Done,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadState {
    pub model_id: String,
    pub status: DownloadStatus,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub files_total: u32,
    pub files_done: u32,
    pub current_file: Option<String>,
    pub error: Option<String>,
}

impl DownloadState {
    fn new(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            status: DownloadStatus::Queued,
            total_bytes: 0,
            downloaded_bytes: 0,
            files_total: 0,
            files_done: 0,
            current_file: None,
            error: None,
        }
    }

    pub fn is_finished(&self) -> bool {
        matches!(
            self.status,
            DownloadStatus::Done | DownloadStatus::Error | DownloadStatus::Cancelled
        )
    }
}

#[derive(Clone)]
struct Handle {
    state: Arc<Mutex<DownloadState>>,
    cancel: CancellationToken,
}

fn registry() -> &'static Mutex<HashMap<String, Handle>> {
    static R: OnceLock<Mutex<HashMap<String, Handle>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Start a download, or return the state of one already running for this model.
///
/// Idempotent on purpose: the UI polls, and a double-click on Download must not
/// start a second writer against the same files.
pub fn start(model_id: &str, revision: &str, dir: PathBuf) -> DownloadState {
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(h) = reg.get(model_id) {
        let s = h.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if !s.is_finished() {
            return s;
        }
    }

    let state = Arc::new(Mutex::new(DownloadState::new(model_id)));
    let cancel = CancellationToken::new();
    reg.insert(
        model_id.to_string(),
        Handle {
            state: Arc::clone(&state),
            cancel: cancel.clone(),
        },
    );
    drop(reg);

    let (id, rev) = (model_id.to_string(), revision.to_string());
    let st = Arc::clone(&state);
    tokio::spawn(async move {
        let result = run(&id, &rev, &dir, Arc::clone(&st), cancel).await;
        let mut s = st.lock().unwrap_or_else(|e| e.into_inner());
        // A cancel already set its own terminal status; do not overwrite it with
        // Done, which would make a cancelled download look successful.
        if s.status != DownloadStatus::Cancelled {
            match result {
                Ok(()) => s.status = DownloadStatus::Done,
                Err(e) => {
                    s.status = DownloadStatus::Error;
                    s.error = Some(e.to_string());
                }
            }
        }
    });

    let snapshot = state.lock().unwrap_or_else(|e| e.into_inner()).clone();
    snapshot
}

/// Progress for one model, if it has ever been downloaded this process.
pub fn status(model_id: &str) -> Option<DownloadState> {
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.get(model_id)
        .map(|h| h.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
}

/// Every download this process knows about.
pub fn all() -> Vec<DownloadState> {
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    reg.values()
        .map(|h| h.state.lock().unwrap_or_else(|e| e.into_inner()).clone())
        .collect()
}

/// Ask a running download to stop. Returns false when there was nothing to stop.
pub fn cancel(model_id: &str) -> bool {
    let reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    match reg.get(model_id) {
        Some(h) => {
            h.cancel.cancel();
            true
        }
        None => false,
    }
}

/// Files that are not weights and not needed to run the model.
///
/// `.gitattributes` and the like are noise, but the expensive one is the
/// PyTorch/ONNX duplicate of a checkpoint that also ships safetensors — several
/// gigabytes of a format neither engine here reads.
fn should_skip(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let base = lower.rsplit('/').next().unwrap_or(&lower);
    if base.starts_with('.') {
        return true;
    }
    if lower.starts_with("onnx/") || lower.starts_with("coreml/") || lower.starts_with("openvino/") {
        return true;
    }
    matches!(
        base,
        "pytorch_model.bin"
            | "tf_model.h5"
            | "model.ckpt.index"
            | "flax_model.msgpack"
            | "readme.md"
            | "license"
            | "license.txt"
    ) || base.ends_with(".onnx")
        || base.ends_with(".pth")
        || base.ends_with(".png")
        || base.ends_with(".jpg")
        || base.ends_with(".gif")
}

#[derive(Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    path: String,
    #[serde(default)]
    size: u64,
}

async fn run(
    model_id: &str,
    revision: &str,
    dir: &Path,
    progress: Arc<Mutex<DownloadState>>,
    cancel: CancellationToken,
) -> Result<()> {
    // No total timeout: a single shard can be gigabytes over a slow link, and a
    // deadline here would kill a download that is working. A stall is caught by
    // the read timeout instead, which resets on every byte.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .read_timeout(std::time::Duration::from_secs(120))
        .build()?;

    set(&progress, |s| s.status = DownloadStatus::Listing);

    let tree_url = format!("{HF_BASE}/api/models/{model_id}/tree/{revision}?recursive=true");
    let tree: Vec<TreeEntry> = client
        .get(&tree_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let files: Vec<TreeEntry> = tree
        .into_iter()
        .filter(|e| e.entry_type == "file" && !should_skip(&e.path))
        .collect();
    if files.is_empty() {
        anyhow::bail!("`{model_id}` has no downloadable files at revision `{revision}`");
    }

    let total: u64 = files.iter().map(|f| f.size).sum();
    set(&progress, |s| {
        s.files_total = files.len() as u32;
        s.total_bytes = total;
        s.status = DownloadStatus::Downloading;
    });

    for entry in files {
        if cancel.is_cancelled() {
            set(&progress, |s| s.status = DownloadStatus::Cancelled);
            return Ok(());
        }
        set(&progress, |s| s.current_file = Some(entry.path.clone()));

        let dst = dir.join(&entry.path);
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Resume: a shard already at its published size is complete. This is
        // what makes an interrupted 14 GB download cost only the shard it died
        // on rather than all of it.
        if let Ok(meta) = tokio::fs::metadata(&dst).await {
            if entry.size > 0 && meta.len() == entry.size {
                set(&progress, |s| {
                    s.files_done += 1;
                    s.downloaded_bytes = s.downloaded_bytes.saturating_add(entry.size);
                });
                continue;
            }
        }

        let url = format!("{HF_BASE}/{model_id}/resolve/{revision}/{}", entry.path);
        let resp = client.get(&url).send().await?.error_for_status()?;
        let mut stream = resp.bytes_stream();

        // Write to a `.part` and rename on completion. A shard cut off midway
        // is otherwise left at the real name, where the resume check above sees
        // the wrong size and re-fetches it — but `is_installed` sees a weight
        // file and calls the model ready.
        let part = dst.with_extension("part");
        let mut file = tokio::fs::File::create(&part).await?;
        while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                drop(file);
                let _ = tokio::fs::remove_file(&part).await;
                set(&progress, |s| s.status = DownloadStatus::Cancelled);
                return Ok(());
            }
            let bytes = chunk?;
            file.write_all(&bytes).await?;
            let n = bytes.len() as u64;
            set(&progress, |s| {
                s.downloaded_bytes = s.downloaded_bytes.saturating_add(n)
            });
        }
        file.flush().await?;
        drop(file);
        tokio::fs::rename(&part, &dst).await?;
        set(&progress, |s| s.files_done += 1);
    }

    set(&progress, |s| s.current_file = None);
    Ok(())
}

fn set(state: &Arc<Mutex<DownloadState>>, f: impl FnOnce(&mut DownloadState)) {
    f(&mut state.lock().unwrap_or_else(|e| e.into_inner()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_weight_formats_are_not_downloaded() {
        // The expensive ones: a repo shipping both safetensors and PyTorch is
        // twice the bytes for no gain, and neither engine here reads ONNX.
        for skip in [
            "pytorch_model.bin",
            "model.onnx",
            "onnx/model.onnx",
            "coreml/weights.bin",
            "weights.pth",
            ".gitattributes",
            "README.md",
            "preview.png",
        ] {
            assert!(should_skip(skip), "`{skip}` should be skipped");
        }
    }

    #[test]
    fn everything_needed_to_run_a_model_is_kept() {
        for keep in [
            "config.json",
            "model.safetensors",
            "model-00001-of-00004.safetensors",
            "model.safetensors.index.json",
            "tokenizer.json",
            "tokenizer_config.json",
            "chat_template.jinja",
            "generation_config.json",
        ] {
            assert!(!should_skip(keep), "`{keep}` must be downloaded");
        }
    }

    #[test]
    fn a_finished_download_is_recognised_as_terminal() {
        let mut s = DownloadState::new("a/b");
        assert!(!s.is_finished());
        for st in [
            DownloadStatus::Done,
            DownloadStatus::Error,
            DownloadStatus::Cancelled,
        ] {
            s.status = st;
            assert!(s.is_finished());
        }
        s.status = DownloadStatus::Downloading;
        assert!(!s.is_finished());
    }
}
