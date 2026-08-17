//! On-device OCR. Nothing else runs in this process any more.
//!
//! This module used to hold every local model SenClaw could run: MLX and Candle
//! LLM engines, Whisper ASR, the MLX TTS backends, ~30 000 lines of model
//! architectures. All of it moved to Space Apps in this repo:
//!
//! | | |
//! |---|---|
//! | [`apps/mlx-lm`](../../apps/mlx-lm) | LLM on Apple Silicon (MLX) |
//! | [`apps/candle`](../../apps/candle) | LLM, cross-platform (pure Rust) |
//! | [`apps/mlx-media`](../../apps/mlx-media) | Whisper ASR + the MLX TTS voices |
//!
//! The daemon reaches all three over loopback through its own app proxy, which
//! starts a stopped session app on the first request. It no longer compiles
//! `mlx-rs`, and `make app-build` no longer builds MLX from C++ source.
//!
//! OCR stays because it is not MLX: [`ocr`] is PaddleOCR on the MNN backend,
//! and the daemon calls it directly on the image-attachment path.

// ── PaddleOCR + MNN OCR engine (cross-platform; feature `ocr-paddle`) ────────
#[cfg(feature = "ocr-paddle")]
pub mod ocr;
#[cfg(feature = "ocr-paddle")]
pub use ocr::{OcrBlock, OcrEngine, OcrResult};
