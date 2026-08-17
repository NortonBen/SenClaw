//! The MLX inference engine, moved out of the SenClaw daemon.
//!
//! These modules are the daemon's `src/local_model/` MLX half, verbatim apart
//! from two rewrites: `crate::local_model::` became `crate::engine::`, and the
//! three settings lookups that reached into `gateway::ui_server::local_models`
//! now read [`local_model_core::settings`], which is the same file on disk.
//!
//! What deliberately did **not** come along:
//!
//! - `mlx_lm/models/whisper.rs` — ASR, not an LLM. The daemon still serves
//!   speech-to-text, and Whisper's only dependency on this tree was
//!   `mlx_lm::error::Error`.
//! - The Whisper / ZipVoice / MMS-VITS drivers and the cognitive embedder, for
//!   the same reason. They keep `mlx-rs` in the daemon, which is why this move
//!   buys release cadence rather than daemon build time — the ~12 600 lines of
//!   model architectures under `mlx_lm/models/` are where every new checkpoint
//!   lands, and they no longer ride the daemon's signed, notarised release.

pub mod chat_template_openai;
pub mod runtime;
pub mod image_input;
pub mod mlx_lm;
pub mod mlx_lm_utils;
pub mod mlx_native;
pub mod mlx_prompt;
pub mod models;
pub mod stream_parser;
pub mod thinking_parse;

use std::path::Path;

pub use mlx_native::MlxNativeEngine;

/// Can this engine run the checkpoint in `dir`?
///
/// Answered by the architecture table rather than by trying to load: the model
/// list is rendered on every screen refresh, and a load costs gigabytes.
pub fn supports(model_id: &str, dir: &Path) -> bool {
    // Weights this engine can read at all. A repo shipping only
    // `pytorch_model.bin` is a complete, valid download of a supported
    // architecture that MLX cannot load — listing it puts a model in the picker
    // that fails the moment it is selected.
    if !local_model_core::store::has_safetensors(dir) {
        return false;
    }
    let Ok(raw) = std::fs::read_to_string(dir.join("config.json")) else {
        return false;
    };
    // A Whisper checkpoint sits in the same directory tree and is not an LLM.
    // Offering it in the model picker produces a model that loads and then
    // answers nothing, which reads as the engine being broken.
    if serde_json::from_str::<serde_json::Value>(&raw)
        .is_ok_and(|v| v.get("n_mels").is_some())
    {
        return false;
    }
    mlx_native::detect_architecture(model_id, dir).is_ok()
}

/// Does this checkpoint take image input?
///
/// From the config, never from the model id. A local checkpoint is named things
/// like `mlx-community/Qwen3.5-2B-OptiQ-4bit`, which matches no vendor pattern —
/// a name-based guess is right or wrong by accident, and the wrong direction is
/// expensive: a text-only endpoint answers an image block with a hard 400 that
/// fails the whole turn.
pub fn has_vision(dir: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(dir.join("config.json")) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    v.get("vision_config").is_some()
        || v.get("vision_tower").is_some()
        || v["architectures"]
            .as_array()
            .is_some_and(|a| a.iter().any(|s| {
                s.as_str()
                    .is_some_and(|s| s.contains("Vision") || s.contains("Conditional"))
            }))
}
