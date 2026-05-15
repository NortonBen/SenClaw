//! Local-model management API.
//!
//! Endpoints:
//!   GET    /api/local-models                — list registry + per-model status
//!   GET    /api/local-models/runtime        — feature flags + dir info
//!   POST   /api/local-models/:id/download   — start HF download (background)
//!   GET    /api/local-models/:id/status     — poll download progress
//!   POST   /api/local-models/:id/cancel     — cancel in-flight download
//!   DELETE /api/local-models/:id            — remove downloaded model dir
//!
//! Models are identified by HuggingFace repo id (e.g. `mlx-community/Qwen3-4B-bf16`).
//! On disk we replace `/` with `__` so the id maps to a single directory under
//! `paths.local_models_dir`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::gateway::group_manager::{
    load_llm_configs, save_llm_config, set_active_llm_config, LlmConfig,
};
use crate::local_model::{read_model_context_length_from_dir, KnownModel, KNOWN_MODELS};
#[cfg(feature = "local-mlx")]
use crate::local_model::{LocalModelRuntime, MlxNativeEngine};

use super::core::{AppError, UiState};

// ---------------------------------------------------------------------------
// Progress state — process-global, shared across requests.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Listing,
    Downloading,
    Done,
    Error,
    Cancelled,
}

#[derive(Clone)]
struct DownloadHandle {
    state: Arc<Mutex<DownloadState>>,
    cancel: CancellationToken,
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

static DOWNLOADS: Lazy<Mutex<HashMap<String, DownloadHandle>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// ---------------------------------------------------------------------------
// Loaded-engine registry — keeps `MlxNativeEngine` instances alive between
// requests so weights/tokenizer state can be reused. Without `local-mlx` we
// still expose the lifecycle (load/unload + a `loaded` flag) so the UI stays
// consistent, but no real engine is constructed.
// ---------------------------------------------------------------------------

#[cfg(feature = "local-mlx")]
static LOADED_ENGINES: Lazy<Mutex<HashMap<String, Arc<MlxNativeEngine>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(not(feature = "local-mlx"))]
static LOADED_ENGINES_STUB: Lazy<Mutex<std::collections::HashSet<String>>> =
    Lazy::new(|| Mutex::new(std::collections::HashSet::new()));

fn loaded_ids() -> Vec<String> {
    #[cfg(feature = "local-mlx")]
    {
        LOADED_ENGINES.lock().unwrap().keys().cloned().collect()
    }
    #[cfg(not(feature = "local-mlx"))]
    {
        LOADED_ENGINES_STUB.lock().unwrap().iter().cloned().collect()
    }
}

fn is_loaded(id: &str) -> bool {
    #[cfg(feature = "local-mlx")]
    {
        LOADED_ENGINES
            .lock()
            .unwrap()
            .contains_key(&canonical_local_model_id(id))
    }
    #[cfg(not(feature = "local-mlx"))]
    {
        LOADED_ENGINES_STUB.lock().unwrap().contains(id)
    }
}

/// Public accessor for `query_llm.rs` to reuse a loaded engine. Returns
/// `None` when the model isn't currently loaded.
#[cfg(feature = "local-mlx")]
pub fn get_loaded_engine(id: &str) -> Option<Arc<MlxNativeEngine>> {
    let cid = canonical_local_model_id(id);
    LOADED_ENGINES.lock().unwrap().get(&cid).cloned()
}

/// Normalize HuggingFace-style model id for registry keys (trim whitespace).
#[must_use]
pub fn canonical_local_model_id(id: &str) -> String {
    id.trim().to_string()
}

/// Get the cached engine for `id` or insert a fresh one. Used by ZenCore on
/// first inference so the heavy MLX state is created once per (model_id) and
/// reused for every subsequent chat — without this, each chat instantiates a
/// new engine and the previous one's unified-memory allocations linger,
/// causing RAM to grow ~3 GB per turn for a 4B-4bit model.
///
/// If [`MlxNativeEngine::kv_cache_bits`] no longer matches `kv_cache_bits`
/// (user changed turboquant settings), the old engine is [`MlxNativeEngine::unload`]d
/// and replaced so KV-cache mode stays consistent.
#[cfg(feature = "local-mlx")]
pub fn get_or_create_loaded_engine(
    id: &str,
    model_dir: &std::path::Path,
    kv_cache_bits: Option<u8>,
) -> Arc<MlxNativeEngine> {
    let cid = canonical_local_model_id(id);
    let mut map = LOADED_ENGINES.lock().unwrap();
    if let Some(existing) = map.get(&cid) {
        if existing.kv_cache_bits() == kv_cache_bits {
            return existing.clone();
        }
        tracing::info!(
            "[local-models] replacing engine `{}`: kv_cache_bits {:?} → {:?}",
            cid,
            existing.kv_cache_bits(),
            kv_cache_bits
        );
        if let Some(old) = map.remove(&cid) {
            old.unload();
        }
    }
    let engine = Arc::new(MlxNativeEngine::new(model_dir, &cid, kv_cache_bits));
    map.insert(cid, engine.clone());
    engine
}

