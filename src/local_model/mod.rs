//! Local LLM inference runtimes.
//!
//! Native MLX inference on Apple Silicon via `mlx-rs` + `mlx-lm` is gated
//! behind the `local-mlx` feature so default builds remain cross-platform.
//! See `docs/mlx-rs-turboquant-native-runtime.md` for the technical plan.

pub mod models;
pub mod runtime;

pub use models::{KnownModel, KNOWN_MODELS};
pub use runtime::{
    ChatMessage, LocalModelRuntime, Role, RuntimeEndpoint, RuntimeHealth, RuntimeStatus,
};

#[cfg(feature = "local-mlx")]
pub mod mlx_native;

#[cfg(feature = "local-mlx")]
pub use mlx_native::MlxNativeEngine;
