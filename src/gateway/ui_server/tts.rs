//! TTS (Text-to-Speech) model management API.
//!
//! Endpoints (all under `/api/tts`):
//!   GET    /api/tts/models                  — catalog + install/download status
//!   POST   /api/tts/models/:id/download     — HuggingFace download (background)
//!   GET    /api/tts/models/:id/status       — poll download progress
//!   POST   /api/tts/models/:id/cancel       — cancel in-flight download
//!   DELETE /api/tts/models/:id              — remove model dir
//!   GET    /api/tts/settings                — { model_id, voice, speed, language }
//!   PUT    /api/tts/settings                — persist selection
//!   POST   /api/tts/synthesize              — JSON { text, language?, voice?, speed? } → WAV bytes
//!
//! Synthesis is **pure Rust** — no Python, no external runtimes. The actual
//! backends live in [`crate::tts`]; this module's [`synthesize_blocking`] is a
//! thin wrapper that adapts the trait's error type to the HTTP layer's
//! `(StatusCode, String)` convention.
//!
//! Download follows the same composite HF pattern as `whisper.rs`: tree API →
//! stream each file into the model dir with resume support.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Json, Response},
};
use futures::StreamExt;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

use crate::gateway::group_manager::{load_tts_settings, save_tts_settings, TtsSettings};

use super::core::{AppError, UiState};

const HF_BASE: &str = "https://huggingface.co";

// ── Catalog ──────────────────────────────────────────────────────────────────

struct TtsCatalogEntry {
    /// Public HuggingFace repo id (also the weights repo).
    id: &'static str,
    label: &'static str,
    approx_size_gb: f32,
    /// Supported language codes.
    languages: &'static [&'static str],
    default_language: &'static str,
    /// Short description shown in the UI.
    description: &'static str,
}

static CATALOG: &[TtsCatalogEntry] = &[
    TtsCatalogEntry {
        id: "macos-speech",
        label: "System Speech (macOS) — Vietnamese (Linh)",
        approx_size_gb: 0.0,
        languages: &["vi"],
        default_language: "vi",
        description: "Zero-dependency macOS native voice. Vietnamese (Linh).",
    },
    TtsCatalogEntry {
        id: "macos-speech-en",
        label: "System Speech (macOS) — English (Samantha)",
        approx_size_gb: 0.0,
        languages: &["en"],
        default_language: "en",
        description: "Zero-dependency macOS native voice. English (Samantha).",
    },
    TtsCatalogEntry {
        id: "facebook/mms-tts-vie",
        label: "MMS-VITS Vietnamese (Meta)",
        approx_size_gb: 0.3,
        languages: &["vi"],
        default_language: "vi",
        description: "Meta Massively Multilingual Speech VITS (Vietnamese) — native pure-Rust MLX synthesis, no Python. Download once (~290 MB), then speaks Vietnamese fully offline. Requires a daemon built with the local-mlx-tts feature; otherwise falls back to macOS voice (X-TTS-Fallback header).",
    },
    // NOTE: community finetunes like dvd1503/mms-tts-vie-finetuned are no
    // longer pinned in the catalog — any HF VitsModel repo still installs via
    // "Add model from Hugging Face" (generic VitsModel routing).
    TtsCatalogEntry {
        id: "facebook/mms-tts-eng",
        label: "MMS-VITS English (Meta)",
        approx_size_gb: 0.3,
        languages: &["en"],
        default_language: "en",
        description: "Meta MMS VITS English — native pure-Rust MLX synthesis, fully offline after download. Same runtime as the Vietnamese model.",
    },
    TtsCatalogEntry {
        id: "pnnbao-ump/VieNeu-TTS-v3-Turbo",
        label: "VieNeu-TTS v3 Turbo (48 kHz, 14 giọng)",
        approx_size_gb: 0.31,
        languages: &["vi", "en"],
        default_language: "vi",
        description: "VieNeu-TTS v3 Turbo (Phạm Nguyễn Ngọc Bảo) — 48 kHz, 14 preset Vietnamese voices, En–Vi code-switching, emotion cues ([cười], [thở dài]). Runs the official ONNX path on CPU (daemon built with tts-vieneu). Set the Voice field to a preset name (default: Phạm Tuyên). Composite download: ONNX graphs + MOSS codec + voices + phoneme dictionary.",
    },
    // NOTE: ZipVoice (mlx-community/zipvoice-vietnamese) was dropped from the
    // catalog — the pure-Rust port is still WIP (never synthesized; always
    // fell back to the macOS voice) and VieNeu now covers high-quality
    // Vietnamese. The `crate::tts::zipvoice` port work remains for when the
    // synthesis path lands.
];

