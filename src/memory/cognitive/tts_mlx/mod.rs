use axum::http::StatusCode;
use std::path::Path;

pub mod tokenizer;
pub mod modules;

pub fn synthesize(
    _model_id: &str,
    _model_path: Option<&Path>,
    _text: &str,
    _language: &str,
    _voice: Option<&str>,
    _speed: f32,
) -> Result<Vec<u8>, (StatusCode, String)> {
    // Phase 1 stub
    Err((
        StatusCode::NOT_IMPLEMENTED,
        "Pure Rust MLX TTS is currently under construction. Phase 1 starting...".into(),
    ))
}
