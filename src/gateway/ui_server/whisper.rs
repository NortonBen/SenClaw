//! Whisper ASR management API.
//!
//! Endpoints (all under `/api/whisper`):
//!   GET    /api/whisper/models                  — catalog + install/download status
//!   POST   /api/whisper/models/:id/download     — composite HF download (background)
//!   GET    /api/whisper/models/:id/status       — poll download progress
//!   POST   /api/whisper/models/:id/cancel       — cancel in-flight download
//!   DELETE /api/whisper/models/:id              — remove model dir
//!   GET    /api/whisper/settings                — { model_id, language }
//!   PUT    /api/whisper/settings                — persist selection
//!   POST   /api/whisper/transcribe              — multipart audio → { text }
//!
//! Whisper checkpoints on mlx-community ship no tokenizer, so a download is
//! **composite**: weights+config from the mlx repo, `tokenizer.json` from the
//! paired `openai/whisper-*` repo, assembled into one model dir.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use axum_extra::extract::Multipart;
use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::gateway::group_manager::{
    load_whisper_settings, save_whisper_settings, WhisperSettings,
};

use super::core::{AppError, UiState};

const HF_BASE: &str = "https://huggingface.co";

/// A curated Whisper model: where to fetch weights, and where to borrow the
/// tokenizer.json from (mlx-community repos don't ship one).
struct CatalogEntry {
    /// Public id (also the weights HF repo) — what the UI passes back.
    id: &'static str,
    label: &'static str,
    approx_size_gb: f32,
    /// Repo to pull `tokenizer.json` from (transformers layout).
    tokenizer_repo: &'static str,
    /// Whisper language code default for this model.
    default_language: &'static str,
}

static CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "mlx-community/whisper-large-v3-turbo",
        label: "Whisper large-v3-turbo (MLX, 128-mel, fast multilingual)",
        approx_size_gb: 1.6,
        tokenizer_repo: "openai/whisper-large-v3-turbo",
        default_language: "vi",
    },
    CatalogEntry {
        id: "mlx-community/whisper-large-v3-turbo-4bit",
        label: "Whisper large-v3-turbo 4-bit (MLX, smaller/faster download)",
        approx_size_gb: 0.46,
        tokenizer_repo: "openai/whisper-large-v3-turbo",
        default_language: "vi",
    },
];