fn catalog_get(id: &str) -> Option<&'static TtsCatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// Selectable voices for a model, if it exposes any: `(voices, default)`.
/// VieNeu reads its preset list from the downloaded `voices_v3_turbo.json`
/// (name + description + gender per voice); the macOS presets are static.
fn model_voices(id: &str, dir: &std::path::Path) -> (Vec<serde_json::Value>, Option<String>) {
    match id {
        "macos-speech" => (
            vec![json!({"name": "Linh", "description": "Giọng nữ tiếng Việt (macOS)"})],
            Some("Linh".to_string()),
        ),
        "macos-speech-en" => (
            vec![json!({"name": "Samantha", "description": "English female (macOS)"})],
            Some("Samantha".to_string()),
        ),
        _ if id == crate::tts::vieneu::MODEL_ID => {
            let Ok(s) = std::fs::read_to_string(dir.join("voices_v3_turbo.json")) else {
                return (Vec::new(), None);
            };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) else {
                return (Vec::new(), None);
            };
            let default_voice = v["default_voice"].as_str().map(str::to_string);
            let mut voices: Vec<serde_json::Value> = v["presets"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .map(|(name, p)| {
                            json!({
                                "name": name,
                                "description": p["description"].as_str().unwrap_or(""),
                                "gender": p["gender"].as_str().unwrap_or(""),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            voices.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            (voices, default_voice)
        }
        _ => (Vec::new(), None),
    }
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
    state.config.paths.tts_models_dir.join(safe_dirname(id))
}

/// A TTS model is considered installed if it is a built-in system voice
/// (any `macos-speech*` preset) or if the directory contains weights.
fn is_installed(state: &UiState, id: &str) -> bool {
    if id.starts_with("macos-speech") {
        return true;
    }
    let dir = model_dir(state, id);
    if id == crate::tts::vieneu::MODEL_ID {
        return crate::tts::vieneu::dir_is_installed(&dir);
    }
    dir.join("config.json").exists()
        && (dir.join("model.safetensors").exists()
            || dir.join("weights.npz").exists()
            || dir.join("model.npz").exists()
            || dir.join("model.safetensors.index.json").exists())
}

// ── Download progress ─────────────────────────────────────────────────────────

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

// ── Routes: model listing ─────────────────────────────────────────────────────

pub(crate) async fn tts_models_list(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let downloads = DOWNLOADS.lock().unwrap();
    let mut models = Vec::new();

    // Catalog entries first.
    for e in CATALOG {
        let dir = model_dir(&state, e.id);
        let download = downloads.get(e.id).map(|h| h.state.lock().unwrap().clone());
        let (voices, default_voice) = model_voices(e.id, &dir);
        models.push(json!({
            "id": e.id,
            "label": e.label,
            "approx_size_gb": e.approx_size_gb,
            "languages": e.languages,
            "default_language": e.default_language,
            "description": e.description,
            "installed": is_installed(&state, e.id),
            "on_disk_path": dir.to_string_lossy(),
            "custom": false,
            "download": download,
            "voices": voices,
            "default_voice": default_voice,
        }));
    }

    // Discover custom installs in tts_models_dir not in catalog.
    if let Ok(entries) = std::fs::read_dir(&state.config.paths.tts_models_dir) {
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
            if is_installed(&state, &id) || download.is_some() {
                models.push(json!({
                    "id": id,
                    "label": format!("TTS custom ({id})"),
                    "approx_size_gb": 0.0,
                    "languages": ["vi", "en"],
                    "default_language": "vi",
                    "description": "",
                    "installed": is_installed(&state, &id),
                    "on_disk_path": dir.to_string_lossy(),
                    "custom": true,
                    "download": download,
                }));
            }
        }
    }

    // Append in-flight downloads not yet on disk.
    for (id, handle) in downloads.iter() {
        if catalog_get(id).is_some() || models.iter().any(|m| m["id"] == *id) {
            continue;
        }
        let dir = model_dir(&state, id);
        models.push(json!({
            "id": id,
            "label": format!("TTS custom ({id})"),
            "approx_size_gb": 0.0,
            "languages": ["vi", "en"],
            "default_language": "vi",
            "description": "",
            "installed": is_installed(&state, id),
            "on_disk_path": dir.to_string_lossy(),
            "custom": true,
            "download": handle.state.lock().unwrap().clone(),
        }));
    }

    Ok(Json(json!({ "models": models })))
}

// ── Routes: download ──────────────────────────────────────────────────────────

/// Normalize a HuggingFace `org/repo` id from bare id or full URL.
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

pub(crate) async fn tts_download(
    State(state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let id = normalize_hf_id(&id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;

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
        let result = if weights_repo == crate::tts::vieneu::MODEL_ID {
            run_vieneu_download(&dir, progress.clone(), cancel).await
        } else {
            run_tts_download(&weights_repo, &dir, progress.clone(), cancel).await
        };
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

pub(crate) async fn tts_status(
    State(_state): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    let downloads = DOWNLOADS.lock().unwrap();
    let progress = downloads.get(&id).map(|h| h.state.lock().unwrap().clone());
    Ok(Json(json!({ "id": id, "download": progress })))
}

pub(crate) async fn tts_cancel(
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

pub(crate) async fn tts_delete(
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

// ── Routes: settings ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct TtsSettingsBody {
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    voice: Option<String>,
    #[serde(default)]
    speed: Option<f32>,
    #[serde(default)]
    language: Option<String>,
}

pub(crate) async fn tts_settings_get(
    State(state): State<Arc<UiState>>,
) -> Result<impl IntoResponse, AppError> {
    let s = load_tts_settings(&state.config.paths.global_config_path);
    Ok(Json(json!({
        "model_id": s.model_id.unwrap_or_else(|| "macos-speech".to_string()),
        "voice": s.voice.unwrap_or_else(|| "Linh".to_string()),
        "speed": s.speed.unwrap_or(1.0),
        "language": s.language.unwrap_or_else(|| "vi".to_string()),
    })))
}

pub(crate) async fn tts_settings_put(
    State(state): State<Arc<UiState>>,
    Json(body): Json<TtsSettingsBody>,
) -> Result<impl IntoResponse, AppError> {
    // Validate speed range.
    if let Some(spd) = body.speed {
        if !(0.25..=4.0).contains(&spd) {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                "speed must be between 0.25 and 4.0".into(),
            ));
        }
    }
    let settings = TtsSettings {
        model_id: body.model_id,
        voice: body.voice,
        speed: body.speed,
        language: body.language,
    };
    save_tts_settings(&state.config.paths.global_config_path, &settings)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(json!({ "ok": true })))
}

// ── Routes: synthesize ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub(crate) struct SynthesizeBody {
    pub text: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub voice: Option<String>,
    #[serde(default)]
    pub speed: Option<f32>,
    /// Model id override; if omitted uses the persisted settings model_id.
    #[serde(default)]
    pub model_id: Option<String>,
}

pub(crate) async fn tts_synthesize(
    State(state): State<Arc<UiState>>,
    Json(body): Json<SynthesizeBody>,
) -> Result<Response, AppError> {
    if body.text.trim().is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "text is empty".into()));
    }

    // Resolve model.
    let settings = load_tts_settings(&state.config.paths.global_config_path);
    let model_id = body
        .model_id
        .clone()
        .or_else(|| settings.model_id.clone())
        .unwrap_or_else(|| "macos-speech".to_string());

    if !is_installed(&state, &model_id) {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            format!("TTS model `{model_id}` is not installed"),
        ));
    }

    // Effective params (body overrides settings).
    let language = body
        .language
        .or_else(|| settings.language.clone())
        .unwrap_or_else(|| "vi".to_string());
    let speed = body.speed.or(settings.speed).unwrap_or(1.0);
    // Voice must fall back to the persisted setting like language/speed do —
    // chat read-aloud sends only `text`, and without this it always spoke with
    // the model's default voice instead of the one picked in Settings.
    let voice = body
        .voice
        .clone()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| settings.voice.clone().filter(|v| !v.trim().is_empty()));
    let text = body.text.clone();

    let model_path = if model_id.starts_with("macos-speech") {
        None
    } else {
        Some(model_dir(&state, &model_id))
    };

    // Run synthesis in a blocking task. Uses honest auto-fallback to
    // macos-speech if the requested backend is still NotImplemented (e.g.
    // ZipVoice native port is WIP) — UI never gets a silent swap because
    // we surface the fallback via the `X-TTS-Fallback` response header.
    let outcome = tokio::task::spawn_blocking(move || {
        crate::tts::synthesize_with_fallback(
            &model_id,
            model_path.as_deref(),
            &text,
            &language,
            voice.as_deref(),
            speed,
        )
    })
    .await
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| AppError(e.0, e.1))?;

    // Return raw WAV bytes with backend/fallback metadata in headers.
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", HeaderValue::from_static("audio/wav"))
        .header(
            "Content-Disposition",
            HeaderValue::from_static("inline; filename=\"speech.wav\""),
        )
        .header("Content-Length", outcome.wav.len().to_string())
        .header(
            "X-TTS-Backend",
            HeaderValue::from_str(&outcome.used_backend)
                .unwrap_or_else(|_| HeaderValue::from_static("unknown")),
        );
    if let Some(reason) = &outcome.fallback_reason {
        // Strip control chars / non-ASCII so the header value stays valid.
        let ascii: String = reason
            .chars()
            .map(|c| {
                if c.is_ascii_graphic() || c == ' ' {
                    c
                } else {
                    '?'
                }
            })
            .collect();
        if let Ok(v) = HeaderValue::from_str(&ascii) {
            builder = builder.header("X-TTS-Fallback", v);
        }
    }
    let response = builder
        .body(Body::from(outcome.wav))
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(response)
}

