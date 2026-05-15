//! Registry of curated local models.
//!
//! Sizes are approximate disk footprints of the MLX 4-bit conversions.

use std::path::Path;

use serde_json::{Map, Value};

#[derive(Debug, Clone, Copy)]
pub struct KnownModel {
    pub id: &'static str,
    pub label: &'static str,
    pub approx_size_gb: f32,
    pub context_length: u32,
    /// Whether the in-tree Qwen3 MLX loader supports this checkpoint today.
    /// Used by the UI to mark "pending upstream" entries.
    pub native_supported: bool,
}

pub const KNOWN_MODELS: &[KnownModel] = &[
    // ── Qwen3 bf16 — native Rust loader works on unquantized weights ─────
    KnownModel {
        id: "mlx-community/Qwen3-0.6B-bf16",
        label: "Qwen3 0.6B (bf16)",
        approx_size_gb: 1.2,
        context_length: 128_000,
        native_supported: true,
    },
    KnownModel {
        id: "mlx-community/Qwen3-1.7B-bf16",
        label: "Qwen3 1.7B (bf16)",
        approx_size_gb: 3.4,
        context_length: 128_000,
        native_supported: true,
    },
    KnownModel {
        id: "mlx-community/Qwen3-4B-bf16",
        label: "Qwen3 4B (bf16) — recommended",
        approx_size_gb: 8.0,
        context_length: 128_000,
        native_supported: true,
    },
    KnownModel {
        id: "mlx-community/Qwen3-8B-bf16",
        label: "Qwen3 8B (bf16)",
        approx_size_gb: 16.0,
        context_length: 128_000,
        native_supported: true,
    },

    // ── Qwen3 4-bit — loaded via `nn::quantize` + custom safetensor remap
    //    (mlx-rs stores quantized weights at `*.inner.weight`).
    KnownModel {
        id: "mlx-community/Qwen3-4B-4bit",
        label: "Qwen3 4B 4-bit",
        approx_size_gb: 2.3,
        context_length: 128_000,
        native_supported: true,
    },
    KnownModel {
        id: "mlx-community/Qwen3-8B-4bit",
        label: "Qwen3 8B 4-bit",
        approx_size_gb: 4.6,
        context_length: 128_000,
        native_supported: true,
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

/// Keys that denote a **model** context window across HF / MLX / llama.cpp-style `tokenizer_config.json`
/// and `config.json` layouts. Order = preference when several exist (first match wins).
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

/// Walk root then common nested objects; different checkpoints nest limits differently.
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

/// Maximum sequence length from on-disk HF-style config (no weights load).
///
/// Each local model directory may use a slightly different JSON shape. This scans, in order:
///
/// 1. **`tokenizer_config.json`** — trusted keys on the root object, then `text_config`, then `model`.
/// 2. **`config.json`** — same scan (VLM / composite configs often put lengths under `text_config`).
///
/// Trusted keys include `model_max_length`, `max_position_embeddings`, `max_seq_len`, `n_ctx`, …  
/// `max_length` is only used when ≥ 512 so we do not pick HF’s tiny tokenizer placeholders.
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
