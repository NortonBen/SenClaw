//! Embedding model downloader + feature introspection.
//!
//! Endpoints:
//!   * `GET  /api/embedding/features`     — which embedding backends are available
//!   * `POST /api/embedding/download-model { model }` — **streaming progress**
//!     download via Server-Sent Events (text/event-stream).
//!
//! The streaming endpoint emits JSON events the UI can render as a live
//! progress bar. We don't reuse the existing `local_candle::files_from_hub`
//! because that one swallows progress — instead we do a direct reqwest
//! stream with per-chunk progress callbacks, writing to the same on-disk
//! cache layout (`~/.senclaw/models/<repo-id-slugified>/<filename>`) so
//! subsequent `LocalProvider::from_files` reads work unchanged.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use super::core::{AppError, UiState};

// =====================================================================
// GET /api/embedding/features
// =====================================================================

pub(crate) async fn embedding_features(State(_s): State<Arc<UiState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "candle": cfg!(feature = "local-embed"),
        "candle_metal": cfg!(feature = "local-embed-metal"),
        "mlx_static": cfg!(feature = "cognitive-mlx-embed"),
        "models_dir": dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".senclaw")
            .join("models")
            .to_string_lossy()
            .to_string(),
    }))
}

// =====================================================================
// GET /api/embedding/models — curated list with downloaded status
// =====================================================================
//
// The curated catalog (name, repo, dims, approx size). Source of truth
// for what the Settings UI lets users pick. Kept here — not in
// `embedding_providers.rs` — so the listing endpoint compiles in the
// default (no-feature) build too: we only need fs::exists checks.

const MODEL_CATALOG: &[(&str, &str, u32, &str)] = &[
    // (model_id_user-facing, repo_id_on_hf, dimensions, label_size_hint)
    (
        "all-MiniLM-L6-v2",
        "sentence-transformers/all-MiniLM-L6-v2",
        384,
        "~90MB",
    ),
    (
        "all-MiniLM-L12-v2",
        "sentence-transformers/all-MiniLM-L12-v2",
        384,
        "~120MB",
    ),
    (
        "paraphrase-multilingual-MiniLM-L12-v2",
        "sentence-transformers/paraphrase-multilingual-MiniLM-L12-v2",
        384,
        "~420MB",
    ),
    (
        "multilingual-e5-small",
        "intfloat/multilingual-e5-small",
        384,
        "~470MB",
    ),
    (
        "multilingual-e5-base",
        "intfloat/multilingual-e5-base",
        768,
        "~1.1GB",
    ),
    ("bge-small-en-v1.5", "BAAI/bge-small-en-v1.5", 384, "~130MB"),
    ("bge-base-en-v1.5", "BAAI/bge-base-en-v1.5", 768, "~440MB"),
    (
        "bge-large-en-v1.5",
        "BAAI/bge-large-en-v1.5",
        1024,
        "~1.3GB",
    ),
];

#[derive(Debug, Serialize)]
pub struct ModelEntry {
    pub id: String,
    pub repo: String,
    pub dimensions: u32,
    pub size_hint: String,
    /// True when both tokenizer.json AND a weights file are present on disk.
    pub installed: bool,
    pub on_disk_path: String,
}

/// Filesystem check: do we have what the candle loader needs?
fn is_model_cached(dir: &Path) -> bool {
    if !dir.exists() {
        return false;
    }
    let has_tok = dir.join("tokenizer.json").exists();
    let has_weights =
        dir.join("model.safetensors").exists() || dir.join("pytorch_model.safetensors").exists();
    has_tok && has_weights
}

pub(crate) async fn embedding_list_models(
    State(_s): State<Arc<UiState>>,
) -> Json<serde_json::Value> {
    let cache_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".senclaw")
        .join("models");

    let entries: Vec<ModelEntry> = MODEL_CATALOG
        .iter()
        .map(|(id, repo, dims, size)| {
            // Slug must match `fetch_hf_file`'s on-disk layout: `repo.replace('/', "--")`.
            let dir = cache_dir.join(repo.replace('/', "--"));
            ModelEntry {
                id: (*id).to_string(),
                repo: (*repo).to_string(),
                dimensions: *dims,
                size_hint: (*size).to_string(),
                installed: is_model_cached(&dir),
                on_disk_path: dir.to_string_lossy().into_owned(),
            }
        })
        .collect();

    Json(serde_json::json!({ "models": entries }))
}

