//! Local LLM inference runtimes.
//!
//! Native MLX inference on Apple Silicon via **`mlx-rs`** (fork [oxiglade/mlx-rs](https://github.com/oxiglade/mlx-rs))
//! plus Qwen3/chat-template code vendored in `mlx_lm` / `mlx_lm_utils` is gated behind the `local-mlx`
//! feature so default builds remain cross-platform.

// Higgs-models vendored code (gated behind local-mlx feature)
#[cfg(feature = "local-mlx")]
pub mod higgs_error;
#[cfg(feature = "local-mlx")]
pub mod higgs_registry;
#[cfg(feature = "local-mlx")]
pub mod higgs_sampling;
#[cfg(feature = "local-mlx")]
pub mod higgs_cache;
#[cfg(feature = "local-mlx")]
pub mod higgs_utils;
#[cfg(feature = "local-mlx")]
pub mod higgs_transformer;
#[cfg(feature = "local-mlx")]
pub mod higgs_turboquant;

pub mod models;
pub mod runtime;

pub use models::{read_model_context_length_from_dir, KnownModel, KNOWN_MODELS};
pub use runtime::{
    ChatMessage, LocalModelRuntime, Role, RuntimeEndpoint, RuntimeHealth, RuntimeStatus,
};

#[cfg(feature = "local-mlx")]
pub mod mlx_lm;
#[cfg(feature = "local-mlx")]
pub mod mlx_lm_utils;
#[cfg(feature = "local-mlx")]
pub mod mlx_native;

#[cfg(feature = "local-mlx")]
pub use mlx_native::MlxNativeEngine;
