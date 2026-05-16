//! Registry of curated local models supported by the Candle backend.

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
    // ── Qwen3 (standard HF safetensors — candle compatible) ────────────────
    KnownModel {
        id: "Qwen/Qwen3-0.6B",
        label: "Qwen3 0.6B",
        approx_size_gb: 1.2,
        context_length: 32_768,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "Qwen/Qwen3-1.7B",
        label: "Qwen3 1.7B",
        approx_size_gb: 3.4,
        context_length: 32_768,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "Qwen/Qwen3-4B",
        label: "Qwen3 4B — recommended",
        approx_size_gb: 8.0,
        context_length: 32_768,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "Qwen/Qwen3-8B",
        label: "Qwen3 8B",
        approx_size_gb: 16.0,
        context_length: 32_768,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "Qwen/Qwen3-14B",
        label: "Qwen3 14B",
        approx_size_gb: 28.0,
        context_length: 32_768,
        native_supported: true,
        vision: false,
    },

    // ── Qwen3 bf16 (MLX-community) — BF16 safetensors, candle compatible ──
    KnownModel {
        id: "mlx-community/Qwen3-0.6B-bf16",
        label: "Qwen3 0.6B (bf16, mlx-community)",
        approx_size_gb: 1.2,
        context_length: 128_000,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "mlx-community/Qwen3-1.7B-bf16",
        label: "Qwen3 1.7B (bf16, mlx-community)",
        approx_size_gb: 3.4,
        context_length: 128_000,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "mlx-community/Qwen3-4B-bf16",
        label: "Qwen3 4B (bf16, mlx-community)",
        approx_size_gb: 8.0,
        context_length: 128_000,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "mlx-community/Qwen3.5-0.8B-OptiQ-4bit",
        label: "Qwen3.5 0.8B OptiQ 4-bit (mlx-community)",
        approx_size_gb: 0.6,
        context_length: 262_144,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "mlx-community/Qwen3-8B-bf16",
        label: "Qwen3 8B (bf16, mlx-community)",
        approx_size_gb: 16.0,
        context_length: 128_000,
        native_supported: true,
        vision: false,
    },

    // ── Gemma 3 / 4 (Google — model_type="gemma3") ────────────────────────
    KnownModel {
        id: "google/gemma-3-1b-it",
        label: "Gemma 3 1B Instruct",
        approx_size_gb: 2.0,
        context_length: 32_768,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "google/gemma-3-4b-it",
        label: "Gemma 3 4B Instruct",
        approx_size_gb: 8.0,
        context_length: 131_072,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "google/gemma-3-12b-it",
        label: "Gemma 3 12B Instruct",
        approx_size_gb: 24.0,
        context_length: 131_072,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "google/gemma-4-9b-it",
        label: "Gemma 4 9B Instruct",
        approx_size_gb: 18.0,
        context_length: 131_072,
        native_supported: true,
        vision: false,
    },

    // ── Mamba 1 (SSM — state-spaces, backbone.* prefix in safetensors) ───
    KnownModel {
        id: "state-spaces/mamba-2.8b",
        label: "Mamba 1 2.8B (SSM)",
        approx_size_gb: 5.6,
        context_length: 2_048,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "state-spaces/mamba-1.4b",
        label: "Mamba 1 1.4B (SSM)",
        approx_size_gb: 2.8,
        context_length: 2_048,
        native_supported: true,
        vision: false,
    },

    // ── Mamba 2 (SSD — state-spaces, backbone.* prefix in safetensors) ───
    KnownModel {
        id: "state-spaces/mamba2-2.7b",
        label: "Mamba 2 2.7B (SSM/SSD)",
        approx_size_gb: 5.4,
        context_length: 4_096,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "state-spaces/mamba2-1.3b",
        label: "Mamba 2 1.3B (SSM/SSD)",
        approx_size_gb: 2.6,
        context_length: 4_096,
        native_supported: true,
        vision: false,
    },
    KnownModel {
        id: "state-spaces/mamba2-370m",
        label: "Mamba 2 370M (SSM/SSD)",
        approx_size_gb: 0.74,
        context_length: 4_096,
        native_supported: true,
        vision: false,
    },

    // ── Qwen3 4-bit (MLX quantisation — NOT compatible with Candle loader) ─
    KnownModel {
        id: "mlx-community/Qwen3-4B-4bit",
        label: "Qwen3 4B 4-bit (mlx-community) — not yet supported",
        approx_size_gb: 2.3,
        context_length: 128_000,
        native_supported: false,
        vision: false,
    },
    KnownModel {
        id: "mlx-community/Qwen3-8B-4bit",
        label: "Qwen3 8B 4-bit (mlx-community) — not yet supported",
        approx_size_gb: 4.6,
        context_length: 128_000,
        native_supported: false,
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
