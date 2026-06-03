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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
#[cfg(feature = "local-mlx")]
use crate::local_model::MlxNativeEngine;
use crate::local_model::{read_model_context_length_from_dir, KnownModel, KNOWN_MODELS};
#[cfg(feature = "local-candle")]
use crate::local_model::{CandleEngine, LocalModelRuntime};

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
// Loaded-engine registry — keeps `CandleEngine` instances alive between
// requests so weights stay in memory and are reused across chats.
// Without `local-candle` we still expose the lifecycle (load/unload + a
// `loaded` flag) so the UI stays consistent.
// ---------------------------------------------------------------------------

#[cfg(feature = "local-candle")]
struct CandleSlot {
    engine: Arc<CandleEngine>,
    last_activity: Mutex<Instant>,
    in_flight: AtomicUsize,
}

#[cfg(feature = "local-candle")]
impl CandleSlot {
    fn new(engine: Arc<CandleEngine>) -> Self {
        Self {
            engine,
            last_activity: Mutex::new(Instant::now()),
            in_flight: AtomicUsize::new(0),
        }
    }

    fn touch(&self) {
        if let Ok(mut g) = self.last_activity.lock() {
            *g = Instant::now();
        }
    }

    fn begin_inference(&self) {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        self.touch();
    }

    fn end_inference(&self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
        // Touch on END too — `touch()` at begin alone makes the idle timer
        // start when inference STARTED, so a 50 s turn followed by ~10 s of
        // user reading time would already cross a 60 s idle threshold and
        // trigger a cold reload mid-conversation. Touching at end resets
        // the countdown from the actual last-use moment.
        self.touch();
    }
}

#[cfg(feature = "local-candle")]
static LOADED_ENGINES: Lazy<Mutex<HashMap<String, CandleSlot>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(not(feature = "local-candle"))]
static LOADED_ENGINES_STUB: Lazy<Mutex<std::collections::HashSet<String>>> =
    Lazy::new(|| Mutex::new(std::collections::HashSet::new()));

// ---------------------------------------------------------------------------
// MLX native engine registry — keeps MlxNativeEngine instances alive between
// requests so weights stay in memory and are reused across chats.
// Gated behind `local-mlx` feature (requires Apple Silicon + mlx-rs).
// ---------------------------------------------------------------------------

#[cfg(feature = "local-mlx")]
struct MlxSlot {
    engine: Arc<MlxNativeEngine>,
    last_activity: Mutex<Instant>,
    in_flight: AtomicUsize,
}

#[cfg(feature = "local-mlx")]
impl MlxSlot {
    fn new(engine: Arc<MlxNativeEngine>) -> Self {
        Self {
            engine,
            last_activity: Mutex::new(Instant::now()),
            in_flight: AtomicUsize::new(0),
        }
    }

    fn touch(&self) {
        if let Ok(mut g) = self.last_activity.lock() {
            *g = Instant::now();
        }
    }

    fn begin_inference(&self) {
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        self.touch();
    }

    fn end_inference(&self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
        // Touch on END too — `touch()` at begin alone makes the idle timer
        // start when inference STARTED, so a 50 s turn followed by ~10 s of
        // user reading time would already cross a 60 s idle threshold and
        // trigger a cold reload mid-conversation. Touching at end resets
        // the countdown from the actual last-use moment.
        self.touch();
    }
}

#[cfg(feature = "local-mlx")]
static MLX_ENGINES: Lazy<Mutex<HashMap<String, MlxSlot>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(feature = "local-candle")]
pub(crate) struct CandleInferenceGuard {
    cid: String,
}

#[cfg(feature = "local-candle")]
impl CandleInferenceGuard {
    /// Marks this model id as actively generating until dropped (paired with unload sweeper).
    pub(crate) fn new(model_id: &str) -> Self {
        let cid = canonical_local_model_id(model_id);
        if let Some(slot) = LOADED_ENGINES.lock().unwrap().get(&cid) {
            slot.begin_inference();
        }
        Self { cid }
    }
}

#[cfg(feature = "local-candle")]
impl Drop for CandleInferenceGuard {
    fn drop(&mut self) {
        if let Some(slot) = LOADED_ENGINES.lock().unwrap().get(&self.cid) {
            slot.end_inference();
        }
    }
}

