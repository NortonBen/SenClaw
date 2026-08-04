mod api;
mod db;
mod editor;
mod llm;
mod mcp;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4610".to_string());
    let state = api::make_state();
    editor::spawn_ensure(state.clone());
    let api_router = api::api_router(state.clone());

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // Prefer the app's own web build. App-specific and packaged (`web_dist`) paths
    // come FIRST; the generic cwd `web/dist` is checked LAST so that running the
    // binary from the repo root doesn't pick up SenClaw's own `web/dist` (the
    // "Senclaw Connect" main UI) — the static-dir collision gotcha.
    let candidates = [
        std::path::PathBuf::from("apps/drawio/web/dist"), // dev: cargo run from repo root
        std::path::PathBuf::from("web_dist"),             // packaged: flat install cwd
        exe_dir.join("web_dist"),                         // packaged: next to the binary
        exe_dir.join("web").join("dist"),
        std::path::PathBuf::from("web/dist"), // last resort (may collide)
    ];
    let dist_path = candidates
        .iter()
        .find(|c| c.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("web/dist"));

    let serve_dir =
        ServeDir::new(&dist_path).not_found_service(ServeFile::new(dist_path.join("index.html")));

    // The draw.io editor webapp is downloaded on first run (draw.war is ~53MB —
    // too big for the install zip) and then served same-origin so the embed
    // iframe + postMessage protocol need no external host.
    let editor_dir = editor::webapp_dir();

    let app = Router::new()
        .nest("/api", api_router)
        .nest_service("/drawio", ServeDir::new(&editor_dir))
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    // Loopback by default. A Space App authenticates nothing of its own — the
    // daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
    // 0.0.0.0 hands the whole REST + MCP surface to anyone on the LAN. Set
    // SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw Diagrams (draw.io) running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
