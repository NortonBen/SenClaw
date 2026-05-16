//! Local LLM inference — Candle (cross-platform) and MLX native (Apple Silicon) backends.
//!
//! ## Backends
//!
//! | Backend | Feature flag | Device | Typical decode (M4 Pro, 0.6B) |
//! |---------|-------------|--------|-------------------------------|
//! | Candle CPU+Accelerate | `local-candle-accelerate` | CPU F32 | ~12 tok/s |
//! | Candle Metal GPU | `local-candle-metal` | Metal BF16 | ~7 tok/s |
//! | MLX native (mlx-rs) | `local-mlx` | MLX Metal | ~60–100 tok/s |
//!
//! ## MLX native setup
//!
//! Apple Silicon only. Uses **`mlx-rs`** plus Qwen3 weights/templates vendored in-tree.
//! No Python required; the inference runs fully in-process via native Rust bindings.
//!
//! ```bash
//! cargo build --features local-mlx
//! ```
//!
//! Native MLX inference on Apple Silicon via **`mlx-rs`** (fork [oxiglade/mlx-rs](https://github.com/oxiglade/mlx-rs))
//! plus Qwen3/chat-template code vendored in `mlx_lm` / `mlx_lm_utils` is gated behind the `local-mlx`
//! feature so default builds remain cross-platform.

pub mod models;
pub mod runtime;
pub mod thinking_parse;
#[cfg(feature = "local-mlx")]
pub mod chat_template_openai;

pub use models::{read_model_context_length_from_dir, KnownModel, KNOWN_MODELS};
pub use runtime::{
    ChatMessage, LocalModelRuntime, Role, RuntimeEndpoint, RuntimeHealth, RuntimeStatus,
};

// ── Native MLX inference backend (Apple Silicon, mlx-rs) ──────────────────
#[cfg(feature = "local-mlx")]
pub mod mlx_lm;
#[cfg(feature = "local-mlx")]
pub mod mlx_lm_utils;
#[cfg(feature = "local-mlx")]
pub mod mlx_native;
#[cfg(feature = "local-mlx")]
pub mod mlx_prompt;

#[cfg(feature = "local-mlx")]
pub use mlx_native::MlxNativeEngine;

// ── Shared tokenizer / chat-template stack (candle) ───────────────────────
#[cfg(feature = "local-candle")]
pub mod tokenizer_utils;

// ── Candle inference backend ───────────────────────────────────────────────
#[cfg(feature = "local-candle")]
pub mod candle_models;
#[cfg(feature = "local-candle")]
pub mod candle_engine;
#[cfg(feature = "local-candle")]
pub use candle_engine::CandleEngine;