fn catalog_get(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

fn safe_dirname(id: &str) -> String {
    id.replace('/', "__")
}

fn unsafe_dirname(name: &str) -> Option<String> {
    let (org, repo) = name.split_once("__")?;
    if org.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{org}/{repo}"))
}

fn model_dir(state: &UiState, id: &str) -> PathBuf {
    state.config.paths.whisper_models_dir.join(safe_dirname(id))
}

fn legacy_model_dir(state: &UiState, id: &str) -> PathBuf {
    state.config.paths.local_models_dir.join(safe_dirname(id))
}

fn installed_model_dir(state: &UiState, id: &str) -> PathBuf {
    let dir = model_dir(state, id);
    if is_installed(&dir) {
        return dir;
    }
    let legacy = legacy_model_dir(state, id);
    if is_installed(&legacy) {
        legacy
    } else {
        dir
    }
}

fn is_whisper_model_dir(dir: &PathBuf) -> bool {
    let Ok(file) = std::fs::File::open(dir.join("config.json")) else {
        return false;
    };
    let Ok(cfg) = serde_json::from_reader::<_, serde_json::Value>(file) else {
        return false;
    };
    [
        "n_mels",
        "n_audio_ctx",
        "n_audio_state",
        "n_audio_layer",
        "n_text_ctx",
        "n_text_state",
        "n_text_layer",
        "n_vocab",
    ]
    .iter()
    .all(|k| cfg.get(*k).and_then(|v| v.as_i64()).is_some())
}

/// A Whisper dir is "installed" once Whisper config + weights + tokenizer are present.
fn is_installed(dir: &PathBuf) -> bool {
    is_whisper_model_dir(dir)
        && (dir.join("weights.safetensors").exists() || dir.join("model.safetensors").exists())
        && dir.join("tokenizer.json").exists()
}

// ── Download progress (process-global) ───────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
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
struct DownloadState {
    model_id: String,
    status: DownloadStatus,
    total_bytes: u64,
    downloaded_bytes: u64,
    current_file: Option<String>,
    files_total: u32,
    files_done: u32,
    error: Option<String>,
}

#[derive(Clone)]
struct DownloadHandle {
    state: Arc<Mutex<DownloadState>>,
    cancel: CancellationToken,
}

static DOWNLOADS: Lazy<Mutex<HashMap<String, DownloadHandle>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ── Routes: model listing ────────────────────────────────────────────────────

pub(crate) async fn whisper_models_list(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let downloads = DOWNLOADS.lock().unwrap();
    let mut models = Vec::new();
    for e in CATALOG {
        let dir = installed_model_dir(&state, e.id);
        let download = downloads.get(e.id).map(|h| h.state.lock().unwrap().clone());
        models.push(json!({
            "id": e.id,
            "label": e.label,
            "approx_size_gb": e.approx_size_gb,
            "default_language": e.default_language,
            "installed": is_installed(&dir),
            "on_disk_path": dir.to_string_lossy(),
            "download": download,
        }));
    }
    for root in [
        &state.config.paths.whisper_models_dir,
        &state.config.paths.local_models_dir,
    ] {
        if let Ok(entries) = std::fs::read_dir(root) {
            for entry in entries.flatten() {
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if !file_type.is_dir() {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_string();
                let Some(id) = unsafe_dirname(&name) else {
                    continue;
                };
                if catalog_get(&id).is_some() || models.iter().any(|m| m["id"] == id) {
                    continue;
                }
                let dir = entry.path();
                let download = downloads.get(&id).map(|h| h.state.lock().unwrap().clone());
                let allow_legacy = root == &state.config.paths.whisper_models_dir
                    || is_whisper_model_dir(&dir)
                    || download.is_some();
                if allow_legacy && (is_installed(&dir) || download.is_some()) {
                    models.push(json!({
                        "id": id,
                        "label": format!("Whisper custom ({id})"),
                        "approx_size_gb": 0.0,
                        "default_language": "vi",
                        "installed": is_installed(&dir),
                        "on_disk_path": dir.to_string_lossy(),
                        "download": download,
                    }));
                }
            }
        }
    }
    for (id, handle) in downloads.iter() {
        if catalog_get(id).is_some() || models.iter().any(|m| m["id"] == *id) {
            continue;
        }
        let dir = installed_model_dir(&state, id);
        models.push(json!({
            "id": id,
            "label": format!("Whisper custom ({id})"),
            "approx_size_gb": 0.0,
            "default_language": "vi",
            "installed": is_installed(&dir),
            "on_disk_path": dir.to_string_lossy(),
            "download": handle.state.lock().unwrap().clone(),
        }));
    }
    Ok(Json(json!({ "models": models })))
}

// ── Routes: download (composite) ─────────────────────────────────────────────

/// Normalize the user's input into a HuggingFace `org/repo` id. Accepts bare ids
/// and full HuggingFace URLs.
fn normalize_hf_id(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty model id".into());
    }
    let stripped = s
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("huggingface.co/")
        .trim_start_matches("hf.co/")
        .trim_end_matches('/');
    let parts: Vec<&str> = stripped.split('/').collect();
    if parts.len() < 2 {
        return Err(format!("expected `org/repo` form, got `{s}`"));
    }
    let org = parts[0];
    let repo = parts[1];
    if org.is_empty() || repo.is_empty() {
        return Err(format!("invalid `org/repo` in `{s}`"));
    }
    for seg in [org, repo] {
        if seg.contains("..") || seg.contains('\\') {
            return Err(format!("unsafe path segment in `{s}`"));
        }
    }
    Ok(format!("{org}/{repo}"))
}

fn infer_tokenizer_repo(weights_repo: &str) -> String {
    let repo_name = weights_repo
        .split('/')
        .next_back()
        .unwrap_or(weights_repo)
        .trim();
    let repo_name = repo_name
        .strip_suffix("-4bit")
        .or_else(|| repo_name.strip_suffix("-8bit"))
        .or_else(|| repo_name.strip_suffix("-fp16"))
        .or_else(|| repo_name.strip_suffix("-bf16"))
        .unwrap_or(repo_name);
    if repo_name.starts_with("whisper-") {
        format!("openai/{repo_name}")
    } else {
        "openai/whisper-large-v3-turbo".to_string()
    }
}

pub(crate) async fn whisper_download(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let id = normalize_hf_id(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    let tokenizer_repo = catalog_get(&id)
        .map(|e| e.tokenizer_repo.to_string())
        .unwrap_or_else(|| infer_tokenizer_repo(&id));

    {
        let downloads = DOWNLOADS.lock().unwrap();
        if let Some(h) = downloads.get(&id) {
            let s = h.state.lock().unwrap();
            if matches!(
                s.status,
                DownloadStatus::Queued | DownloadStatus::Listing | DownloadStatus::Downloading
            ) {
                return Err(AppError(
                    StatusCode::CONFLICT,
                    format!("download for {id} already in progress"),
                ));
            }
        }
    }

    let dir = model_dir(&state, &id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let progress = Arc::new(Mutex::new(DownloadState {
        model_id: id.clone(),
        status: DownloadStatus::Queued,
        total_bytes: 0,
        downloaded_bytes: 0,
        current_file: None,
        files_total: 0,
        files_done: 0,
        error: None,
    }));
    let cancel = CancellationToken::new();
    DOWNLOADS.lock().unwrap().insert(
        id.clone(),
        DownloadHandle {
            state: progress.clone(),
            cancel: cancel.clone(),
        },
    );

    let weights_repo = id.clone();
    tokio::spawn(async move {
        let result = run_whisper_download(
            &weights_repo,
            &tokenizer_repo,
            &dir,
            progress.clone(),
            cancel,
        )
        .await;
        let mut s = progress.lock().unwrap();
        match result {
            Ok(()) if s.status != DownloadStatus::Cancelled => s.status = DownloadStatus::Done,
            Ok(()) => {}
            Err(e) => {
                s.status = DownloadStatus::Error;
                s.error = Some(e.to_string());
            }
        }
    });

    Ok(Json(json!({ "ok": true, "id": id })))
}

pub(crate) async fn whisper_status(
    State(_state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let downloads = DOWNLOADS.lock().unwrap();
    let progress = downloads.get(&id).map(|h| h.state.lock().unwrap().clone());
    Ok(Json(json!({ "id": id, "download": progress })))
}

pub(crate) async fn whisper_cancel(
    State(_state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let downloads = DOWNLOADS.lock().unwrap();
    if let Some(h) = downloads.get(&id) {
        h.cancel.cancel();
        h.state.lock().unwrap().status = DownloadStatus::Cancelled;
    }
    Ok(Json(json!({ "ok": true })))
}

pub(crate) async fn whisper_delete(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let dir = model_dir(&state, &id);
    if dir.exists() {
        tokio::fs::remove_dir_all(&dir)
            .await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    let legacy = legacy_model_dir(&state, &id);
    if legacy.exists() {
        tokio::fs::remove_dir_all(&legacy)
            .await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    DOWNLOADS.lock().unwrap().remove(&id);
    // Drop any cached engine bound to this dir.
    drop_engine(&dir);
    drop_engine(&legacy);
    Ok(Json(json!({ "ok": true })))
}

// ── Routes: settings ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct WhisperSettingsBody {
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

pub(crate) async fn whisper_settings_get(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let s = load_whisper_settings(&state.config.paths.global_config_path);
    Ok(Json(json!({
        "model_id": s.model_id,
        "language": s.language.unwrap_or_else(|| "vi".to_string()),
    })))
}

pub(crate) async fn whisper_settings_put(
    State(state): State<Arc<UiState>>,
    Json(body): Json<WhisperSettingsBody>,
) -> Result<impl IntoResponse, AppError> {
    let settings = WhisperSettings {
        model_id: body.model_id,
        language: body.language,
    };
    save_whisper_settings(&state.config.paths.global_config_path, &settings)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

// ── Route: transcribe (multipart audio) ──────────────────────────────────────

pub(crate) async fn whisper_transcribe(
    State(state): State<Arc<UiState>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let mut audio: Option<(String, Vec<u8>)> = None;
    let mut language: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("read multipart: {e}")))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name == "language" {
            language = field.text().await.ok().filter(|s| !s.is_empty());
        } else {
            // Treat any other field as the audio payload.
            let filename = field.file_name().unwrap_or("audio.bin").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("read audio: {e}")))?;
            audio = Some((filename, bytes.to_vec()));
        }
    }

    let (filename, bytes) =
        audio.ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "no audio field".into()))?;

    // Pick the selected model, else the only installed catalog model.
    let settings = load_whisper_settings(&state.config.paths.global_config_path);
    let model_id = settings
        .model_id
        .clone()
        .or_else(|| {
            CATALOG
                .iter()
                .map(|e| e.id.to_string())
                .find(|id| is_installed(&installed_model_dir(&state, id)))
        })
        .ok_or_else(|| {
            AppError(
                StatusCode::BAD_REQUEST,
                "no Whisper model selected or installed".into(),
            )
        })?;
    let dir = installed_model_dir(&state, &model_id);
    if !is_installed(&dir) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            format!("model `{model_id}` is not installed"),
        ));
    }
    let lang = language.or(settings.language);

    transcribe_impl(dir, filename, bytes, lang).await
}

