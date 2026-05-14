//! Qwen2 / Qwen2.5 dense checkpoints (`model_type`: `qwen2`).
//!
//! MLX layouts follow the Llama-style `model.embed_tokens` + `model.layers.*` tree
//! (without Qwen3’s attention RMS norms). Reuses [`super::llama`].

pub use super::llama::*;
