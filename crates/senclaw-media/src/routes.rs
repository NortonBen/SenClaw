//! The HTTP surface: OpenAI-shaped speech endpoints.
//!
//! `/v1/audio/transcriptions` is the OpenAI name on purpose. The daemon is the
//! only caller today, but a surface someone else already knows how to drive
//! costs nothing extra to expose, and it keeps the sidecar testable with `curl`
//! alone.
//!
//! Two decode backends stand behind the one route. **MLX** on Apple Silicon —
//! several times faster on the same audio — and **Candle** (pure Rust, CPU)
//! everywhere else, which is what makes the Windows and Linux sidecars real
//! transcribers instead of 501 stubs. `SENCLAW_ASR_BACKEND=candle` forces the
//! Candle path on macOS too; that override is how the cross-platform backend
//! gets integration-tested on the machine this repo is developed on.
//!
//! The two backends read **different checkpoint families** from the same model
//! root: MLX loads `mlx-community/*`, Candle loads HF-layout `openai/*`. A
//! directory the chosen backend cannot load is a 400 naming the family to
//! download, not a load-time stack trace.
//!
//! Every handler runs its MLX work on `spawn_blocking`. MLX is a blocking,
//! Metal-bound workload; on the async reactor it would stall every other request
//! this app is serving — including the daemon's health probe, which would then
//! report the app as down in the middle of a working transcription.

use std::path::PathBuf;

use axum::{
    body::Bytes,
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;

#[cfg(target_os = "macos")]
use crate::mlx::WhisperEngine;

/// Which decoder serves this request.
fn use_candle() -> bool {
    if cfg!(not(target_os = "macos")) {
        return true;
    }
    std::env::var("SENCLAW_ASR_BACKEND")
        .map(|v| v.trim().eq_ignore_ascii_case("candle"))
        .unwrap_or(false)
}

pub fn router() -> Router {
    Router::new()
        .route("/v1/audio/transcriptions", post(transcribe))
}

fn err(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(json!({ "error": { "message": msg.into() } }))).into_response()
}

#[derive(Deserialize)]
struct TranscribeQuery {
    /// Absolute path to the Whisper checkpoint directory. The daemon owns model
    /// storage and download, so it passes the resolved directory rather than a
    /// model id this app would have to look up a second time.
    model_dir: String,
    /// ISO language hint, or omitted to let Whisper detect.
    #[serde(default)]
    language: Option<String>,
    /// Ask for per-segment timings instead of a flat string.
    #[serde(default)]
    timestamps: bool,
    /// Original upload filename, or just its extension.
    ///
    /// **Load-bearing.** The decoder probes the container *by file extension*,
    /// so a temp file written as `.audio` fails on a perfectly good AIFF. The
    /// daemon has the name; passing it costs a query parameter.
    #[serde(default)]
    filename: Option<String>,
}

/// `POST /v1/audio/transcriptions?model_dir=…` with the raw audio as the body.
///
/// Raw bytes rather than `multipart/form-data`: the only client is the daemon,
/// which already has the bytes in memory, and multipart would mean buffering
/// them a second time to parse a single field.
async fn transcribe(Query(q): Query<TranscribeQuery>, body: Bytes) -> Response {
    if body.is_empty() {
        return err(StatusCode::BAD_REQUEST, "empty audio body");
    }
    let dir = PathBuf::from(&q.model_dir);
    if !dir.is_dir() {
        return err(
            StatusCode::BAD_REQUEST,
            format!("model_dir does not exist: {}", dir.display()),
        );
    }

    let language = q.language.clone();
    let timestamps = q.timestamps;
    let filename = q.filename.clone();
    let audio = body.to_vec();

    if use_candle() {
        return transcribe_candle(dir, filename, body.to_vec(), language).await;
    }
    #[cfg(not(target_os = "macos"))]
    unreachable!("use_candle() is always true off macOS");
    #[cfg(target_os = "macos")]
    transcribe_mlx(dir, filename, body.to_vec(), language, timestamps).await
}