fn safe_dirname(model_id: &str) -> String {
    model_id.replace('/', "__")
}

fn model_dir(state: &UiState, model_id: &str) -> PathBuf {
    state
        .config
        .paths
        .local_models_dir
        .join(safe_dirname(model_id))
}

fn is_installed(dir: &PathBuf) -> bool {
    dir.join("tokenizer.json").exists() && dir.join("config.json").exists()
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ModelEntry {
    id: String,
    label: String,
    approx_size_gb: f32,
    context_length: u32,
    native_supported: bool,
    installed: bool,
    on_disk_path: String,
    download: Option<DownloadState>,
    /// True for models not in the curated registry (installed via custom HF URL).
    custom: bool,
    /// True when an engine is kept warm in memory for this model.
    loaded: bool,
}

/// Reverse `safe_dirname`: `org__repo` → `org/repo`. Only treats the first
/// `__` as the separator so repo names containing `__` survive.
fn dirname_to_id(name: &str) -> Option<String> {
    let (org, repo) = name.split_once("__")?;
    if org.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{org}/{repo}"))
}

pub(crate) async fn local_models_list(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    // Snapshot the download state into owned values up-front so we never hold
    // a std::Mutex guard across `.await`.
    let download_snapshot: HashMap<String, DownloadState> = {
        let downloads = DOWNLOADS.lock().unwrap();
        downloads
            .iter()
            .map(|(k, h)| (k.clone(), h.state.lock().unwrap().clone()))
            .collect()
    };

    let mut entries: Vec<ModelEntry> = KNOWN_MODELS
        .iter()
        .map(|m| {
            let dir = model_dir(&state, m.id);
            let installed = is_installed(&dir);
            let context_length = if installed {
                read_model_context_length_from_dir(&dir).unwrap_or(m.context_length)
            } else {
                m.context_length
            };
            ModelEntry {
                id: m.id.to_string(),
                label: m.label.to_string(),
                approx_size_gb: m.approx_size_gb,
                context_length,
                native_supported: m.native_supported,
                installed,
                on_disk_path: dir.to_string_lossy().to_string(),
                download: download_snapshot.get(m.id).cloned(),
                custom: false,
                loaded: is_loaded(m.id),
            }
        })
        .collect();

    // Discover custom installs not in the registry.
    let known: std::collections::HashSet<&'static str> =
        KNOWN_MODELS.iter().map(|m| m.id).collect();
    let mut discovered: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Ok(mut rd) = tokio::fs::read_dir(&state.config.paths.local_models_dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let Ok(ft) = ent.file_type().await else { continue };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().into_owned();
            let Some(id) = dirname_to_id(&name) else { continue };
            if known.contains(id.as_str()) {
                continue;
            }
            discovered.insert(id);
        }
    }
    for id in download_snapshot.keys() {
        if !known.contains(id.as_str()) {
            discovered.insert(id.clone());
        }
    }

    for id in discovered {
        let dir = model_dir(&state, &id);
        let loaded = is_loaded(&id);
        let installed = is_installed(&dir);
        let context_length = if installed {
            read_model_context_length_from_dir(&dir).unwrap_or(DEFAULT_MLX_MAX_PROMPT_TOKENS)
        } else {
            0
        };
        entries.push(ModelEntry {
            label: format!("{} (custom)", id),
            native_supported: id.to_lowercase().contains("qwen3"),
            installed,
            on_disk_path: dir.to_string_lossy().to_string(),
            download: download_snapshot.get(&id).cloned(),
            id: id.clone(),
            approx_size_gb: 0.0,
            context_length,
            custom: true,
            loaded,
        });
    }

    Ok(Json(json!({ "models": entries })))
}

pub(crate) async fn local_models_runtime(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let mlx_feature = cfg!(feature = "local-mlx");
    let turboquant_feature = cfg!(feature = "local-mlx-turboquant");
    Ok(Json(json!({
        "feature_local_mlx": mlx_feature,
        "feature_turboquant": turboquant_feature,
        "local_models_dir": state.config.paths.local_models_dir.to_string_lossy(),
        "platform": std::env::consts::OS,
    })))
}