// ── Composite download worker ────────────────────────────────────────────────

#[derive(Deserialize)]
struct HfTreeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    path: String,
    #[serde(default)]
    size: u64,
}

fn should_skip(name: &str) -> bool {
    let lower = name.to_lowercase();
    matches!(
        lower.as_str(),
        ".gitattributes" | "readme.md" | "license" | "license.md" | "license.txt"
    ) || lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".svg")
}

async fn run_whisper_download(
    weights_repo: &str,
    tokenizer_repo: &str,
    dir: &PathBuf,
    progress: Arc<Mutex<DownloadState>>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    progress.lock().unwrap().status = DownloadStatus::Listing;

    // List the weights repo tree.
    let tree_url = format!("{HF_BASE}/api/models/{weights_repo}/tree/main?recursive=true");
    let tree: Vec<HfTreeEntry> = client
        .get(&tree_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let mut files: Vec<(String, String, u64)> = tree
        .into_iter()
        .filter(|e| e.entry_type == "file" && !should_skip(&e.path))
        .map(|e| (weights_repo.to_string(), e.path, e.size))
        .collect();
    // Append the tokenizer from the paired repo (size unknown → 0).
    files.push((tokenizer_repo.to_string(), "tokenizer.json".to_string(), 0));

    {
        let mut s = progress.lock().unwrap();
        s.files_total = files.len() as u32;
        s.total_bytes = files.iter().map(|f| f.2).sum();
        s.status = DownloadStatus::Downloading;
    }

    for (repo, path, size) in files {
        if cancel.is_cancelled() {
            progress.lock().unwrap().status = DownloadStatus::Cancelled;
            return Ok(());
        }
        progress.lock().unwrap().current_file = Some(path.clone());

        let dst = dir.join(&path);
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // Resume: skip if a complete copy exists.
        if size > 0 {
            if let Ok(meta) = tokio::fs::metadata(&dst).await {
                if meta.len() == size {
                    let mut s = progress.lock().unwrap();
                    s.files_done += 1;
                    s.downloaded_bytes += size;
                    continue;
                }
            }
        }

        let url = format!("{HF_BASE}/{repo}/resolve/main/{path}");
        let resp = client.get(&url).send().await?.error_for_status()?;
        let mut stream = resp.bytes_stream();
        let mut file = tokio::fs::File::create(&dst).await?;
        while let Some(chunk) = stream.next().await {
            if cancel.is_cancelled() {
                drop(file);
                let _ = tokio::fs::remove_file(&dst).await;
                progress.lock().unwrap().status = DownloadStatus::Cancelled;
                return Ok(());
            }
            let bytes = chunk?;
            file.write_all(&bytes).await?;
            progress.lock().unwrap().downloaded_bytes += bytes.len() as u64;
        }
        file.flush().await?;
        progress.lock().unwrap().files_done += 1;
    }

    Ok(())
}

// ── Engine cache + transcription bridge ──────────────────────────────────────

#[cfg(feature = "local-mlx-whisper")]
static ENGINES: Lazy<Mutex<HashMap<String, Arc<crate::local_model::WhisperEngine>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(feature = "local-mlx-whisper")]
fn get_or_create_engine(dir: &PathBuf) -> Arc<crate::local_model::WhisperEngine> {
    let key = dir.to_string_lossy().to_string();
    let mut map = ENGINES.lock().unwrap();
    map.entry(key)
        .or_insert_with(|| Arc::new(crate::local_model::WhisperEngine::new(dir.clone())))
        .clone()
}