#[cfg(feature = "local-mlx")]
pub(crate) struct MlxInferenceGuard {
    cid: String,
}

#[cfg(feature = "local-mlx")]
impl MlxInferenceGuard {
    pub(crate) fn new(model_id: &str) -> Self {
        let cid = canonical_local_model_id(model_id);
        if let Some(slot) = MLX_ENGINES.lock().unwrap().get(&cid) {
            slot.begin_inference();
        }
        Self { cid }
    }
}

#[cfg(feature = "local-mlx")]
impl Drop for MlxInferenceGuard {
    fn drop(&mut self) {
        if let Some(slot) = MLX_ENGINES.lock().unwrap().get(&self.cid) {
            slot.end_inference();
        }
    }
}

#[cfg(feature = "local-mlx")]
pub fn get_or_create_mlx_engine(id: &str, model_dir: &std::path::Path) -> Arc<MlxNativeEngine> {
    let cid = canonical_local_model_id(id);
    let settings_dir = model_dir.parent().unwrap_or(model_dir);
    let kv_bits = load_settings_blocking(settings_dir)
        .kv_cache_bits
        .filter(|b| *b > 0);
    let mut map = MLX_ENGINES.lock().unwrap();
    if let Some(existing) = map.get(&cid) {
        if existing.engine.kv_cache_bits() == kv_bits {
            return existing.engine.clone();
        }
        let old = map.remove(&cid).expect("mlx slot present");
        old.engine.unload();
    }
    let engine = Arc::new(MlxNativeEngine::new(model_dir, &cid, kv_bits));
    map.insert(cid, MlxSlot::new(engine.clone()));
    engine
}

#[cfg(feature = "local-mlx")]
pub fn get_mlx_engine(id: &str) -> Option<Arc<MlxNativeEngine>> {
    let cid = canonical_local_model_id(id);
    MLX_ENGINES
        .lock()
        .unwrap()
        .get(&cid)
        .map(|s| s.engine.clone())
}

/// Periodic task: unload in-memory weights when [`LocalModelSettings::idle_unload_secs`] elapses since last activity.
///
/// Started from [`run_daemon`](crate::run_daemon) when **`local-candle`** and/or **`local-mlx`** is enabled.
#[cfg(any(feature = "local-candle", feature = "local-mlx"))]
pub fn spawn_idle_unload_worker(models_root: PathBuf) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(15));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            let settings = load_settings_blocking(&models_root);
            let secs = settings
                .idle_unload_secs
                .unwrap_or(DEFAULT_IDLE_UNLOAD_SECS);
            if secs == 0 {
                continue;
            }
            sweep_idle_models(Duration::from_secs(secs as u64));
        }
    });
}

#[cfg(any(feature = "local-candle", feature = "local-mlx"))]
fn sweep_idle_models(idle_after: Duration) {
    #[cfg(feature = "local-candle")]
    {
        let cids: Vec<String> = LOADED_ENGINES.lock().unwrap().keys().cloned().collect();
        for cid in cids {
            let mut map = LOADED_ENGINES.lock().unwrap();
            let Some(slot) = map.get(&cid) else {
                continue;
            };
            if slot.in_flight.load(Ordering::Acquire) > 0 {
                continue;
            }
            if !slot.engine.is_loaded() {
                continue;
            }
            let last_ok = slot.last_activity.lock().ok().map(|g| *g);
            let Some(last) = last_ok else {
                continue;
            };
            if last.elapsed() < idle_after {
                continue;
            }
            tracing::info!(
                "[local-models] idle unload (Candle) — `{cid}` after {:?} inactivity",
                idle_after
            );
            let slot = map.remove(&cid).expect("cid present");
            drop(map);
            slot.engine.unload();
        }
    }
    #[cfg(feature = "local-mlx")]
    {
        let cids: Vec<String> = MLX_ENGINES.lock().unwrap().keys().cloned().collect();
        for cid in cids {
            let mut map = MLX_ENGINES.lock().unwrap();
            let Some(slot) = map.get(&cid) else {
                continue;
            };
            if slot.in_flight.load(Ordering::Acquire) > 0 {
                continue;
            }
            if !slot.engine.is_loaded() {
                continue;
            }
            let last_ok = slot.last_activity.lock().ok().map(|g| *g);
            let last: Instant = match last_ok {
                Some(v) => v,
                None => continue,
            };
            if last.elapsed() < idle_after {
                continue;
            }
            tracing::info!(
                "[local-models] idle unload (MLX native) — `{cid}` after {:?} inactivity",
                idle_after
            );
            let slot = map.remove(&cid).expect("cid present");
            drop(map);
            slot.engine.unload();
        }
    }
}