// =====================================================================
// POST /api/embedding/download-model — SSE streaming
// =====================================================================
//
// Event shapes (one JSON per `event:` field):
//   start       { repo: string, files: [string] }
//   file_start  { file: string, total: number|null }
//   progress    { file: string, downloaded: number, total: number|null }
//   file_done   { file: string }
//   done        { dir: string }
//   error       { message: string }

#[derive(Debug, Deserialize)]
pub(crate) struct DownloadBody {
    pub model: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "phase")]
#[serde(rename_all = "snake_case")]
enum DlEvent {
    Start {
        repo: String,
        files: Vec<String>,
    },
    FileStart {
        file: String,
        total: Option<u64>,
    },
    Progress {
        file: String,
        downloaded: u64,
        total: Option<u64>,
    },
    FileDone {
        file: String,
    },
    Done {
        dir: String,
    },
    Error {
        message: String,
    },
}

pub(crate) async fn embedding_download_model(
    State(_s): State<Arc<UiState>>,
    Json(body): Json<DownloadBody>,
) -> Response {
    let model = body.model.trim().to_owned();
    if model.is_empty() {
        return AppError(StatusCode::BAD_REQUEST, "missing 'model' name".into()).into_response();
    }
    if !cfg!(feature = "local-embed") {
        return AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Model downloads require the 'local-embed' feature. \
             Rebuild with: cargo build --features local-embed \
             (or local-embed-metal on Apple Silicon)."
                .to_owned(),
        )
        .into_response();
    }

    let cache_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".senclaw")
        .join("models");

    let (tx, rx) = mpsc::channel::<DlEvent>(32);

    // Drive the download on a background task — the request handler returns
    // immediately with the SSE response wired to `rx`. The task lives as
    // long as the channel is open; closing it cleanly aborts.
    tokio::spawn(async move {
        if let Err(e) = run_download(&model, cache_dir, tx.clone()).await {
            let _ = tx
                .send(DlEvent::Error {
                    message: e.to_string(),
                })
                .await;
        }
    });

    // Convert mpsc::Receiver<DlEvent> → impl Stream<Item = Result<Event, …>>
    let stream = ReceiverStream::new(rx).map(|ev| {
        let json = serde_json::to_string(&ev).unwrap_or_else(|_| "{}".into());
        Ok::<_, std::convert::Infallible>(Event::default().data(json))
    });

    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response()
}

// =====================================================================
// Download driver
// =====================================================================
//
// On the default (no-feature) build this still compiles — but
// `embedding_download_model` short-circuits with 503 before ever calling
// it, so the empty body is fine.

#[cfg(feature = "local-embed")]
async fn run_download(
    model: &str,
    cache_dir: PathBuf,
    tx: mpsc::Sender<DlEvent>,
) -> anyhow::Result<()> {
    use crate::memory::embedding_providers::local_candle;

    let repo_id = local_candle::resolve_repo(model)?.to_owned();
    let files = vec![
        "config.json".to_owned(),
        "tokenizer.json".to_owned(),
        "model.safetensors".to_owned(),
    ];

    let _ = tx
        .send(DlEvent::Start {
            repo: repo_id.clone(),
            files: files.clone(),
        })
        .await;

    let dest_dir = cache_dir.join(repo_id.replace('/', "--"));
    let client = reqwest::Client::new();

    for filename in &files {
        // Allow PyTorch safetensors as a fallback for the weights file —
        // matches `files_from_hub`.
        let primary_ok = stream_one(&client, &repo_id, filename, &dest_dir, tx.clone()).await;
        if let Err(e) = primary_ok {
            if filename == "model.safetensors" {
                tracing::warn!(
                    "[LocalEmbed] {filename} failed ({e}); trying pytorch_model.safetensors"
                );
                stream_one(
                    &client,
                    &repo_id,
                    "pytorch_model.safetensors",
                    &dest_dir,
                    tx.clone(),
                )
                .await?;
            } else {
                return Err(e);
            }
        }
    }

    let _ = tx
        .send(DlEvent::Done {
            dir: dest_dir.to_string_lossy().into_owned(),
        })
        .await;
    Ok(())
}

#[cfg(not(feature = "local-embed"))]
async fn run_download(
    _model: &str,
    _cache_dir: PathBuf,
    _tx: mpsc::Sender<DlEvent>,
) -> anyhow::Result<()> {
    Ok(())
}

