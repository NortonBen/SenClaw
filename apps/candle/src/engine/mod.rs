//! The Candle inference engine, moved out of the SenClaw daemon.
//!
//! The daemon's `src/local_model/` Candle half, verbatim apart from two
//! rewrites: `crate::local_model::` became `crate::engine::`, and the settings
//! lookups that reached into `gateway::ui_server::local_models` now read
//! [`local_model_core::settings`] — the same file on disk, so a machine that has
//! been using local models keeps its settings.
//!
//! Pure Rust and cross-platform, which is the whole reason this app exists
//! separately from `mlx-lm`: MLX is Apple Silicon only, and on Linux or Windows
//! this is the local option.

pub mod candle_engine;
/// The OpenAI-shape chat-template renderer. Not MLX-specific despite living
/// under the MLX tree in the daemon — it is minijinja over the checkpoint's own
/// `chat_template`, which is what both engines feed their tokenizer.
pub mod chat_template_openai;
pub mod candle_models;
/// Tokenizer wrapper shared with the MLX app — the name is historical; there
/// is nothing MLX-specific in it.
pub mod mlx_lm_utils;
pub mod models;
pub mod runtime;
pub mod stream_parser;
pub mod thinking_parse;
pub mod tokenizer_utils;

use std::path::Path;

pub use candle_engine::CandleEngine;

/// Prompt ceiling, in tokens.
///
/// Far below the MLX app's 128 k, and deliberately: Candle decodes at roughly
/// 7–12 tok/s against MLX's 60–100, so a long prompt here is minutes of prefill
/// rather than seconds. A cap that produces an answer beats a window that
/// produces a timeout.
pub const DEFAULT_CANDLE_MAX_PROMPT_TOKENS: u32 = 512;

/// Generated-token ceiling, for the same reason.
pub const DEFAULT_CANDLE_MAX_NEW_TOKENS: u32 = 512;

/// Can this engine run the checkpoint in `dir`?
///
/// From the config, never by attempting a load: the model list is rendered on
/// every screen refresh and a load costs gigabytes.
pub fn supports(dir: &Path) -> bool {
    // `find_weight_files` looks for safetensors only; a `pytorch_model.bin`
    // repo would be listed and then fail at load.
    if !local_model_core::store::has_safetensors(dir) {
        return false;
    }
    let Ok(raw) = std::fs::read_to_string(dir.join("config.json")) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    // A Whisper checkpoint sits in the same directory tree and is not an LLM.
    if v.get("n_mels").is_some() {
        return false;
    }
    // MLX-quantized checkpoints carry a `quantization` block Candle's loaders do
    // not read. Listing one would offer a model that fails at load time — and
    // the two apps share a model directory, so this is the common case, not a
    // corner one.
    if v.get("quantization").is_some() {
        return false;
    }
    candle_engine::detect_arch(&raw).is_ok()
}

/// Candle's LLM path is text-only. Declared explicitly rather than inferred:
/// SenClaw sends real image blocks to a model whose card says `vision: true`,
/// and a text-only endpoint answers those with a hard 400 that fails the whole
/// turn — where saying `false` merely routes the image through OCR.
pub fn has_vision(_dir: &Path) -> bool {
    false
}
