//! Registry of curated local models offered by default in the system.
//!
//! Trimmed to the four checkpoints verified end-to-end on the native MLX
//! backend (Apple Silicon): two Qwen3 transformers and two Qwen3.5 hybrid
//! (GatedDeltaNet + attention, OptiQ-quantised) models.

use std::path::Path;

use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy)]
pub struct KnownModel {
    pub id: &'static str,
    pub label: &'static str,
    pub approx_size_gb: f32,
    pub context_length: u32,
    /// Whether the in-tree Candle loader can run this checkpoint today.
    /// `false` = installable but not yet runnable locally (shown as "pending" in UI).
    pub native_supported: bool,
    /// Whether this model supports image/vision inputs.
    pub vision: bool,
}

pub const KNOWN_MODELS: &[KnownModel] = &[
    // ── Qwen3 transformers (MLX native) ───────────────────────────────────
    KnownModel {
        id: "mlx-community/Qwen3-4B-Instruct-2507-4bit",
        label: "Qwen3 4B Instruct 2507 4-bit — recommended for tools/agents (GPU-bound prefill + prefix cache)",
        approx_size_gb: 1.8,
        context_length: 250_720,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "mlx-community/Qwen2.5-0.5B-Instruct-4bit",
        label: "Qwen2.5 0.5B Instruct 4-bit — fast and lightweight chat model",
        approx_size_gb: 0.4,
        context_length: 32_768,
        native_supported: true,
        vision: false,
    },

    // ── Qwen3.5 hybrid (GatedDeltaNet + attention, OptiQ quant — MLX native) ─
    // The linear-attention layers run a sequential CPU-orchestrated scan (no
    // Metal kernel in this mlx-rs), so a *cold* prefill on long / tool-heavy
    // prompts is slower than the attention-only Qwen3 models. BUT a recurrent
    // prefix cache (snapshots the SSM/conv state at the clean pre-gen-prompt
    // boundary) now skips ~90% of prefill on multi-turn agentic loops — only the
    // first turn pays full prefill; subsequent tool turns reuse the cached state.
    // So Qwen3.5 is viable for chat AND in-request agentic. (Build release —
    // `make run-release` — or prefill is 3-5× slower; see Cargo.toml.)
    KnownModel {
        id: "mlx-community/Qwen3.5-0.8B-OptiQ-4bit",
        label: "Qwen3.5 0.8B OptiQ 4-bit — chat + agentic (prefix-cached turns)",
        approx_size_gb: 0.6,
        context_length: 262_144,
        native_supported: true,
        vision: false,
    },

    // ── Gemma 4 (PLE + cross-layer KV sharing, MLX native, text-only path) ─
    // `gemma4_text` backbone of `Gemma4ForConditionalGeneration`; vision/audio
    // towers are skipped. Per-Layer Embeddings give the "effective" 2B params.
    KnownModel {
        id: "mlx-community/gemma-4-e2b-it-4bit",
        label: "Gemma 4 E2B-it 4-bit — text-only (vision/audio towers skipped)",
        approx_size_gb: 3.6,
        context_length: 131_072,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "mlx-community/gemma-4-e2b-it-OptiQ-4bit",
        label: "Gemma 4 E2B-it OptiQ 4-bit — mixed 4/8-bit (higher quality, ~10% slower)",
        approx_size_gb: 4.0,
        context_length: 131_072,
        native_supported: true,
        vision: false,
    },
];

pub fn find(id: &str) -> Option<&'static KnownModel> {
    KNOWN_MODELS.iter().find(|m| m.id == id)
}

fn json_value_as_u32(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n
            .as_u64()
            .and_then(|u| u.try_into().ok())
            .or_else(|| n.as_i64().and_then(|i| u32::try_from(i.max(0)).ok())),
        _ => None,
    }
}

/// Keys that denote a **model** context window across HF / MLX / llama.cpp-style configs.
/// Order = preference when several exist (first match wins).
const CONTEXT_LENGTH_KEYS_TRUSTED: &[&str] = &[
    "model_max_length",
    "max_model_length",
    "max_position_embeddings",
    "max_seq_len",
    "max_sequence_length",
    "max_sequence_len",
    "n_ctx",
    "context_length",
    "seq_length",
];

/// `max_length` is often a tokenizer-only default (e.g. 20) — only treat as context when large enough.
const CONTEXT_LENGTH_MAX_LENGTH_MIN: u32 = 512;

fn scan_map_for_context_length(obj: &Map<String, Value>) -> Option<u32> {
    for key in CONTEXT_LENGTH_KEYS_TRUSTED {
        if let Some(n) = obj.get(*key).and_then(json_value_as_u32) {
            if n > 0 {
                return Some(n);
            }
        }
    }
    if let Some(n) = obj.get("max_length").and_then(json_value_as_u32) {
        if n >= CONTEXT_LENGTH_MAX_LENGTH_MIN {
            return Some(n);
        }
    }
    None
}

fn context_length_from_hf_json_value(v: &Value) -> Option<u32> {
    let obj = v.as_object()?;
    scan_map_for_context_length(obj)
        .or_else(|| {
            obj.get("text_config")
                .and_then(|x| x.as_object())
                .and_then(scan_map_for_context_length)
        })
        .or_else(|| {
            obj.get("model")
                .and_then(|x| x.as_object())
                .and_then(scan_map_for_context_length)
        })
}

/// Read the maximum sequence length from the on-disk HF-style config files.
///
/// Scans `tokenizer_config.json` then `config.json`; handles various field names
/// across HF, MLX, and llama.cpp layouts.
pub fn read_model_context_length_from_dir(model_dir: &Path) -> Option<u32> {
    const MIN_SANE: u32 = 512;
    const MAX_SANE: u32 = 8_388_608;

    let tok_path = model_dir.join("tokenizer_config.json");
    if let Ok(raw) = std::fs::read_to_string(&tok_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if let Some(n) = context_length_from_hf_json_value(&v) {
                return Some(n.clamp(MIN_SANE, MAX_SANE));
            }
        }
    }

    let cfg_path = model_dir.join("config.json");
    let raw = std::fs::read_to_string(&cfg_path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let n = context_length_from_hf_json_value(&v)?;
    Some(n.clamp(MIN_SANE, MAX_SANE))
}

/// Infer whether a model supports vision from its HuggingFace repo id.
pub fn infer_vision_from_id(id: &str) -> bool {
    let lower = id.to_lowercase();
    lower.contains("vl")
        || lower.contains("vision")
        || lower.contains("visual")
        || lower.contains("llava")
        || lower.contains("clip")
        || lower.contains("intern-vit")
        || lower.contains("phi-3-vision")
        || lower.contains("pixtral")
        || lower.contains("minicpm-v")
}
