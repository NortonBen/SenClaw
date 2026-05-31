//! Local LLM inference — Candle (cross-platform) and MLX native (Apple Silicon) backends.
//!
//! ## Unified per-model interface
//!
//! All local models — regardless of arch (Qwen, Llama, Gemma-3/4, Mamba, …) —
//! plug into the same interface, driven by per-model config files:
//!
//! ```text
//!     ~/.senclaw/local-models/<safe-id>/
//!         ├── tokenizer_config.json          ─┐
//!         ├── chat_template.jinja  (or .json) ┼─► ParserConfig::from_model_dir()
//!         ├── tokenizer.json                  │           │
//!         ├── config.json                     │           │
//!         └── model.safetensors[.index.json] ─┘           │
//!                                                          ▼
//!                                       ┌───────────────────────────┐
//!                                       │   ParserConfig            │
//!                                       │   • chat_template         │
//!                                       │   • bos_token / eos_token │
//!                                       │   • MarkerSet (per-arch)  │
//!                                       └───────────────────────────┘
//!                                            │              │
//!                              ┌─────────────┘              └─────────────┐
//!                              ▼                                          ▼
//!                       INPUT (rendering)                       OUTPUT (parsing)
//!                       render_chat() → prompt           LocalStreamParser → events
//!                                                                │
//!                                                                ▼
//!                                          ParserEvent::{Visible, Reasoning, ToolCall}
//!                                                  (single OpenAI-compatible shape)
//!
//! Adding a new local model: ship the standard HuggingFace files. Marker
//! discovery is automatic: explicit named role-tokens (`soc_token`,
//! `stc_token`, …) → chat_template scan → dialect preset fallback. No code
//! changes needed for the parser interface.
//!
//! Entry points the rest of the codebase uses:
//! - [`MlxNativeEngine::stream_events_to_channel`] — canonical event stream
//!   (markers stripped, OpenAI-shape tool_calls).
//! - [`MlxNativeEngine::parser_config`] — engine's loaded config for callers
//!   that want to drive the parser themselves (`stream_openai_to_channel` +
//!   `pipe_text_stream_to_events`).
//! - [`stream_parser::ParserConfig::from_model_dir`] — for tools, examples,
//!   and tests that need to inspect a model's interface without spinning up
//!   the engine.
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
pub mod stream_parser;
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