// ── HF download worker ────────────────────────────────────────────────────────

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

/// Composite download for VieNeu-TTS v3 Turbo — four sources into one model dir:
///   1. int8 ONNX graphs + config + tokenizer (HF `pnnbao-ump/VieNeu-TTS-v3-Turbo`,
///      subfolder `onnx_int8/`)
///   2. MOSS codec decoder (HF `OpenMOSS-Team/MOSS-Audio-Tokenizer-Nano-ONNX`)
///   3. preset voices JSON (upstream GitHub, Apache-2.0)
///   4. `sea_g2p.bin` phoneme dictionary, extracted from the pinned sea-g2p
///      wheel on PyPI (the dictionary is platform-independent data)
async fn run_vieneu_download(
    dir: &PathBuf,
    progress: Arc<Mutex<DownloadState>>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    use anyhow::Context;

    const SEA_G2P_VERSION: &str = "0.7.18";
    let vieneu = crate::tts::vieneu::MODEL_ID;
    let codec = "OpenMOSS-Team/MOSS-Audio-Tokenizer-Nano-ONNX";
    let voices_url = "https://raw.githubusercontent.com/pnnbao97/VieNeu-TTS/main/src/vieneu/assets/voices_v3_turbo.json";

    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for f in [
        "vieneu_prefill.onnx",
        "vieneu_decode_step.onnx",
        "vieneu_acoustic_cached.onnx",
        "vieneu_backbone_shared.data",
        "vieneu_v3_heads.npz",
        "config.json",
        "tokenizer.json",
    ] {
        files.push((
            format!("{HF_BASE}/{vieneu}/resolve/main/onnx_int8/{f}"),
            dir.join("onnx_int8").join(f),
        ));
    }
    for f in [
        "moss_audio_tokenizer_decode_full.onnx",
        "moss_audio_tokenizer_decode_shared.data",
    ] {
        files.push((
            format!("{HF_BASE}/{codec}/resolve/main/{f}"),
            dir.join("codec").join(f),
        ));
    }
    files.push((voices_url.to_string(), dir.join("voices_v3_turbo.json")));

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    progress.lock().unwrap().status = DownloadStatus::Listing;
    // Resolve the sea-g2p wheel URL (any platform wheel carries the same .bin).
    let pypi: serde_json::Value = client
        .get(format!(
            "https://pypi.org/pypi/sea-g2p/{SEA_G2P_VERSION}/json"
        ))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let wheel_url = pypi["urls"]
        .as_array()
        .and_then(|urls| {
            urls.iter()
                .find(|u| u["filename"].as_str().is_some_and(|f| f.ends_with(".whl")))
        })
        .and_then(|u| u["url"].as_str())
        .context("no sea-g2p wheel found on PyPI")?
        .to_string();

    {
        let mut s = progress.lock().unwrap();
        s.files_total = (files.len() + 1) as u32;
        s.status = DownloadStatus::Downloading;
    }

    for (url, dst) in &files {
        if cancel.is_cancelled() {
            progress.lock().unwrap().status = DownloadStatus::Cancelled;
            return Ok(());
        }
        progress.lock().unwrap().current_file = Some(
            dst.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        );
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        download_url_streaming(&client, url, dst, &progress, &cancel).await?;
        if cancel.is_cancelled() {
            progress.lock().unwrap().status = DownloadStatus::Cancelled;
            return Ok(());
        }
        progress.lock().unwrap().files_done += 1;
    }

    // sea_g2p.bin: download the wheel to a temp file, extract the dictionary.
    let bin_dst = dir.join("sea_g2p.bin");
    if !bin_dst.exists() {
        progress.lock().unwrap().current_file = Some("sea_g2p.bin (wheel)".into());
        let tmp = dir.join(".sea_g2p.whl.part");
        download_url_streaming(&client, &wheel_url, &tmp, &progress, &cancel).await?;
        if cancel.is_cancelled() {
            let _ = tokio::fs::remove_file(&tmp).await;
            progress.lock().unwrap().status = DownloadStatus::Cancelled;
            return Ok(());
        }
        let bin_dst2 = bin_dst.clone();
        let tmp2 = tmp.clone();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let f = std::fs::File::open(&tmp2)?;
            let mut zip = zip::ZipArchive::new(f).context("sea-g2p wheel is not a zip")?;
            let mut entry = zip
                .by_name("sea_g2p/sea_g2p.bin")
                .context("sea_g2p.bin missing from wheel")?;
            let mut out = std::fs::File::create(&bin_dst2)?;
            std::io::copy(&mut entry, &mut out)?;
            Ok(())
        })
        .await??;
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    progress.lock().unwrap().files_done += 1;
    Ok(())
}

