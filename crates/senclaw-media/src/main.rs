//! `senclaw-media` — the MLX speech sidecar.
//!
//! Whisper ASR, and only Whisper ASR. It is the last thing SenClaw runs that
//! needs `mlx-rs`, so it is the last thing standing between `make app-build`
//! and a daemon that compiles no MLX at all.
//!
//! ## Why a sidecar and not a Space App
//!
//! A Space App is *installed* — the user chooses it, it can be uninstalled, and
//! the daemon must cope with it being absent. Speech-to-text is not optional in
//! that sense: it backs voice chat and the transcribe endpoint, and a missing
//! app would read as a broken feature rather than an uninstalled one. So this
//! binary ships **beside** the daemon and is spawned on demand, the same way
//! the daemon already spawns its own `*-server` subcommands.
//!
//! ## Why it is a separate binary rather than a module
//!
//! Two reasons, and the second is the one that made it worth doing:
//!
//! - **Build cost.** `mlx-sys` compiles MLX from C++ source. Linking it into
//!   the daemon meant every daemon build paid for it, including the ones that
//!   changed a web route.
//! - **Dead source per platform.** MLX is Apple Silicon only. In the daemon the
//!   whole stack sat behind `#[cfg(feature)]`, present in the tree on every
//!   platform and gated at a hundred call sites. Here the platform gate is one
//!   `#[cfg(target_os)]` on one module: a Linux or Windows build of this
//!   sidecar contains no MLX source and no MLX dependency, and still serves a
//!   coherent (if smaller) API.
//!
//! Everything that does *not* need MLX stayed in the daemon on purpose — TTS is
//! ONNX on the CPU, OCR is MNN. Splitting those out would have bought nothing
//! and cost a process hop per call.

mod audio;
mod candle_whisper;
#[cfg(target_os = "macos")]
mod mlx;
mod routes;

use axum::{routing::get, Json, Router};
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port = std::env::var("PORT").unwrap_or_else(|_| "18790".to_string());

    let app = Router::new()
        .route("/health", get(health))
        .merge(routes::router())
        .layer(CorsLayer::permissive())
        // Audio uploads: a few minutes of WAV is tens of megabytes, and axum's
        // 2 MB default would reject a transcription that worked in-process.
        .layer(axum::extract::DefaultBodyLimit::max(256 * 1024 * 1024));

    // Loopback only. This sidecar authenticates nothing — the daemon is its
    // only caller, over 127.0.0.1 — so a wildcard bind would hand transcription
    // (and the file paths in its requests) to anyone on the LAN.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    tracing::info!("senclaw-media listening on http://{host}:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Answers before any weights are read.
///
/// The daemon waits on this to decide the sidecar came up. A Whisper checkpoint
/// takes seconds to load; making the probe wait for it would report a working
/// process as a failed launch.
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        // Every build transcribes now: MLX on Apple Silicon, Candle on the
        // CPU everywhere else. Reported so the daemon can display which.
        "asr": true,
        "asr_backend": if cfg!(target_os = "macos") { "mlx" } else { "candle" },
    }))
}
