//! Registry of curated local models.
//!
//! Sizes are approximate disk footprints of the MLX 4-bit conversions.

use std::path::Path;

use serde_json::Value;

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

/// Maximum sequence length from on-disk HF-style config (no weights load).
///
/// Preference order:
/// 1. [`tokenizer_config.json`](https://huggingface.co/docs/transformers/main_classes/tokenizer#transformers.PreTrainedTokenizerFast.model_max_length) — `model_max_length`
/// 2. `config.json` — `model_max_length`, then `max_position_embeddings`, then the same keys under `text_config` (VLM / composite configs).
pub fn read_model_context_length_from_dir(model_dir: &Path) -> Option<u32> {
    const MIN_SANE: u32 = 512;
    const MAX_SANE: u32 = 8_388_608;

    let tok_path = model_dir.join("tokenizer_config.json");
    if let Ok(raw) = std::fs::read_to_string(&tok_path) {
        if let Ok(v) = serde_json::from_str::<Value>(&raw) {
            if let Some(n) = v.get("model_max_length").and_then(json_value_as_u32) {
                return Some(n.clamp(MIN_SANE, MAX_SANE));
            }
        }
    }

    let cfg_path = model_dir.join("config.json");
    let raw = std::fs::read_to_string(&cfg_path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;

    let from_root = || {
        v.get("model_max_length")
            .or_else(|| v.get("max_position_embeddings"))
            .and_then(json_value_as_u32)
    };
    let from_text = || {
        v.get("text_config").and_then(|tc| {
            tc.get("model_max_length")
                .or_else(|| tc.get("max_position_embeddings"))
                .and_then(json_value_as_u32)
        })
    };

    let n = from_root().or_else(from_text)?;
    Some(n.clamp(MIN_SANE, MAX_SANE))
}