/// Stream one URL to a file with resume-by-size skip + progress accounting.
async fn download_url_streaming(
    client: &reqwest::Client,
    url: &str,
    dst: &std::path::Path,
    progress: &Arc<Mutex<DownloadState>>,
    cancel: &CancellationToken,
) -> anyhow::Result<()> {
    let resp = client.get(url).send().await?.error_for_status()?;
    if let (Some(len), Ok(meta)) = (resp.content_length(), std::fs::metadata(dst)) {
        if meta.len() == len {
            progress.lock().unwrap().downloaded_bytes += len;
            return Ok(()); // already complete
        }
    }
    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(dst).await?;
    while let Some(chunk) = stream.next().await {
        if cancel.is_cancelled() {
            drop(file);
            let _ = tokio::fs::remove_file(dst).await;
            return Ok(());
        }
        let bytes = chunk?;
        file.write_all(&bytes).await?;
        progress.lock().unwrap().downloaded_bytes += bytes.len() as u64;
    }
    file.flush().await?;
    Ok(())
}

async fn run_tts_download(
    repo: &str,
    dir: &PathBuf,
    progress: Arc<Mutex<DownloadState>>,
    cancel: CancellationToken,
) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    progress.lock().unwrap().status = DownloadStatus::Listing;

    let tree_url = format!("{HF_BASE}/api/models/{repo}/tree/main?recursive=true");
    let tree: Vec<HfTreeEntry> = client
        .get(&tree_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let files: Vec<(String, u64)> = tree
        .into_iter()
        .filter(|e| e.entry_type == "file" && !should_skip(&e.path))
        .map(|e| (e.path, e.size))
        .collect();

    {
        let mut s = progress.lock().unwrap();
        s.files_total = files.len() as u32;
        s.total_bytes = files.iter().map(|f| f.1).sum();
        s.status = DownloadStatus::Downloading;
    }

    for (path, size) in files {
        if cancel.is_cancelled() {
            progress.lock().unwrap().status = DownloadStatus::Cancelled;
            return Ok(());
        }
        progress.lock().unwrap().current_file = Some(path.clone());

        let dst = dir.join(&path);
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Resume: skip if exact size matches.
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

// ── Synthesis dispatch (thin wrapper around `crate::tts`) ────────────────────

/// Synthesize text to WAV bytes — pure Rust, no Python.
///
/// All backend logic lives in [`crate::tts`]; this function exists so the HTTP
/// handler keeps its existing `(StatusCode, String)` error shape and can be
/// called from `tokio::task::spawn_blocking` without re-exporting the trait.
pub fn synthesize_blocking(
    model_id: &str,
    model_path: Option<&std::path::Path>,
    text: &str,
    language: &str,
    voice: Option<&str>,
    speed: f32,
) -> Result<Vec<u8>, (StatusCode, String)> {
    crate::tts::synthesize(model_id, model_path, text, language, voice, speed)
}

#[cfg(test)]
mod synth_tests {
    use super::*;

    /// A valid WAV file starts with "RIFF" + 4 size bytes + "WAVE".
    fn looks_like_wav(bytes: &[u8]) -> bool {
        bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_speech_produces_valid_wav() {
        let wav = synthesize_blocking(
            "macos-speech",
            None,
            "Xin chào, đây là một kiểm tra.",
            "vi",
            Some("Linh"),
            1.0,
        )
        .expect("macos-speech synthesis should succeed");
        assert!(
            wav.len() > 1024,
            "wav suspiciously small: {} bytes",
            wav.len()
        );
        assert!(looks_like_wav(&wav), "output is not a RIFF/WAVE file");
    }

    /// Empty text should be rejected by the caller, but if it slips through the
    /// macOS `say` utility still emits a short valid WAV — guard the contract.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_speech_speed_changes_output_size() {
        let fast = synthesize_blocking(
            "macos-speech",
            None,
            "Một hai ba bốn năm sáu bảy.",
            "vi",
            None,
            1.5,
        )
        .expect("fast synth");
        let slow = synthesize_blocking(
            "macos-speech",
            None,
            "Một hai ba bốn năm sáu bảy.",
            "vi",
            None,
            0.75,
        )
        .expect("slow synth");
        // Slower rate ⇒ longer audio ⇒ bigger WAV.
        assert!(
            slow.len() > fast.len(),
            "expected slow ({}) > fast ({}) bytes",
            slow.len(),
            fast.len()
        );
    }

    /// Direct-backend stub contract: invoking `ZipVoiceBackend` returns
    /// `NotImplemented` until the native synthesis path lands. Complements the
    /// dispatch-level test below.
    #[test]
    fn zipvoice_backend_is_not_implemented_stub() {
        use crate::tts::{SynthesisRequest, TtsBackend, TtsError};
        let r = crate::tts::zipvoice::ZipVoiceBackend.synthesize(&SynthesisRequest {
            text: "Xin chào.",
            language: "vi",
            voice: None,
            speed: 1.0,
            model_dir: None,
        });
        match r {
            Err(TtsError::NotImplemented(_)) => {}
            Err(other) => panic!("expected NotImplemented, got {other:?}"),
            Ok(_) => panic!("ZipVoice stub should error until implemented"),
        }
    }

    /// Synthesis must be pure Rust — no external runtime should ever be spawned
    /// for a non-`macos-speech` model. Calling `synthesize_blocking` with the HF
    /// model returns `501` (the foundation-only stub) rather than a `503` from
    /// a Python fallback.
    #[test]
    fn hf_path_is_pure_rust_no_sidecar_503() {
        let r = synthesize_blocking(
            "mlx-community/zipvoice-vietnamese",
            Some(std::path::Path::new("/nonexistent/senclaw/whatever")),
            "Xin chào.",
            "vi",
            None,
            1.0,
        );
        match r {
            Err((code, msg)) => {
                assert_eq!(
                    code,
                    StatusCode::NOT_IMPLEMENTED,
                    "expected 501 from pure-Rust stub, got {code}: {msg}"
                );
                let lower = msg.to_lowercase();
                assert!(
                    !lower.contains("python")
                        && !lower.contains("pip ")
                        && !lower.contains("mlx-audio"),
                    "error message must not mention a Python sidecar runtime; got: {msg}"
                );
            }
            Ok(_) => panic!("HF stub should error until the native port is wired"),
        }
    }
}
