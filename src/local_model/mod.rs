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

#[cfg(feature = "local-mlx")]
pub mod chat_template_openai;
pub mod models;
pub mod runtime;
pub mod stream_parser;
pub mod thinking_parse;

// ── Whisper ASR audio front-end (pure-Rust, CPU; feature `whisper-audio`) ──
#[cfg(feature = "whisper-audio")]
pub mod audio;

// ── Gemma-4 vision input front-end (image decode → CHW float32) ──────────
// Bundled into `local-mlx`: an image preprocessor without the MLX model that
// consumes it would be dead code. Cross-platform pure-Rust math (no native
// deps); tests run as part of the regular `--features local-mlx` suite.
#[cfg(feature = "local-mlx")]
pub mod image_input;

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

// ── Native Whisper ASR driver (Apple Silicon; feature `local-mlx-whisper`) ──
#[cfg(feature = "local-mlx-whisper")]
pub mod whisper_transcribe;
#[cfg(feature = "local-mlx-whisper")]
pub use whisper_transcribe::WhisperEngine;

// ── PaddleOCR + MNN OCR engine (cross-platform; feature `ocr-paddle`) ────────
// Pure-Rust crate `ocr-rs` (rust-paddle-ocr). macOS gets Metal/CoreML
// acceleration via the additive `ocr-paddle-metal` feature. Default builds
// don't pull MNN's C++ toolchain.
#[cfg(feature = "ocr-paddle")]
pub mod ocr;
#[cfg(feature = "ocr-paddle")]
pub use ocr::{OcrBlock, OcrEngine, OcrResult};

// ── Shared tokenizer / chat-template stack (candle) ───────────────────────
#[cfg(feature = "local-candle")]
pub mod tokenizer_utils;

// ── Candle inference backend ───────────────────────────────────────────────
#[cfg(feature = "local-candle")]
pub mod candle_engine;
#[cfg(feature = "local-candle")]
pub mod candle_models;
#[cfg(feature = "local-candle")]
pub use candle_engine::CandleEngine;
