pub mod bonsai_q1;
pub mod falcon_mamba;
pub mod gated_delta;
pub mod gemma2;
pub mod gemma3;
pub mod gemma4;
/// Gemma-4 vision-side modules (MultimodalEmbedder now; vision tower
/// pending). Lives alongside `gemma4` and inherits its `local-mlx` gate via
/// the parent `mlx_lm` module — no extra feature flag needed.
pub mod gemma4_vision;
pub mod llama;
pub mod mamba2;
pub mod qwen3;
pub mod qwen3_5;
/// Shared Qwen-family parser primitives (used by `qwen3` and `qwen3_5`).
pub mod qwen_common;
/// Whisper encoder/decoder ASR (gated on `local-mlx-whisper` via its driver,
/// but the model itself compiles under `local-mlx`).
pub mod whisper;
