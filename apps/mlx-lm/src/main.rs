//! MLX local-model Space App — Apple Silicon.
//!
//! Serves an OpenAI surface backed by the in-process `mlx-rs` engine, plus the
//! model-management screen that used to live in the daemon.
//!
//! ## The startup rule this app is shaped around
//!
//! **The port binds and `/health` answers before a single byte of weights is
//! read.** The daemon health-gates a newly spawned app on a 30-second budget
//! with a 5-second probe timeout, and a 4 GB checkpoint does not load inside
//! that. An app that loads in `main` is reported as failing to start, with
//! nothing in the message to say that loading was the reason — so the engine is
//! loaded lazily, on the first `/v1/chat/completions`.

mod engine;
mod provider;

use std::sync::Arc;

use axum::{routing::get, Json, Router};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use provider::MlxProvider;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4832".to_string());
    let provider = Arc::new(MlxProvider::new());

    // Publish the model list before serving. This is what lets the daemon
    // register this app's models while it is stopped — which it is, most of the
    // time. Without the cache nothing would ever select one of these models, so
    // nothing would call the app, so it would never start.
    if let Err(e) = provider.publish() {
        tracing::warn!("model list not published: {e}");
    }
    provider.spawn_idle_sweeper();

    let app = Router::new()
        .route("/health", get(health))
        .merge(app_space_sdk::llm::openai_router(Arc::clone(&provider)))
        .merge(local_model_core::api::router(Arc::clone(&provider)))
        .fallback_service(web_dir())
        .layer(CorsLayer::permissive());

    // Loopback by default. A Space App authenticates nothing of its own — the
    // daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
    // 0.0.0.0 hands the whole REST surface to anyone on the LAN. Set
    // SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
    tracing::info!("SenClaw MLX running on http://{host}:{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Answers immediately, whether or not any weights are loaded. A cold engine is
/// a healthy engine — this endpoint says the process is up, nothing more, and
/// making it wait for a load is what turns a working app into one the daemon
/// reports as failing to start.
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

fn web_dir() -> ServeDir<tower_http::set_status::SetStatus<ServeFile>> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/mlx-lm/web"),
        exe_dir.join("web"),
        std::path::PathBuf::from("web"),
    ];
    let dir = candidates
        .iter()
        .find(|c| c.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("web"));
    ServeDir::new(&dir).not_found_service(ServeFile::new(dir.join("index.html")))
}