// ---------------------------------------------------------------------------
// Settings (KV-cache quantization) — persisted as JSON next to the models dir.
// ---------------------------------------------------------------------------

/// When `settings.json` omits `max_prompt_tokens`, native MLX uses this (see
/// `max_new_tokens` below). Higher values use more unified memory for KV + prefill;
/// tune down in `settings.json` on memory-constrained machines.
pub const DEFAULT_MLX_MAX_PROMPT_TOKENS: u32 = 128_000;

/// Default cap on **generated** tokens per request when omitted from settings.
/// Hard-capped at **8192** in API / `mlx_native` decode loop.
pub const DEFAULT_MLX_MAX_NEW_TOKENS: u32 = 8192;

/// When TurboQuant KV is active, `mlx_native` caps **decode** at this (below API 8192)
/// to cut RAM / CPU on the slow CPU attention path; raise via `max_new_tokens` in JSON only up to this cap.
pub const TURBOQUANT_MAX_NEW_TOKENS_CAP: u32 = 2048;

/// Hard ceiling on **prompt + decode** token positions when TurboQuant KV is active
/// (`kv_cache_bits` set, build with `local-mlx-turboquant`). Native MLX clamps
/// `max_prompt_tokens` so `max_prompt_tokens + max_new_tokens ≤` this value.
pub const TURBOQUANT_MAX_CONTEXT_TOKENS: u32 = 128_000 + 8192;

/// When `kv_cache_bits` is `2` (invalid for turboquant-rs QJL path), native MLX maps to this **TQ3** total bit budget (`3`).
pub const DEFAULT_TURBOQUANT_KV_TOTAL_BITS: u8 = 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalModelSettings {
    /// TurboQuant KV total bit budget: **`3` = TQ3** (default remap), **`4` = TQ4**. **`2`** is accepted in JSON for compatibility but maps to [`DEFAULT_TURBOQUANT_KV_TOTAL_BITS`] (TQ3) at runtime.
    #[serde(default)]
    pub kv_cache_bits: Option<u8>,
    /// Upper bound on prompt length after chat-template encoding. Older conversation is dropped
    /// from the **start** (suffix preserved). `None` → [`DEFAULT_MLX_MAX_PROMPT_TOKENS`].
    /// With TurboQuant KV, effective prompt cap is further limited so prompt + decode ≤ [`TURBOQUANT_MAX_CONTEXT_TOKENS`].
    #[serde(default)]
    pub max_prompt_tokens: Option<u32>,
    /// Cap on generated tokens per request. `None` → [`DEFAULT_MLX_MAX_NEW_TOKENS`].
    /// With TurboQuant KV (native MLX), runtime further caps at [`TURBOQUANT_MAX_NEW_TOKENS_CAP`].
    #[serde(default)]
    pub max_new_tokens: Option<u32>,
    /// Pass `enable_thinking` to the chat template. `None` = let the template decide (model
    /// default). `false` = disable thinking (Qwen3 pre-fills `<think>\n\n</think>` to skip
    /// the reasoning block). Defaults to `false` to avoid unbounded thinking overhead.
    #[serde(default = "default_enable_thinking")]
    pub enable_thinking: Option<bool>,
    /// Context length (tokens already in KV cache) at which TurboQuant quantization activates.
    /// `0` = quantize immediately (from first decode step). `None` → default 2048.
    #[serde(default)]
    pub tq_activate_at: Option<u32>,
}

fn default_enable_thinking() -> Option<bool> {
    Some(false)
}

fn settings_path(state: &UiState) -> PathBuf {
    state
        .config
        .paths
        .local_models_dir
        .join("settings.json")
}

pub fn load_settings_blocking(dir: &std::path::Path) -> LocalModelSettings {
    let path = dir.join("settings.json");
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => LocalModelSettings::default(),
    }
}

pub(crate) async fn local_models_settings_get(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let path = settings_path(&state);
    let settings = match tokio::fs::read_to_string(&path).await {
        Ok(s) => serde_json::from_str::<LocalModelSettings>(&s).unwrap_or_default(),
        Err(_) => LocalModelSettings::default(),
    };
    Ok(Json(settings))
}

pub(crate) async fn local_models_settings_put(
    State(state): State<Arc<UiState>>,
    Json(settings): Json<LocalModelSettings>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(bits) = settings.kv_cache_bits {
        if !matches!(bits, 2 | 3 | 4) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("kv_cache_bits must be 2, 3, or 4 (got {bits}); 2 is remapped to TQ3 at runtime when using turboquant"),
            ));
        }
    }
    if let Some(n) = settings.max_prompt_tokens {
        if !(512..=262_144).contains(&n) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("max_prompt_tokens must be between 512 and 262144 (got {n})"),
            ));
        }
    }
    if let Some(n) = settings.max_new_tokens {
        if !(1..=8192).contains(&n) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("max_new_tokens must be between 1 and 8192 (got {n})"),
            ));
        }
    }
    tokio::fs::create_dir_all(&state.config.paths.local_models_dir)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let path = settings_path(&state);
    let body = serde_json::to_string_pretty(&settings)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    tokio::fs::write(&path, body)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(settings))
}

