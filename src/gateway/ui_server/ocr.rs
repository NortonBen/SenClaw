//! OCR (PaddleOCR + MNN) management & inference API.
//!
//! Endpoints (all under `/api/ocr`):
//!   GET    /api/ocr/models                  — catalog + install/download status
//!   POST   /api/ocr/models/:id/download     — fetch det/rec/keys (background)
//!   GET    /api/ocr/models/:id/status       — poll download progress
//!   POST   /api/ocr/models/:id/cancel       — cancel in-flight download
//!   DELETE /api/ocr/models/:id              — remove model dir
//!   GET    /api/ocr/settings                — { model_id, language }
//!   PUT    /api/ocr/settings                — persist selection
//!   POST   /api/ocr/recognize               — multipart image → { text, blocks }
//!
//! Unlike Whisper, the model "download" is just three direct URLs (det.mnn,
//! rec.mnn, keys.txt) per catalog entry — no HuggingFace tree walk.

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

use crate::gateway::group_manager::{load_ocr_settings, save_ocr_settings, OcrSettings};

use super::core::{AppError, UiState};

#[cfg(feature = "ocr-paddle")]
use crate::local_model::ocr::{
    installed_model_files, CatalogEntry, CATALOG, DEFAULT_MODEL_ID, DET_FILE, KEYS_FILE, REC_FILE,
};

// On builds without OCR support we still need the catalog shape so the UI can
// render — provide a stub. The download/recognize handlers below check the
// feature flag and return 501 when missing.
#[cfg(not(feature = "ocr-paddle"))]
mod stub {
    pub struct CatalogEntry {
        pub id: &'static str,
        pub label: &'static str,
        pub description: &'static str,
        pub det_url: &'static str,
        pub rec_url: &'static str,
        pub keys_url: &'static str,
        pub approx_size_mb: f32,
        pub default_language: &'static str,
        pub version: u8,
        pub is_default: bool,
    }
    pub static CATALOG: &[CatalogEntry] = &[];
    pub const DEFAULT_MODEL_ID: &str = "";
    pub const DET_FILE: &str = "det.mnn";
    pub const REC_FILE: &str = "rec.mnn";
    pub const KEYS_FILE: &str = "keys.txt";
    pub fn installed_model_files(
        dir: &std::path::Path,
    ) -> (std::path::PathBuf, std::path::PathBuf, std::path::PathBuf) {
        (dir.join(DET_FILE), dir.join(REC_FILE), dir.join(KEYS_FILE))
    }
}
#[cfg(not(feature = "ocr-paddle"))]
use stub::{
    installed_model_files, CatalogEntry, CATALOG, DEFAULT_MODEL_ID, DET_FILE, KEYS_FILE, REC_FILE,
};

fn catalog_get(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

fn safe_dirname(id: &str) -> String {
    id.replace('/', "__")
}

fn model_dir(state: &UiState, id: &str) -> PathBuf {
    state.config.paths.ocr_models_dir.join(safe_dirname(id))
}

fn is_installed(dir: &PathBuf) -> bool {
    let (det, rec, keys) = installed_model_files(dir);
    [det, rec, keys].iter().all(|p| {
        std::fs::metadata(p)
            .map(|m| m.len() > 0)
            .unwrap_or(false)
    })
}

// ── Download progress (process-global) ───────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
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

pub(crate) async fn ocr_models_list(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let downloads = DOWNLOADS.lock().unwrap();
    let mut models = Vec::new();
    for e in CATALOG {
        let dir = model_dir(&state, e.id);
        let download = downloads.get(e.id).map(|h| h.state.lock().unwrap().clone());
        models.push(json!({
            "id": e.id,
            "label": e.label,
            "description": e.description,
            "approx_size_mb": e.approx_size_mb,
            "default_language": e.default_language,
            "version": e.version,
            "is_default": e.is_default,
            "installed": is_installed(&dir),
            "on_disk_path": dir.to_string_lossy(),
            "download": download,
        }));
    }
    // Also surface any download entries the user kicked off for ids not in the
    // bundled catalog (custom URLs etc.).
    for (id, handle) in downloads.iter() {
        if catalog_get(id).is_some() || models.iter().any(|m| m["id"] == *id) {
            continue;
        }
        let dir = model_dir(&state, id);
        models.push(json!({
            "id": id,
            "label": format!("OCR custom ({id})"),
            "description": "User-supplied model URLs",
            "approx_size_mb": 0.0,
            "default_language": "vi",
            "version": 0,
            "is_default": false,
            "installed": is_installed(&dir),
            "on_disk_path": dir.to_string_lossy(),
            "download": handle.state.lock().unwrap().clone(),
        }));
    }
    // Also scan on-disk dir for any custom-installed models the user dropped
    // in manually (not in catalog, no download record).
    if let Ok(entries) = std::fs::read_dir(&state.config.paths.ocr_models_dir) {
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            // dir-name → id (reverse of safe_dirname; OCR ids never contain '/')
            let id = name.clone();
            if catalog_get(&id).is_some() || models.iter().any(|m| m["id"] == id) {
                continue;
            }
            let dir = entry.path();
            if !is_installed(&dir) {
                continue;
            }
            models.push(json!({
                "id": id,
                "label": format!("OCR custom ({id})"),
                "description": "Manually-installed model directory",
                "approx_size_mb": 0.0,
                "default_language": "vi",
                "version": 0,
                "is_default": false,
                "installed": true,
                "on_disk_path": dir.to_string_lossy(),
                "download": serde_json::Value::Null,
            }));
        }
    }
    Ok(Json(json!({
        "models": models,
        "default_model_id": DEFAULT_MODEL_ID,
    })))
}

