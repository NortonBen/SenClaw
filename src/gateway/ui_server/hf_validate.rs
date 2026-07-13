//! Pre-download HuggingFace model compatibility validation.
//!
//! Endpoints (registered in [`super::core`]):
//!   GET /api/tts/models/:id/validate
//!   GET /api/whisper/models/:id/validate
//!   GET /api/local-models/:id/validate
//!
//! Each checks the *metadata only* — repo info, file tree, and the small
//! `config.json` — against what the corresponding native loader actually
//! supports, so the UI can tell the user "this won't work" (and why) BEFORE
//! they download hundreds of MB. Nothing here fetches weights.
//!
//! The support rules deliberately mirror the real loaders:
//! - TTS   → `crate::tts::mms_vits` (HF `VitsModel*`, single speaker,
//!   safetensors + vocab.json).
//! - STT   → `crate::local_model::mlx_lm::models::whisper::ModelDimensions`
//!   (MLX-converted config with flat `n_mels`/`n_audio_state`/… keys).
//! - LLM   → `crate::local_model::mlx_native::detect_architecture`'s
//!   `model_type` dispatch.
//! Keep them in sync when a loader gains/loses an architecture.

use axum::{
    extract::Path as AxumPath,
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Serialize;
use serde_json::Value;

use super::core::AppError;

const HF_BASE: &str = "https://huggingface.co";

#[derive(Debug, Serialize)]
pub(crate) struct ValidateReport {
    pub id: String,
    /// Whether the corresponding native loader can run this checkpoint.
    pub supported: bool,
    /// Human-readable explanation (what matched, or why it's rejected).
    pub reason: String,
    /// Architecture / model_type detected from config.json (if any).
    pub architecture: Option<String>,
    /// Sum of downloadable file sizes in bytes (what a download would fetch).
    pub total_size_bytes: u64,
    /// Repo requires accepting terms / auth — our downloader can't fetch it.
    pub gated: bool,
    /// Metadata could not be fully retrieved (network/HF hiccup); the verdict
    /// is advisory and the UI may still allow a download attempt.
    pub inconclusive: bool,
}

enum Domain {
    Tts,
    Whisper,
    LocalLlm,
}

pub(crate) async fn tts_validate(
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    validate(Domain::Tts, &id).await.map(Json)
}

pub(crate) async fn whisper_validate(
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    validate(Domain::Whisper, &id).await.map(Json)
}

pub(crate) async fn local_models_validate(
    AxumPath(id): AxumPath<String>,
) -> Result<impl IntoResponse, AppError> {
    validate(Domain::LocalLlm, &id).await.map(Json)
}

/// Normalize a HuggingFace `org/repo` id from a bare id or full URL.
/// (Same rules as the per-domain download handlers.)
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
    let (org, repo) = (parts[0], parts[1]);
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

async fn validate(domain: Domain, raw_id: &str) -> Result<ValidateReport, AppError> {
    let id = normalize_hf_id(raw_id).map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // 1. Repo info — existence + gating.
    let info_url = format!("{HF_BASE}/api/models/{id}");
    let info = match client.get(&info_url).send().await {
        // HF answers 401 (not 404) for unknown repos so private repo names
        // don't leak — treat all three as "not there for us".
        Ok(r)
            if matches!(
                r.status(),
                StatusCode::NOT_FOUND | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
            ) =>
        {
            return Ok(ValidateReport {
                id,
                supported: false,
                reason: "model not found on Hugging Face (or it is private) — check the org/repo id"
                    .into(),
                architecture: None,
                total_size_bytes: 0,
                gated: false,
                inconclusive: false,
            });
        }
        Ok(r) if r.status().is_success() => r.json::<Value>().await.ok(),
        _ => None,
    };
    let Some(info) = info else {
        return Ok(ValidateReport {
            id,
            supported: false,
            reason: "could not reach the Hugging Face API — verdict unavailable; you may still try downloading".into(),
            architecture: None,
            total_size_bytes: 0,
            gated: false,
            inconclusive: true,
        });
    };
    // `gated` is false | "auto" | "manual"; private repos also 401 on files.
    let gated = !matches!(info.get("gated"), None | Some(Value::Bool(false)));
    let private = info.get("private").and_then(Value::as_bool).unwrap_or(false);
    if gated || private {
        return Ok(ValidateReport {
            id,
            supported: false,
            reason: "repo is gated/private — the built-in downloader has no Hugging Face login".into(),
            architecture: None,
            total_size_bytes: 0,
            gated: true,
            inconclusive: false,
        });
    }

    // 2. File tree — names + total size.
    let tree_url = format!("{HF_BASE}/api/models/{id}/tree/main?recursive=true");
    let tree: Vec<Value> = match client.get(&tree_url).send().await {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
        _ => Vec::new(),
    };
    let files: Vec<(String, u64)> = tree
        .iter()
        .filter(|e| e["type"] == "file")
        .map(|e| {
            (
                e["path"].as_str().unwrap_or_default().to_string(),
                e["size"].as_u64().unwrap_or(0),
            )
        })
        .collect();
    let total_size_bytes: u64 = files.iter().map(|f| f.1).sum();
    let has_file = |name: &str| files.iter().any(|(p, _)| p == name);
    let has_safetensors =
        has_file("model.safetensors") || has_file("model.safetensors.index.json");

    // 3. config.json (small) — the architecture source of truth.
    let cfg_url = format!("{HF_BASE}/{id}/resolve/main/config.json");
    let cfg: Option<Value> = match client.get(&cfg_url).send().await {
        Ok(r) if r.status().is_success() => r.json().await.ok(),
        _ => None,
    };

    let (supported, reason, architecture) = match &cfg {
        None => (
            false,
            "repo has no readable config.json — cannot determine the architecture".to_string(),
            None,
        ),
        Some(cfg) => match domain {
            Domain::Tts => check_tts(cfg, has_safetensors, &has_file),
            Domain::Whisper => check_whisper(cfg, &has_file),
            Domain::LocalLlm => check_local_llm(cfg, has_safetensors),
        },
    };

    Ok(ValidateReport {
        id,
        supported,
        reason,
        architecture,
        total_size_bytes,
        gated: false,
        inconclusive: cfg.is_none() && files.is_empty(),
    })
}

/// TTS rule — mirrors `crate::tts::mms_vits` (`dir_is_vits_model` + loader).
fn check_tts(
    cfg: &Value,
    has_safetensors: bool,
    has_file: &dyn Fn(&str) -> bool,
) -> (bool, String, Option<String>) {
    let arch = cfg["architectures"][0].as_str().map(str::to_string);
    if arch.as_deref().is_some_and(|a| a.starts_with("VieNeu")) {
        return (
            true,
            "VieNeu-TTS v3 Turbo — supported via the ONNX runtime backend (daemon built with tts-vieneu); download is composite (ONNX graphs + MOSS codec + voices + phoneme dictionary)"
                .into(),
            arch,
        );
    }
    let is_vits = arch
        .as_deref()
        .is_some_and(|a| a.starts_with("VitsModel"));
    if !is_vits {
        let a = arch.clone().unwrap_or_else(|| "unknown".into());
        return (
            false,
            format!(
                "architecture `{a}` is not supported for TTS — only HF VitsModel checkpoints \
                 (facebook/mms-tts-* and finetunes) run natively today"
            ),
            arch,
        );
    }
    let speakers = cfg["num_speakers"].as_u64().unwrap_or(1);
    let spk_embed = cfg["speaker_embedding_size"].as_u64().unwrap_or(0);
    if speakers > 1 || spk_embed > 0 {
        return (
            false,
            "multi-speaker VITS checkpoints are not supported yet (single-speaker MMS only)"
                .into(),
            arch,
        );
    }
    if !has_safetensors {
        return (
            false,
            "no model.safetensors in the repo (only .bin/.onnx?) — the native loader needs safetensors"
                .into(),
            arch,
        );
    }
    if !has_file("vocab.json") {
        return (false, "missing vocab.json (tokenizer)".into(), arch);
    }
    (
        true,
        "HF VitsModel (MMS family) — runs on the native pure-Rust VITS backend".into(),
        arch,
    )
}

/// Whisper rule — mirrors `ModelDimensions` (MLX-converted flat config).
fn check_whisper(cfg: &Value, has_file: &dyn Fn(&str) -> bool) -> (bool, String, Option<String>) {
    let mlx_dims = ["n_mels", "n_audio_state", "n_text_layer"]
        .iter()
        .all(|k| cfg.get(*k).is_some());
    let weights =
        has_file("weights.safetensors") || has_file("model.safetensors") || has_file("weights.npz");
    if mlx_dims && weights {
        let q = cfg["quantization"]["bits"]
            .as_u64()
            .map(|b| format!(", {b}-bit quantized"))
            .unwrap_or_default();
        return (
            true,
            format!("MLX Whisper checkpoint (mlx-community layout{q}) — supported"),
            Some("whisper (MLX)".into()),
        );
    }
    if cfg["model_type"].as_str() == Some("whisper") {
        return (
            false,
            "this is the HF transformers Whisper layout — use an mlx-community/whisper-* \
             conversion instead (its config.json carries flat n_mels/n_audio_state dims)"
                .into(),
            Some("whisper (transformers)".into()),
        );
    }
    let mt = cfg["model_type"].as_str().unwrap_or("unknown").to_string();
    (
        false,
        format!("not a Whisper checkpoint (model_type `{mt}`)"),
        Some(mt),
    )
}

/// Local-LLM rule — mirrors `mlx_native::detect_architecture`.
fn check_local_llm(cfg: &Value, has_safetensors: bool) -> (bool, String, Option<String>) {
    let mt_raw = cfg["model_type"].as_str().unwrap_or("").to_lowercase();
    if mt_raw.is_empty() {
        return (
            false,
            "config.json has no model_type — cannot route to a native loader".into(),
            None,
        );
    }
    let mt = mt_raw.as_str();
    let arch = Some(mt_raw.clone());

    // Explicitly-rejected families first (mirrors detect_architecture bails).
    if mt.contains("qwen3_moe") || mt.contains("qwen3_5_moe") {
        return (
            false,
            "Qwen3-MoE is not supported by native MLX — use a dense Qwen3 checkpoint".into(),
            arch,
        );
    }
    if mt.contains("qwen3_next") {
        return (false, "Qwen3-Next is not supported by native MLX".into(), arch);
    }

    let supported = mt.starts_with("mamba2")
        || mt.starts_with("falcon_mamba")
        || mt == "mamba"
        || mt.starts_with("ouro")
        || mt.contains("qwen3_5")
        || mt.contains("qwen3")
        || mt.contains("qwen2")
        || mt.starts_with("gemma2")
        || mt.starts_with("gemma3")
        || mt.starts_with("gemma4")
        || mt.starts_with("deepseek_v2")
        || mt.starts_with("llama")
        || mt.contains("bonsai");
    if !supported {
        return (
            false,
            format!(
                "model_type `{mt_raw}` has no native MLX loader — supported: qwen3 / qwen3_5 / \
                 qwen2 / llama / gemma2 / gemma3 / gemma4 / deepseek_v2 / mamba / mamba2 / \
                 falcon_mamba / ouro / bonsai"
            ),
            arch,
        );
    }
    if !has_safetensors {
        return (
            false,
            "no model.safetensors(.index.json) in the repo — the native loader needs safetensors"
                .into(),
            arch,
        );
    }
    let q = cfg["quantization"]["bits"]
        .as_u64()
        .map(|b| format!(" ({b}-bit quantized)"))
        .unwrap_or_default();
    (
        true,
        format!("model_type `{mt_raw}`{q} — supported by the native MLX runtime"),
        arch,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tts_accepts_vits_finetune_wrapper() {
        let cfg = json!({
            "architectures": ["VitsModelForPreTraining"],
            "num_speakers": 1, "speaker_embedding_size": 0
        });
        let (ok, reason, arch) = check_tts(&cfg, true, &|f| f == "vocab.json");
        assert!(ok, "{reason}");
        assert_eq!(arch.as_deref(), Some("VitsModelForPreTraining"));
    }

    #[test]
    fn tts_rejects_multispeaker_and_foreign_arch() {
        let multi = json!({"architectures": ["VitsModel"], "num_speakers": 4});
        let (ok, reason, _) = check_tts(&multi, true, &|_| true);
        assert!(!ok && reason.contains("multi-speaker"), "{reason}");

        let xtts = json!({"architectures": ["XttsModel"]});
        let (ok, reason, _) = check_tts(&xtts, true, &|_| true);
        assert!(!ok && reason.contains("XttsModel"), "{reason}");
    }

    #[test]
    fn whisper_wants_mlx_layout() {
        let mlx = json!({"n_mels": 128, "n_audio_state": 1280, "n_text_layer": 4,
                         "quantization": {"bits": 4, "group_size": 64}});
        let (ok, reason, _) = check_whisper(&mlx, &|f| f == "weights.safetensors");
        assert!(ok, "{reason}");
        assert!(reason.contains("4-bit"));

        let hf = json!({"model_type": "whisper"});
        let (ok, reason, _) = check_whisper(&hf, &|_| true);
        assert!(!ok && reason.contains("mlx-community"), "{reason}");
    }

    #[test]
    fn llm_matrix_mirrors_detect_architecture() {
        for mt in ["qwen3", "qwen3_5", "qwen2", "llama", "gemma2", "gemma3_text",
                   "gemma4_text", "deepseek_v2", "mamba", "mamba2", "falcon_mamba",
                   "ouro", "bonsai_q1"] {
            let cfg = json!({"model_type": mt});
            let (ok, reason, _) = check_local_llm(&cfg, true);
            assert!(ok, "{mt} should be supported: {reason}");
        }
        for mt in ["qwen3_moe", "qwen3_next", "phi3", "t5"] {
            let cfg = json!({"model_type": mt});
            let (ok, _, _) = check_local_llm(&cfg, true);
            assert!(!ok, "{mt} should be rejected");
        }
        // Supported arch but no safetensors → rejected.
        let (ok, reason, _) = check_local_llm(&json!({"model_type": "qwen3"}), false);
        assert!(!ok && reason.contains("safetensors"), "{reason}");
    }

    #[test]
    fn normalizes_urls_and_rejects_garbage() {
        assert_eq!(
            normalize_hf_id("https://huggingface.co/facebook/mms-tts-vie/").unwrap(),
            "facebook/mms-tts-vie"
        );
        assert!(normalize_hf_id("nonsense").is_err());
        assert!(normalize_hf_id("a/../b").is_err());
    }
}