/// Stream-download a single file with per-chunk progress events. Writes
/// atomically (`*.tmp` → rename) so a crash mid-download doesn't leave a
/// half-written file the next run would mistake for "already cached".
#[cfg(feature = "local-embed")]
async fn stream_one(
    client: &reqwest::Client,
    repo_id: &str,
    filename: &str,
    dest_dir: &Path,
    tx: mpsc::Sender<DlEvent>,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use tokio::io::AsyncWriteExt;

    let dest = dest_dir.join(filename);
    if dest.exists() {
        // Treat already-cached as a zero-byte "done" so the UI still ticks.
        let _ = tx
            .send(DlEvent::FileStart {
                file: filename.to_owned(),
                total: Some(0),
            })
            .await;
        let _ = tx
            .send(DlEvent::FileDone {
                file: filename.to_owned(),
            })
            .await;
        return Ok(());
    }

    tokio::fs::create_dir_all(dest_dir).await.context("mkdir")?;
    let url = format!("https://huggingface.co/{repo_id}/resolve/main/{filename}");

    let response = client
        .get(&url)
        .send()
        .await
        .context("HTTP request")?
        .error_for_status()
        .context("HTTP error")?;

    let total = response.content_length();
    let _ = tx
        .send(DlEvent::FileStart {
            file: filename.to_owned(),
            total,
        })
        .await;

    let tmp = dest.with_extension("tmp");
    let mut file = tokio::fs::File::create(&tmp).await.context("create tmp")?;
    let mut stream = response.bytes_stream();
    let mut downloaded: u64 = 0;

    // Throttle progress events — one per ~64 KiB OR ~250 ms whichever first.
    // Stops the UI from getting buried for big weight files.
    let mut bytes_since_event: u64 = 0;
    let mut last_emit = std::time::Instant::now();
    let progress_threshold: u64 = 64 * 1024;
    let progress_interval = Duration::from_millis(250);

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.context("stream chunk")?;
        file.write_all(&chunk).await.context("write chunk")?;
        downloaded += chunk.len() as u64;
        bytes_since_event += chunk.len() as u64;

        if bytes_since_event >= progress_threshold || last_emit.elapsed() >= progress_interval {
            let _ = tx
                .send(DlEvent::Progress {
                    file: filename.to_owned(),
                    downloaded,
                    total,
                })
                .await;
            bytes_since_event = 0;
            last_emit = std::time::Instant::now();
        }
    }
    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp, &dest).await.context("rename")?;

    let _ = tx
        .send(DlEvent::FileDone {
            file: filename.to_owned(),
        })
        .await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_model_is_rejected() {
        let body = DownloadBody { model: "  ".into() };
        assert!(body.model.trim().is_empty());
    }

    #[test]
    fn dl_event_serializes_as_tagged() {
        let ev = DlEvent::Progress {
            file: "model.safetensors".into(),
            downloaded: 1024,
            total: Some(4096),
        };
        let j = serde_json::to_string(&ev).unwrap();
        assert!(j.contains("\"phase\":\"progress\""));
        assert!(j.contains("\"downloaded\":1024"));
        assert!(j.contains("\"total\":4096"));
    }

    #[test]
    fn is_model_cached_requires_both_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        assert!(!is_model_cached(&dir), "empty dir → not cached");
        std::fs::write(dir.join("tokenizer.json"), "{}").unwrap();
        assert!(!is_model_cached(&dir), "tokenizer alone → not cached");
        std::fs::write(dir.join("model.safetensors"), "fake").unwrap();
        assert!(is_model_cached(&dir), "both files → cached");
    }

    #[test]
    fn is_model_cached_accepts_pytorch_safetensors_variant() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        std::fs::write(dir.join("tokenizer.json"), "{}").unwrap();
        std::fs::write(dir.join("pytorch_model.safetensors"), "fake").unwrap();
        assert!(is_model_cached(&dir));
    }

    #[test]
    fn catalog_covers_known_dropdown_options() {
        // These are the IDs the UI dropdown shows. Backend catalog must
        // include all of them so the "installed" badge can light up.
        let ids: Vec<&str> = MODEL_CATALOG.iter().map(|(id, _, _, _)| *id).collect();
        for expected in [
            "all-MiniLM-L6-v2",
            "bge-small-en-v1.5",
            "multilingual-e5-small",
        ] {
            assert!(ids.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn dl_event_error_shape() {
        let ev = DlEvent::Error {
            message: "boom".into(),
        };
        let j = serde_json::to_string(&ev).unwrap();
        assert!(j.contains("\"phase\":\"error\""));
        assert!(j.contains("boom"));
    }
}