// ── Routes: download ─────────────────────────────────────────────────────────

pub(crate) async fn ocr_download(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let entry = catalog_get(&id).ok_or_else(|| {
        AppError(
            StatusCode::BAD_REQUEST,
            format!("unknown OCR model id `{id}`"),
        )
    })?;

    {
        let downloads = DOWNLOADS.lock().unwrap();
        if let Some(h) = downloads.get(&id) {
            let s = h.state.lock().unwrap();
            if matches!(s.status, DownloadStatus::Queued | DownloadStatus::Downloading) {
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
        files_total: 3,
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

    let files = vec![
        (entry.det_url.to_string(), DET_FILE.to_string()),
        (entry.rec_url.to_string(), REC_FILE.to_string()),
        (entry.keys_url.to_string(), KEYS_FILE.to_string()),
    ];

    tokio::spawn(async move {
        let result = run_ocr_download(files, &dir, progress.clone(), cancel).await;
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

// ── Route: custom URL download ───────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct OcrCustomDownloadBody {
    /// Free-form id chosen by the user (e.g. `my-vietnamese-v5`). Used as the
    /// directory name; must not contain `/`.
    id: String,
    det_url: String,
    rec_url: String,
    keys_url: String,
}

pub(crate) async fn ocr_custom_download(
    State(state): State<Arc<UiState>>,
    Json(body): Json<OcrCustomDownloadBody>,
) -> Result<impl IntoResponse, AppError> {
    let id = body.id.trim().to_string();
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "invalid model id (no '/', '\\\\', '..')".into(),
        ));
    }
    for (label, url) in [
        ("det_url", &body.det_url),
        ("rec_url", &body.rec_url),
        ("keys_url", &body.keys_url),
    ] {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("{label} must start with http(s)://"),
            ));
        }
    }

    {
        let downloads = DOWNLOADS.lock().unwrap();
        if let Some(h) = downloads.get(&id) {
            let s = h.state.lock().unwrap();
            if matches!(s.status, DownloadStatus::Queued | DownloadStatus::Downloading) {
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
        files_total: 3,
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

    let files = vec![
        (body.det_url, DET_FILE.to_string()),
        (body.rec_url, REC_FILE.to_string()),
        (body.keys_url, KEYS_FILE.to_string()),
    ];

    tokio::spawn(async move {
        let result = run_ocr_download(files, &dir, progress.clone(), cancel).await;
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

pub(crate) async fn ocr_status(
    State(_state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let downloads = DOWNLOADS.lock().unwrap();
    let progress = downloads.get(&id).map(|h| h.state.lock().unwrap().clone());
    Ok(Json(json!({ "id": id, "download": progress })))
}

pub(crate) async fn ocr_cancel(
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

pub(crate) async fn ocr_delete(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let dir = model_dir(&state, &id);
    if dir.exists() {
        tokio::fs::remove_dir_all(&dir)
            .await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    DOWNLOADS.lock().unwrap().remove(&id);
    drop_engine(&dir);
    Ok(Json(json!({ "ok": true })))
}

// ── Routes: settings ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct OcrSettingsBody {
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    language: Option<String>,
}

/// Pick the first installed model — preferring the catalog default, then the
/// rest of the catalog in order, then any custom-installed dirs.
fn auto_select_model_id(state: &UiState) -> Option<String> {
    if !DEFAULT_MODEL_ID.is_empty() && is_installed(&model_dir(state, DEFAULT_MODEL_ID)) {
        return Some(DEFAULT_MODEL_ID.to_string());
    }
    for e in CATALOG {
        if is_installed(&model_dir(state, e.id)) {
            return Some(e.id.to_string());
        }
    }
    // Custom installed dirs.
    if let Ok(entries) = std::fs::read_dir(&state.config.paths.ocr_models_dir) {
        for entry in entries.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if !ft.is_dir() {
                continue;
            }
            if is_installed(&entry.path()) {
                return Some(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    None
}

pub(crate) async fn ocr_settings_get(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let mut s = load_ocr_settings(&state.config.paths.global_config_path);
    // Auto-promote the first installed model so the user doesn't have to
    // manually pick after their first download.
    if s.model_id.is_none() {
        if let Some(id) = auto_select_model_id(&state) {
            s.model_id = Some(id.clone());
            let _ = save_ocr_settings(&state.config.paths.global_config_path, &s);
        }
    }
    Ok(Json(json!({
        "model_id": s.model_id,
        "language": s.language.unwrap_or_else(|| "vi".to_string()),
        "default_model_id": DEFAULT_MODEL_ID,
    })))
}

pub(crate) async fn ocr_settings_put(
    State(state): State<Arc<UiState>>,
    Json(body): Json<OcrSettingsBody>,
) -> Result<impl IntoResponse, AppError> {
    let settings = OcrSettings {
        model_id: body.model_id,
        language: body.language,
    };
    save_ocr_settings(&state.config.paths.global_config_path, &settings)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

// ── Route: recognize (multipart image) ───────────────────────────────────────

pub(crate) async fn ocr_recognize(
    State(state): State<Arc<UiState>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let mut image: Option<Vec<u8>> = None;
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
            let bytes = field
                .bytes()
                .await
                .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("read image: {e}")))?;
            image = Some(bytes.to_vec());
        }
    }

    let bytes = image.ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "no image field".into()))?;

    let settings = load_ocr_settings(&state.config.paths.global_config_path);
    let model_id = settings
        .model_id
        .clone()
        .or_else(|| {
            CATALOG
                .iter()
                .map(|e| e.id.to_string())
                .find(|id| is_installed(&model_dir(&state, id)))
        })
        .ok_or_else(|| {
            AppError(
                StatusCode::BAD_REQUEST,
                "no OCR model selected or installed".into(),
            )
        })?;
    let dir = model_dir(&state, &model_id);
    if !is_installed(&dir) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            format!("model `{model_id}` is not installed"),
        ));
    }
    let lang = language.or(settings.language).unwrap_or_else(|| "vi".into());

    recognize_impl(dir, bytes, lang).await
}

// ── Download worker ──────────────────────────────────────────────────────────

async fn run_ocr_download(
    files: Vec<(String, String)>,
    dir: &PathBuf,
    progress: Arc<Mutex<DownloadState>>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    progress.lock().unwrap().status = DownloadStatus::Downloading;

    for (url, filename) in files {
        if cancel.is_cancelled() {
            progress.lock().unwrap().status = DownloadStatus::Cancelled;
            return Ok(());
        }
        progress.lock().unwrap().current_file = Some(filename.clone());

        let dst = dir.join(&filename);
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // HEAD probe for size (best-effort; non-fatal if it fails).
        let expected_size = client
            .head(&url)
            .send()
            .await
            .ok()
            .and_then(|r| r.content_length());
        if let Some(size) = expected_size {
            if let Ok(meta) = tokio::fs::metadata(&dst).await {
                if meta.len() == size {
                    let mut s = progress.lock().unwrap();
                    s.files_done += 1;
                    s.downloaded_bytes += size;
                    continue;
                }
            }
            let mut s = progress.lock().unwrap();
            s.total_bytes += size;
        }

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

// ── Engine cache + recognize bridge ──────────────────────────────────────────

#[cfg(feature = "ocr-paddle")]
static ENGINES: Lazy<Mutex<HashMap<String, Arc<crate::local_model::OcrEngine>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(feature = "ocr-paddle")]
fn get_or_create_engine(dir: &PathBuf, lang: &str) -> Arc<crate::local_model::OcrEngine> {
    let key = dir.to_string_lossy().to_string();
    let mut map = ENGINES.lock().unwrap();
    map.entry(key)
        .or_insert_with(|| Arc::new(crate::local_model::OcrEngine::new(dir.clone(), lang)))
        .clone()
}

#[cfg(feature = "ocr-paddle")]
fn drop_engine(dir: &PathBuf) {
    ENGINES
        .lock()
        .unwrap()
        .remove(&dir.to_string_lossy().to_string());
}

#[cfg(not(feature = "ocr-paddle"))]
fn drop_engine(_dir: &PathBuf) {}

#[cfg(feature = "ocr-paddle")]
async fn recognize_impl(
    dir: PathBuf,
    bytes: Vec<u8>,
    language: String,
) -> Result<axum::response::Json<serde_json::Value>, AppError> {
    let engine = get_or_create_engine(&dir, &language);
    let result = tokio::task::spawn_blocking(move || {
        let res = engine.recognize_bytes(&bytes);
        engine.unload();
        res
    })
    .await
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({
        "ok": true,
        "text": result.text,
        "blocks": result.blocks,
    })))
}

#[cfg(not(feature = "ocr-paddle"))]
async fn recognize_impl(
    _dir: PathBuf,
    _bytes: Vec<u8>,
    _language: String,
) -> Result<axum::response::Json<serde_json::Value>, AppError> {
    Err(AppError(
        StatusCode::NOT_IMPLEMENTED,
        "OCR requires building with `--features ocr-paddle` (or `ocr-paddle-metal` on macOS)".into(),
    ))
}