#[cfg(feature = "local-mlx-whisper")]
fn drop_engine(dir: &PathBuf) {
    ENGINES
        .lock()
        .unwrap()
        .remove(&dir.to_string_lossy().to_string());
}

#[cfg(not(feature = "local-mlx-whisper"))]
fn drop_engine(_dir: &PathBuf) {}

#[cfg(feature = "local-mlx-whisper")]
async fn transcribe_impl(
    dir: PathBuf,
    filename: String,
    bytes: Vec<u8>,
    language: Option<String>,
) -> Result<axum::response::Json<serde_json::Value>, AppError> {
    let debug = matches!(
        std::env::var("SENCLAW_WHISPER_DEBUG").as_deref(),
        Ok("1" | "true" | "TRUE" | "yes" | "YES")
    );
    if debug {
        eprintln!(
            "[whisper-debug] api transcribe request filename={filename:?} bytes={} model_dir={} language={:?}",
            bytes.len(),
            dir.display(),
            language
        );
    }
    // Persist the upload to a temp file so symphonia can probe by extension.
    let ext = std::path::Path::new(&filename)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("bin");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp = std::env::temp_dir().join(format!(
        "senclaw-whisper-{}-{nonce}.{ext}",
        std::process::id()
    ));
    tokio::fs::write(&tmp, &bytes)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let engine = get_or_create_engine(&dir);
    let tmp_for_task = tmp.clone();
    let (text, stats) = tokio::task::spawn_blocking(move || {
        engine.transcribe_file_timed(&tmp_for_task, language.as_deref())
    })
    .await
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = tokio::fs::remove_file(&tmp).await;
    if debug {
        eprintln!(
            "[whisper-debug] api transcribe response chars={} chunks={} tokens={} no_speech_prob={:.3} avg_logprob={:.3} total_ms={:.1}",
            text.chars().count(),
            stats.n_chunks,
            stats.tokens,
            stats.no_speech_prob,
            stats.avg_logprob,
            stats.total_ms
        );
    }

    Ok(Json(json!({ "ok": true, "text": text })))
}

#[cfg(not(feature = "local-mlx-whisper"))]
async fn transcribe_impl(
    _dir: PathBuf,
    _filename: String,
    _bytes: Vec<u8>,
    _language: Option<String>,
) -> Result<axum::response::Json<serde_json::Value>, AppError> {
    Err(AppError(
        StatusCode::NOT_IMPLEMENTED,
        "Whisper transcription requires building with `--features local-mlx-whisper`".into(),
    ))
}