/// Candle path: chunked greedy decode on the CPU. No per-segment stats — the
/// `timestamps` flag degrades to plain text rather than erroring, because the
/// daemon always asks for stats and a missing extra field beats a failed
/// transcription.
async fn transcribe_candle(
    dir: PathBuf,
    filename: Option<String>,
    audio: Vec<u8>,
    language: Option<String>,
) -> Response {
    if !crate::candle_whisper::supports_dir(&dir) {
        return err(
            StatusCode::BAD_REQUEST,
            format!(
                "`{}` is not a Candle-compatible Whisper checkpoint — download an                  HF-layout repo such as `openai/whisper-large-v3-turbo` for this platform",
                dir.display()
            ),
        );
    }
    let result = tokio::task::spawn_blocking(move || {
        let tmp = write_probe_file(&audio, filename.as_deref())?;
        // Loaded per request and dropped after, same policy as the MLX path:
        // an idle sidecar must cost megabytes, and a CPU reload is cheap
        // relative to a CPU decode.
        let mut engine = crate::candle_whisper::CandleWhisper::load(&dir)?;
        let out = engine.transcribe_file(&tmp, language.as_deref());
        let _ = std::fs::remove_file(&tmp);
        out.map(|text| json!({ "text": text, "backend": "candle" }))
    })
    .await;
    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("transcription task panicked: {e}"),
        ),
    }
}

/// Write the upload where a decoder can probe it. The extension is
/// load-bearing: symphonia probes the container by it.
fn write_probe_file(audio: &[u8], filename: Option<&str>) -> anyhow::Result<std::path::PathBuf> {
    let ext = filename
        .and_then(|f| std::path::Path::new(f).extension())
        .and_then(|e| e.to_str())
        .filter(|e| !e.is_empty())
        .unwrap_or("wav");
    let tmp = std::env::temp_dir().join(format!(
        "senclaw-asr-{}-{}.{ext}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::write(&tmp, audio)?;
    Ok(tmp)
}

#[cfg(target_os = "macos")]
async fn transcribe_mlx(
    dir: PathBuf,
    filename: Option<String>,
    audio: Vec<u8>,
    language: Option<String>,
    timestamps: bool,
) -> Response {
    let result = tokio::task::spawn_blocking(move || {
        let engine = WhisperEngine::new(dir);
        // Make sure the ~2 GB of weights goes back to the OS whichever way this
        // closure exits. Dropping the engine is not enough: MLX keeps freed
        // buffers in its own cache, so without an explicit unload (which also
        // clears that cache) the process idles at the full model size — 2.8 GB
        // resident for a sidecar that is supposed to cost megabytes between
        // requests. Reloading on the next request costs a couple of seconds,
        // which is the right trade for a process that sits idle most of the day.
        struct UnloadOnExit<'a>(&'a WhisperEngine);
        impl Drop for UnloadOnExit<'_> {
            fn drop(&mut self) {
                self.0.unload();
            }
        }
        let _unload = UnloadOnExit(&engine);
        // The engine reads a file; write the upload to a temp path rather than
        // teaching it a second entry point. Dropped when this closure returns.
        // Keep the caller's extension: the decoder probes by it.
        let ext = filename
            .as_deref()
            .and_then(|f| std::path::Path::new(f).extension())
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty())
            .unwrap_or("wav");
        let tmp = std::env::temp_dir().join(format!(
            "senclaw-asr-{}-{}.{ext}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&tmp, &audio)?;
        let out = if timestamps {
            engine
                .transcribe_file_timed(&tmp, language.as_deref())
                .map(|(text, stats)| {
                    json!({
                        "text": text,
                                                "decode_ms": stats.decode_ms,
                        "audio_secs": stats.audio_secs,
                    })
                })
        } else {
            engine
                .transcribe_file(&tmp, language.as_deref())
                .map(|text| json!({ "text": text }))
        };
        let _ = std::fs::remove_file(&tmp);
        out
    })
    .await;

    match result {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err(e)) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        Err(e) => err(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("transcription task panicked: {e}"),
        ),
    }
}
