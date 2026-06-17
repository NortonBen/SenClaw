//! On-device OCR powered by PaddleOCR (PP-OCRv4/v5) on the MNN inference
//! backend, via the pure-Rust crate [`ocr-rs`](https://github.com/zibo-chen/rust-paddle-ocr).
//!
//! ## Features
//!
//! - Cross-platform CPU inference (Linux/Windows/macOS).
//! - macOS Metal/CoreML acceleration when built with the additive
//!   `ocr-paddle-metal` feature.
//! - ~10 supported languages including **Vietnamese** via the
//!   `latin_PP-OCRv5_mobile_rec` recognition model.
//! - Lazy-loaded engine with explicit [`OcrEngine::unload`] to release RAM
//!   between requests — same pattern as [`super::whisper_transcribe::WhisperEngine`].
//!
//! ## Layout
//!
//! ```text
//!     {ocr_models_dir}/{safe-id}/
//!         ├── det.mnn    PP-OCRv5 detection model
//!         ├── rec.mnn    Recognition model (per language / latin)
//!         └── keys.txt   Charset (ppocr_keys_v5.txt or latin variant)
//! ```
//!
//! Catalog entries (default model URLs) live in [`catalog`].

pub mod catalog;
pub mod engine;

pub use catalog::{
    default_entry, installed_model_files, CatalogEntry, CATALOG, DEFAULT_MODEL_ID, DET_FILE,
    KEYS_FILE, REC_FILE,
};
pub use engine::{OcrBlock, OcrEngine, OcrResult};