#[derive(Deserialize)]
pub(crate) struct DownloadBody {
    /// Optional HF revision; defaults to "main".
    #[serde(default)]
    revision: Option<String>,
}

/// Normalize the user's input into a HuggingFace `org/repo` id. Accepts
/// bare ids (`mlx-community/Qwen3-4B-bf16`) and full URLs
/// (`https://huggingface.co/mlx-community/Qwen3-4B-bf16/tree/main`).
fn normalize_hf_id(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty model id".into());
    }
    // Strip URL prefix and any trailing path segments after `org/repo`.
    let stripped = s
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("huggingface.co/")
        .trim_start_matches("hf.co/")
        .trim_end_matches('/');
    let parts: Vec<&str> = stripped.split('/').collect();
    if parts.len() < 2 {
        return Err(format!(
            "expected `org/repo` form, got `{s}`"
        ));
    }
    let org = parts[0];
    let repo = parts[1];
    if org.is_empty() || repo.is_empty() {
        return Err(format!("invalid `org/repo` in `{s}`"));
    }
    // Reject characters that would escape the filesystem path.
    for seg in [org, repo] {
        if seg.contains("..") || seg.contains('\\') {
            return Err(format!("unsafe path segment in `{s}`"));
        }
    }
    Ok(format!("{org}/{repo}"))
}

pub(crate) async fn local_models_download(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    body: Option<Json<DownloadBody>>,
) -> Result<impl IntoResponse, AppError> {
    // Accept any normalized `org/repo` (custom or registry).
    let id = normalize_hf_id(&id)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;

    // Bail if a download for this model is already running.
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

    let revision = body
        .and_then(|b| b.0.revision)
        .unwrap_or_else(|| "main".to_string());
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
    let handle = DownloadHandle {
        state: progress.clone(),
        cancel: cancel.clone(),
    };
    DOWNLOADS.lock().unwrap().insert(id.clone(), handle);

    let id_clone = id.clone();
    tokio::spawn(async move {
        let result = run_download(&id_clone, &revision, &dir, progress.clone(), cancel).await;
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

pub(crate) async fn local_models_status(
    State(_state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let downloads = DOWNLOADS.lock().unwrap();
    let progress = downloads
        .get(&id)
        .map(|h| h.state.lock().unwrap().clone());
    Ok(Json(json!({ "id": id, "download": progress })))
}

pub(crate) async fn local_models_cancel(
    State(_state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let downloads = DOWNLOADS.lock().unwrap();
    if let Some(h) = downloads.get(&id) {
        h.cancel.cancel();
        let mut s = h.state.lock().unwrap();
        s.status = DownloadStatus::Cancelled;
    }
    Ok(Json(json!({ "ok": true })))
}

pub(crate) async fn local_models_delete(
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
    Ok(Json(json!({ "ok": true })))
}

// ---------------------------------------------------------------------------
// Download worker — HuggingFace tree listing + streaming file fetch.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct HfTreeEntry {
    #[serde(rename = "type")]
    entry_type: String,
    path: String,
    #[serde(default)]
    size: u64,
}

const HF_BASE: &str = "https://huggingface.co";

/// Files we skip even when present in the tree (saves bandwidth on docs/images).
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
        || lower.ends_with(".pdf")
}

// ---------------------------------------------------------------------------
// Load / unload — keep engines warm or release them.
// ---------------------------------------------------------------------------

pub(crate) async fn local_models_load(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let cid = canonical_local_model_id(&id);
    let dir = model_dir(&state, &cid);
    if !is_installed(&dir) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            format!("model `{cid}` is not installed"),
        ));
    }

    #[cfg(feature = "local-mlx")]
    {
        let kv_bits = load_settings_blocking(&state.config.paths.local_models_dir).kv_cache_bits;
        // Reuse the same registry entry as ZenCore (`get_or_create_loaded_engine`) so
        // clicking "Load" does not allocate a second copy of weights.
        let engine = get_or_create_loaded_engine(&cid, &dir, kv_bits);
        engine
            .ensure_installed()
            .await
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
        let warm = engine.clone();
        tokio::task::spawn_blocking(move || warm.warm_up())
            .await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }
    #[cfg(not(feature = "local-mlx"))]
    {
        LOADED_ENGINES_STUB.lock().unwrap().insert(cid.clone());
    }

    Ok(Json(json!({ "ok": true, "id": cid, "loaded": true })))
}