fn loaded_ids() -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    #[cfg(feature = "local-candle")]
    {
        let map = LOADED_ENGINES.lock().unwrap();
        ids.extend(
            map.iter()
                .filter(|(_, slot)| slot.engine.is_loaded())
                .map(|(k, _)| k.clone()),
        );
    }
    #[cfg(not(feature = "local-candle"))]
    {
        ids.extend(LOADED_ENGINES_STUB.lock().unwrap().iter().cloned());
    }
    #[cfg(feature = "local-mlx")]
    {
        let mlx_ids: Vec<String> = MLX_ENGINES
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, slot)| slot.engine.is_loaded())
            .map(|(k, _)| k.clone())
            .collect();
        ids.extend(mlx_ids);
    }
    ids.sort();
    ids.dedup();
    ids
}

fn is_loaded(id: &str) -> bool {
    let cid = canonical_local_model_id(id);
    #[cfg(feature = "local-candle")]
    {
        if LOADED_ENGINES
            .lock()
            .unwrap()
            .get(&cid)
            .is_some_and(|slot| slot.engine.is_loaded())
        {
            return true;
        }
    }
    #[cfg(not(feature = "local-candle"))]
    {
        if LOADED_ENGINES_STUB.lock().unwrap().contains(&cid) {
            return true;
        }
    }
    #[cfg(feature = "local-mlx")]
    {
        if MLX_ENGINES
            .lock()
            .unwrap()
            .get(&cid)
            .is_some_and(|slot| slot.engine.is_loaded())
        {
            return true;
        }
    }
    false
}

/// Public accessor for `query_llm.rs` to reuse a loaded engine.
/// Returns `None` when the model isn't currently loaded.
#[cfg(feature = "local-candle")]
pub fn get_loaded_engine(id: &str) -> Option<Arc<CandleEngine>> {
    let cid = canonical_local_model_id(id);
    LOADED_ENGINES
        .lock()
        .unwrap()
        .get(&cid)
        .map(|slot| slot.engine.clone())
}

/// Normalize HuggingFace-style model id for registry keys (trim whitespace).
#[must_use]
pub fn canonical_local_model_id(id: &str) -> String {
    id.trim().to_string()
}

