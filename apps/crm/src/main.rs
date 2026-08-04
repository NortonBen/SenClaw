// The `tools_list()` MCP tool catalogue is one giant `json!` literal that
// nests deeper as more tools are added. Bump past the compiler's 128-arm
// default so the macro expands cleanly.
#![recursion_limit = "512"]

mod api;
mod api_dashboard;
mod api_inbox;
mod api_org;
mod api_sale;
mod channels;
mod db;
mod db_dashboard;
mod db_inbox;
mod db_org;
mod db_sale;
mod guardrail;
mod llm;
mod mcp;
mod mcp_ext;
mod sale;
mod senclaw;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4390".to_string());
    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // Prefer the app's own web build. App-specific and packaged (`web_dist`) paths
    // come FIRST; the generic cwd `web/dist` is checked LAST so that running the
    // binary from the repo root doesn't pick up SenClaw's own `web/dist` (the
    // "SenClaw Connect" main UI) — the static-dir collision gotcha.
    let candidates = [
        std::path::PathBuf::from("apps/crm/web/dist"), // dev: cargo run from repo root
        std::path::PathBuf::from("web_dist"),          // packaged: flat install cwd
        exe_dir.join("web_dist"),                      // packaged: next to the binary
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

    let app = Router::new()
        .nest("/api", api_router)
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
    println!("SenClaw CRM running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