pub(crate) async fn local_models_unload(
    State(_state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let cid = canonical_local_model_id(&id);
    #[cfg(feature = "local-mlx")]
    {
        // Two-step unload: explicitly drop cached Model+Tokenizer so MLX
        // frees the unified-memory allocation, THEN release the Arc.
        if let Some(engine) = LOADED_ENGINES.lock().unwrap().remove(&cid) {
            engine.unload();
        }
    }
    #[cfg(not(feature = "local-mlx"))]
    {
        LOADED_ENGINES_STUB.lock().unwrap().remove(&cid);
    }
    Ok(Json(json!({ "ok": true, "id": cid, "loaded": false })))
}

pub(crate) async fn local_models_loaded_list(
    State(_state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(json!({ "loaded": loaded_ids() })))
}

// ---------------------------------------------------------------------------
// "Use in LLM" — create an LLM-config entry pointing at the installed model.
// ---------------------------------------------------------------------------

pub(crate) async fn local_models_use_as_llm(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let dir = model_dir(&state, &id);
    if !is_installed(&dir) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            format!("model `{id}` is not installed"),
        ));
    }

    let known: Option<&KnownModel> = KNOWN_MODELS.iter().find(|m| m.id == id);
    let label = known
        .map(|m| format!("Local {}", m.label))
        .unwrap_or_else(|| format!("Local {id}"));
    let context_length = read_model_context_length_from_dir(&dir)
        .or_else(|| known.map(|m| m.context_length))
        .unwrap_or(DEFAULT_MLX_MAX_PROMPT_TOKENS);

    let cfg_id = format!(
        "llm_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        rand::Rng::gen_range(&mut rand::thread_rng(), 1000u32..9999u32)
    );
    let cfg = LlmConfig {
        id: cfg_id.clone(),
        label,
        provider: "local-mlx".to_string(),
        base_url: String::new(),
        api_key: String::new(),
        model_name: id.clone(),
        adapt: "local-mlx-native".to_string(),
        max_tokens: DEFAULT_MLX_MAX_NEW_TOKENS,
        context_length,
        vision: Some(false),
    };
    save_llm_config(&state.config.paths.global_config_path, &cfg)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Auto-activate when this is the first configured profile.
    let stored = load_llm_configs(&state.config.paths.global_config_path);
    if stored.configs.len() == 1 {
        let _ = set_active_llm_config(&state.config.paths.global_config_path, Some(&cfg_id));
    }

    Ok(Json(json!({ "ok": true, "config": cfg, "active": stored.configs.len() == 1 })))
}

async fn run_download(
    model_id: &str,
    revision: &str,
    dir: &PathBuf,
    progress: Arc<Mutex<DownloadState>>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    progress.lock().unwrap().status = DownloadStatus::Listing;

    // List the repo tree.
    let tree_url = format!("{HF_BASE}/api/models/{model_id}/tree/{revision}?recursive=true");
    let tree: Vec<HfTreeEntry> = client
        .get(&tree_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let files: Vec<HfTreeEntry> = tree
        .into_iter()
        .filter(|e| e.entry_type == "file" && !should_skip(&e.path))
        .collect();

    let total_bytes: u64 = files.iter().map(|f| f.size).sum();
    {
        let mut s = progress.lock().unwrap();
        s.files_total = files.len() as u32;
        s.total_bytes = total_bytes;
        s.status = DownloadStatus::Downloading;
    }

    for entry in files {
        if cancel.is_cancelled() {
            progress.lock().unwrap().status = DownloadStatus::Cancelled;
            return Ok(());
        }
        {
            let mut s = progress.lock().unwrap();
            s.current_file = Some(entry.path.clone());
        }
        let dst = dir.join(&entry.path);
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Skip if already complete (resume support).
        if let Ok(meta) = tokio::fs::metadata(&dst).await {
            if entry.size > 0 && meta.len() == entry.size {
                let mut s = progress.lock().unwrap();
                s.files_done += 1;
                s.downloaded_bytes += entry.size;
                continue;
            }
        }

        let url = format!("{HF_BASE}/{model_id}/resolve/{revision}/{}", entry.path);
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
            let mut s = progress.lock().unwrap();
            s.downloaded_bytes = s.downloaded_bytes.saturating_add(bytes.len() as u64);
        }
        file.flush().await?;
        progress.lock().unwrap().files_done += 1;
    }

    Ok(())
}