/// Return the cached engine for `id`, or create and cache a new one.
/// Weights are loaded lazily on the first `generate_stream` call.
#[cfg(feature = "local-candle")]
pub fn get_or_create_loaded_engine(id: &str, model_dir: &std::path::Path) -> Arc<CandleEngine> {
    let cid = canonical_local_model_id(id);
    let mut map = LOADED_ENGINES.lock().unwrap();
    if let Some(existing) = map.get(&cid) {
        return existing.engine.clone();
    }
    let engine = Arc::new(CandleEngine::new(model_dir, &cid));
    map.insert(cid, CandleSlot::new(engine.clone()));
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
    /// True when the model supports image/vision inputs.
    vision: bool,
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
                vision: m.vision,
            }
        })
        .collect();

    // Discover custom installs not in the registry.
    let known: std::collections::HashSet<&'static str> =
        KNOWN_MODELS.iter().map(|m| m.id).collect();
    let mut discovered: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Ok(mut rd) = tokio::fs::read_dir(&state.config.paths.local_models_dir).await {
        while let Ok(Some(ent)) = rd.next_entry().await {
            let Ok(ft) = ent.file_type().await else {
                continue;
            };
            if !ft.is_dir() {
                continue;
            }
            let name = ent.file_name().to_string_lossy().into_owned();
            let Some(id) = dirname_to_id(&name) else {
                continue;
            };
            if known.contains(id.as_str()) {
                continue;
            }
            if is_whisper_model_dir(&ent.path()) {
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
        if is_whisper_model_dir(&dir) {
            continue;
        }
        let loaded = is_loaded(&id);
        let installed = is_installed(&dir);
        let context_length = if installed {
            read_model_context_length_from_dir(&dir).unwrap_or(DEFAULT_MLX_MAX_PROMPT_TOKENS)
        } else {
            0
        };
        entries.push(ModelEntry {
            label: format!("{} (custom)", id),
            native_supported: id.to_lowercase().contains("qwen3")
                || id.to_lowercase().contains("qwen2"),
            installed,
            on_disk_path: dir.to_string_lossy().to_string(),
            download: download_snapshot.get(&id).cloned(),
            id: id.clone(),
            approx_size_gb: 0.0,
            context_length,
            custom: true,
            loaded,
            vision: crate::local_model::models::infer_vision_from_id(&id),
        });
    }

    Ok(Json(json!({ "models": entries })))
}

pub(crate) async fn local_models_runtime(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let candle_feature = cfg!(feature = "local-candle");
    let metal_feature = cfg!(feature = "local-candle-metal");
    Ok(Json(json!({
        "feature_local_candle": candle_feature,
        "feature_metal": metal_feature,
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

/// Default prompt length cap for the **Candle** CPU/Metal backend.
///
/// Much lower than the MLX default because:
/// - Tools are **never** forwarded to local Candle models (they can't use them).
/// - After tool stripping a typical system-prompt + chat history is ~200–500 tokens.
/// - CPU prefill is O(seq_len) per layer; keeping ≤ 512 tokens ensures prefill
///   completes in < 60 s even in unoptimized debug builds.
///
/// Raise via `max_prompt_tokens` in `settings.json` for longer conversations
/// (or build with `--release` where this is 10–15× faster).
pub const DEFAULT_CANDLE_MAX_PROMPT_TOKENS: u32 = 512;

/// Default **generation** token cap for the Candle CPU/Metal backend.
///
/// CPU decode throughput on a small model (0.6B–4B) is typically 3–15 tok/s, so
/// 512 tokens completes within ~30–170 s even on slow hardware.  Raise via
/// `max_new_tokens` in `settings.json` for longer outputs (beware latency).
pub const DEFAULT_CANDLE_MAX_NEW_TOKENS: u32 = 512;

/// Default KV-cache window (tokens) — shared between the MLX (GPU/Metal) and
/// Candle (CPU) engines. Both back-ends read this value through the same
/// `max_kv_tokens` field in `settings.json`; tune there per deployment.
///
/// Sized to fit the full MCP tool list (~120 tokens / tool × ~120 tools ≈
/// 14 K tokens) **plus** the ~2 K-token decode reserve, plus headroom for
/// chat history. Below this the trim loop has to drop a handful of tools
/// from the end of the list.
///
/// Memory cost reference:
/// - Qwen3-4B BF16 KV (36 layers × 8 KV heads × head 128): `≈ 2.9 GB` @ 20 K.
/// - Candle CPU users on low-RAM devices should drop this in `settings.json`
///   (attention is O(L²) on CPU, so wider window also costs prefill time).
pub const DEFAULT_KV_WINDOW_TOKENS: u32 = 20_480;

/// When TurboQuant KV is active, `mlx_native` caps **decode** at this (below API 8192)
/// to cut RAM / CPU on the slow CPU attention path; raise via `max_new_tokens` in JSON only up to this cap.
pub const TURBOQUANT_MAX_NEW_TOKENS_CAP: u32 = 2048;

/// Hard ceiling on **prompt + decode** token positions when TurboQuant KV is active
/// (`kv_cache_bits` set, build with `local-mlx-turboquant`). Native MLX clamps
/// `max_prompt_tokens` so `max_prompt_tokens + max_new_tokens ≤` this value.
pub const TURBOQUANT_MAX_CONTEXT_TOKENS: u32 = 128_000 + 8192;

/// Idle auto-unload: when **`idle_unload_secs`** is **`None`** (field omitted
/// / JSON **`null`**), sweep uses this default (**300 s = 5 min**).
///
/// Why 5 min, not the old 60 s default:
/// - In normal multi-turn chat the user pauses 30 s – 3 min between messages
///   (read reply → think → type next prompt). At 60 s the model unloads
///   during nearly every pause, forcing a ~1 s reload **and** a cold
///   prefill (15 K-token tools-heavy prompts re-prefilling take ~30 s).
/// - Matches `prefix_cache::IDLE_TTL_SECS = 300 s` so the prefix cache TTL
///   actually fires before the model itself unloads. Pre-fix the prefix
///   cache was effectively useless after pauses because the model unload
///   dropped the whole `Loaded` (including the cache) at 60 s.
/// - On Apple Silicon unified memory, holding a 4-bit Qwen3-4B (~2.3 GB)
///   for an extra few minutes is cheap; the prefill saving on the next
///   resume (~30 s) is much more valuable.
///
/// Override via `idle_unload_secs` in `settings.json` for RAM-tight setups.
pub const DEFAULT_IDLE_UNLOAD_SECS: u32 = 300;

/// When `kv_cache_bits` is `2` (invalid for turboquant-rs QJL path), native MLX maps to this **TQ3** total bit budget (`3`).
pub const DEFAULT_TURBOQUANT_KV_TOTAL_BITS: u8 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalModelSettings {
    /// TurboQuant KV total bit budget: **`3` = TQ3** (default remap), **`4` = TQ4**. **`2`** is accepted in JSON for compatibility but maps to [`DEFAULT_TURBOQUANT_KV_TOTAL_BITS`] (TQ3) at runtime. **`0`** disables TurboQuant (FP16 KV), same as `None` — the UI "Off" option sends `0`.
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
    /// Sampling temperature. `Some(0.0)` keeps greedy argmax (`argmax`).
    ///
    /// `None` defaults to **`0`** for transformer families that usually run greedy (Qwen3, Llama);
    /// for **Gemma‑3**, `None` → **0.65** to avoid greedy repetition loops with small checkpoints.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// HF-style repetition penalty on recent token ids (>1 activates). **`1.0` = disabled.**
    ///
    /// `None` defaults to **`1.15`** for Gemma‑3 and **`1.0`** for other architectures on native MLX.
    #[serde(default)]
    pub repetition_penalty: Option<f32>,
    /// Pass `enable_thinking` to the chat template. `None` = let the template decide (model
    /// default). `false` = disable thinking (Qwen3 pre-fills `<think>\n\n</think>` to skip
    /// the reasoning block). Defaults to `false` to avoid unbounded thinking overhead.
    #[serde(default = "default_enable_thinking")]
    pub enable_thinking: Option<bool>,
    /// Context length (tokens already in KV cache) at which TurboQuant quantization activates.
    /// `0` = quantize immediately (from first decode step). `None` → default 2048.
    #[serde(default)]
    pub tq_activate_at: Option<u32>,
    /// Maximum number of KV-cache tokens kept in memory per layer (Candle and MLX).
    ///
    /// When the rolling window is full, the **oldest** tokens are evicted so memory
    /// stays bounded.  RoPE positions remain absolute — quality is preserved for
    /// the retained context window.
    ///
    /// `None` → [`DEFAULT_KV_WINDOW_TOKENS`] (16 384).
    /// Range: 128 – 262 144.
    #[serde(default)]
    pub max_kv_tokens: Option<u32>,
    /// MLX native packed KV on Metal (`mlx.core.quantize` + `quantized_matmul`).
    /// `None` or `0` → FP16 [`ConcatKeyValueCache`]. `4` or `8` → packed KV (saves RAM).
    /// Distinct from [`Self::kv_cache_bits`] (future turboquant-rs CPU path).
    #[serde(default)]
    pub mlx_kv_cache_bits: Option<u8>,
    /// Preferred inference backend for this machine.
    ///
    /// `"mlx"` → in-process mlx-rs (Apple Silicon, ~60–100 tok/s on M4 Pro).
    /// `"candle"` → Candle CPU+Accelerate (~12 tok/s) or Metal (~7 tok/s).
    /// `None` → auto-detect: `"mlx"` when the `local-mlx` feature is compiled in, else `"candle"`.
    #[serde(default)]
    pub preferred_backend: Option<String>,
    /// Unload cached local weights (**Candle** / **MLX native**) after this many seconds without use.
    /// **`Some(0)`** = disabled. **`None`** (omitted / JSON **`null`**) uses [`DEFAULT_IDLE_UNLOAD_SECS`].
    /// Timer resets on each inference and on explicit **Load** in the UI.
    #[serde(default)]
    pub idle_unload_secs: Option<u32>,
    /// **MLX native** memory-pressure guard: before a turn's prefill, if the
    /// process RSS exceeds this many MiB, drop the prefix-cache KV (weights stay
    /// loaded) and return MLX's pool to the OS. Trades the next turn's
    /// prefix-cache hit for a bounded footprint. **`None`** / **`0`** = disabled
    /// (the default — KV is only bounded by the prefix cache's own TTL / byte
    /// budget). Set to your RAM budget for this process (must exceed the model
    /// weight size, else KV is released every turn).
    #[serde(default)]
    pub kv_release_rss_mib: Option<u32>,
    /// **MLX native** session-end cleanup: when `true`, release the model's
    /// prefix-cache + KV (and return MLX's pool to the OS) as soon as a chat
    /// session finishes — i.e. the agentic loop ends with a final answer that
    /// makes no tool calls. Weights stay loaded (next session is still warm),
    /// so this only drops the per-session KV (~hundreds of MB) immediately
    /// instead of waiting for the prefix cache's idle TTL or `idle_unload_secs`.
    /// The within-session prefix cache (tool-call turns) is untouched — release
    /// happens only on the terminal, tool-call-free turn. **`None`** / **`false`**
    /// = disabled (default; KV persists until idle eviction). Best for RAM-tight
    /// machines or when switching between models / long idle gaps are common.
    #[serde(default)]
    pub release_cache_after_session: Option<bool>,
}

impl Default for LocalModelSettings {
    fn default() -> Self {
        Self {
            kv_cache_bits: None,
            max_prompt_tokens: None,
            max_new_tokens: None,
            temperature: None,
            repetition_penalty: None,
            enable_thinking: default_enable_thinking(),
            tq_activate_at: None,
            max_kv_tokens: None,
            mlx_kv_cache_bits: None,
            preferred_backend: None,
            idle_unload_secs: Some(DEFAULT_IDLE_UNLOAD_SECS),
            kv_release_rss_mib: None,
            release_cache_after_session: None,
        }
    }
}

fn default_enable_thinking() -> Option<bool> {
    Some(false)
}

fn settings_path(state: &UiState) -> PathBuf {
    state.config.paths.local_models_dir.join("settings.json")
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
        if !matches!(bits, 0 | 2 | 3 | 4) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("kv_cache_bits must be 0, 2, 3, or 4 (got {bits}); 0 disables TurboQuant (FP16 KV), 2 is remapped to TQ3 at runtime when using turboquant"),
            ));
        }
    }
    if let Some(bits) = settings.mlx_kv_cache_bits {
        if !matches!(bits, 0 | 4 | 8) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("mlx_kv_cache_bits must be 0, 4, or 8 (got {bits})"),
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
    if let Some(t) = settings.temperature {
        if !(0.0..=4.0).contains(&t) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("temperature must be between 0 and 4 (got {t})"),
            ));
        }
    }
    if let Some(p) = settings.repetition_penalty {
        if !(1.0..=2.0).contains(&p) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("repetition_penalty must be between 1 and 2 (got {p})"),
            ));
        }
    }
    if let Some(n) = settings.max_kv_tokens {
        if !(128..=262_144).contains(&n) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("max_kv_tokens must be between 128 and 262144 (got {n})"),
            ));
        }
    }
    if let Some(ref b) = settings.preferred_backend {
        if !matches!(b.as_str(), "mlx" | "candle") {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("preferred_backend must be \"mlx\" or \"candle\" (got \"{b}\")"),
            ));
        }
    }
    if let Some(n) = settings.idle_unload_secs {
        if n > 0 && n < 60 {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("idle_unload_secs must be 0 (disable) or at least 60 (got {n})"),
            ));
        }
        if n > 604_800 {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("idle_unload_secs must not exceed 604800 (7 days); got {n}"),
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
        return Err(format!("expected `org/repo` form, got `{s}`"));
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
    let id = normalize_hf_id(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;

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
    let progress = downloads.get(&id).map(|h| h.state.lock().unwrap().clone());
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

    #[cfg(feature = "local-candle")]
    {
        // Reuse the same registry entry as ZenCore so clicking "Load"
        // does not allocate a second copy of weights in memory.
        let engine = get_or_create_loaded_engine(&cid, &dir);
        engine
            .ensure_installed()
            .await
            .map_err(|e| AppError(StatusCode::BAD_REQUEST, e.to_string()))?;
        let warm = engine.clone();
        tokio::task::spawn_blocking(move || warm.warm_up())
            .await
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if let Some(slot) = LOADED_ENGINES.lock().unwrap().get(&cid) {
            slot.touch();
        }
    }
    #[cfg(not(feature = "local-candle"))]
    {
        LOADED_ENGINES_STUB.lock().unwrap().insert(cid.clone());
    }

    Ok(Json(json!({ "ok": true, "id": cid, "loaded": true })))
}

#[allow(unused_mut)]
pub(crate) async fn local_models_unload(
    State(_state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let cid = canonical_local_model_id(&id);
    let mut unloaded_any = false;
    #[cfg(feature = "local-candle")]
    {
        if let Some(slot) = LOADED_ENGINES.lock().unwrap().remove(&cid) {
            slot.engine.unload();
            unloaded_any = true;
        }
    }
    #[cfg(not(feature = "local-candle"))]
    {
        LOADED_ENGINES_STUB.lock().unwrap().remove(&cid);
    }
    // Also unload any MLX native engine for this model.
    #[cfg(feature = "local-mlx")]
    {
        if let Some(slot) = MLX_ENGINES.lock().unwrap().remove(&cid) {
            slot.engine.unload();
            unloaded_any = true;
        }
    }
    let _ = unloaded_any;
    Ok(Json(json!({ "ok": true, "id": cid, "loaded": false })))
}

/// Unload **all** in-memory engines (both Candle and MLX) and return
/// approximate freed bytes. Use this when the user wants to reclaim RAM
/// without waiting for `idle_unload_secs` to elapse.
#[allow(unused_mut)]
pub(crate) async fn local_models_unload_all(
    State(_state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let mut unloaded: Vec<String> = Vec::new();
    #[cfg(feature = "local-candle")]
    {
        let cids: Vec<String> = LOADED_ENGINES.lock().unwrap().keys().cloned().collect();
        for cid in cids {
            if let Some(slot) = LOADED_ENGINES.lock().unwrap().remove(&cid) {
                slot.engine.unload();
                unloaded.push(cid);
            }
        }
    }
    #[cfg(feature = "local-mlx")]
    {
        let cids: Vec<String> = MLX_ENGINES.lock().unwrap().keys().cloned().collect();
        for cid in cids {
            if let Some(slot) = MLX_ENGINES.lock().unwrap().remove(&cid) {
                slot.engine.unload();
                unloaded.push(cid);
            }
        }
    }
    tracing::info!(
        "[local-models] manual unload-all: dropped {} engine(s): {:?}",
        unloaded.len(),
        unloaded,
    );
    Ok(Json(json!({
        "ok": true,
        "unloaded": unloaded,
        "count": unloaded.len(),
    })))
}

/// Load a model using the MLX native backend (in-process mlx-rs inference).
pub(crate) async fn local_models_load_mlx(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    #[cfg(not(feature = "local-mlx"))]
    {
        let _ = (state, id);
        return Err(AppError(
            StatusCode::NOT_IMPLEMENTED,
            "local-mlx feature not enabled — rebuild with `cargo build --features local-mlx`"
                .to_string(),
        ));
    }

    #[cfg(feature = "local-mlx")]
    {
        let cid = canonical_local_model_id(&id);
        let dir = model_dir(&state, &cid);
        if !is_installed(&dir) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                format!("model `{cid}` is not installed"),
            ));
        }
        let engine = get_or_create_mlx_engine(&cid, &dir);
        // warm_up() is synchronous (blocks while loading weights); run on blocking thread.
        tokio::task::spawn_blocking(move || engine.warm_up())
            .await
            .map_err(|e| {
                AppError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("task panic: {e}"),
                )
            })?
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if let Some(slot) = MLX_ENGINES.lock().unwrap().get(&cid) {
            slot.touch();
        }
        Ok(Json(json!({
            "ok": true,
            "id": cid,
            "loaded": true,
            "backend": "local-mlx",
        })))
    }

    // Unreachable when local-mlx is enabled, but satisfies the return type when it's not.
    #[cfg(not(feature = "local-mlx"))]
    #[allow(unreachable_code)]
    Ok(Json(json!({ "ok": false })))
}

pub(crate) async fn local_models_loaded_list(
    State(_state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(json!({ "loaded": loaded_ids() })))
}

// ---------------------------------------------------------------------------
// "Use in LLM" — create an LLM-config entry pointing at the installed model.
// Optionally accepts `?backend=mlx` to create an mlx-lm sidecar config
// instead of the default Candle in-process config.
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize, Default)]
pub(crate) struct UseAsLlmQuery {
    /// `"mlx"` → mlx-lm sidecar (OpenAI adapter, ~60–100 tok/s on M4 Pro)
    /// `"candle"` or omitted → Candle in-process (~12 tok/s on M4 Pro with Accelerate)
    pub backend: Option<String>,
}

pub(crate) async fn local_models_use_as_llm(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    axum::extract::Query(query): axum::extract::Query<UseAsLlmQuery>,
) -> Result<impl IntoResponse, AppError> {
    let dir = model_dir(&state, &id);
    if !is_installed(&dir) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            format!("model `{id}` is not installed"),
        ));
    }

    // Resolve backend: explicit query param > settings.preferred_backend > auto-detect.
    let backend_choice = query.backend.clone().or_else(|| {
        let s = load_settings_blocking(&state.config.paths.local_models_dir);
        s.preferred_backend.clone()
    });

    #[cfg(feature = "local-mlx")]
    let mlx_available = true;
    #[cfg(not(feature = "local-mlx"))]
    let mlx_available = false;

    let use_mlx = match backend_choice.as_deref() {
        Some("mlx") => true,
        Some("candle") => false,
        // No preference: default to MLX when the feature is compiled in.
        _ => mlx_available,
    };

    let provider = if use_mlx { "local-mlx" } else { "local-candle" };

    // Dedup: "Use in LLM" should add a model to the LLM profiles only once.
    // If a profile for this exact (provider, model) pair already exists,
    // return it instead of creating a duplicate.
    let existing = load_llm_configs(&state.config.paths.global_config_path);
    if let Some(found) = existing
        .configs
        .iter()
        .find(|c| c.provider == provider && c.model_name == id)
    {
        let is_active = existing.active_id.as_deref() == Some(found.id.as_str());
        return Ok(Json(json!({
            "ok": true,
            "config": found,
            "active": is_active,
            "existed": true,
        })));
    }

    let known: Option<&KnownModel> = KNOWN_MODELS.iter().find(|m| m.id == id);
    let label = if use_mlx {
        known
            .map(|m| format!("Local {} (MLX)", m.label))
            .unwrap_or_else(|| format!("Local {id} (MLX)"))
    } else {
        known
            .map(|m| format!("Local {}", m.label))
            .unwrap_or_else(|| format!("Local {id}"))
    };
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

    let cfg = if use_mlx {
        // MLX config: the mlx_lm.server will be auto-started on first use.
        // base_url is determined at runtime by the MlxEngine; leave it blank
        // here and let query_llm.rs look up the running engine's port.
        LlmConfig {
            id: cfg_id.clone(),
            label,
            provider: provider.to_string(),
            base_url: String::new(), // filled in dynamically by the adapter
            api_key: String::new(),
            model_name: id.clone(),
            adapt: "local-mlx".to_string(),
            max_tokens: DEFAULT_MLX_MAX_NEW_TOKENS,
            context_length,
            vision: Some(false),
        }
    } else {
        LlmConfig {
            id: cfg_id.clone(),
            label,
            provider: provider.to_string(),
            base_url: String::new(),
            api_key: String::new(),
            model_name: id.clone(),
            adapt: "local-candle-native".to_string(),
            max_tokens: DEFAULT_CANDLE_MAX_NEW_TOKENS,
            context_length,
            vision: Some(false),
        }
    };
    save_llm_config(&state.config.paths.global_config_path, &cfg)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Auto-activate when this is the first configured profile.
    let stored = load_llm_configs(&state.config.paths.global_config_path);
    if stored.configs.len() == 1 {
        let _ = set_active_llm_config(&state.config.paths.global_config_path, Some(&cfg_id));
    }

    Ok(Json(
        json!({ "ok": true, "config": cfg, "active": stored.configs.len() == 1, "existed": false }),
    ))
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
