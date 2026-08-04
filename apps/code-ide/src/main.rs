mod api;
mod db;
mod deepwiki;
mod llm;
mod mcp;
mod pty;
mod watch;
mod workspace;

use axum::Router;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

/// Locate DeepWiki's built web UI (served at `/deepwiki`). Dev: the standalone
/// app's dist; release: `deepwiki_dist` next to the binary.
fn deepwiki_dist(exe_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    [
        std::path::PathBuf::from("apps/code-ide/deepwiki-web/dist"),
        std::path::PathBuf::from("deepwiki-web/dist"),
        std::path::PathBuf::from("deepwiki_dist"),
        exe_dir.join("deepwiki_dist"),
    ]
    .into_iter()
    .find(|c| c.join("index.html").exists())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let port = std::env::var("PORT").unwrap_or_else(|_| "4340".to_string());
    let state = api::make_state();
    let api_router = api::api_router(state);

    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    // Prefer the app's own web build (dev: cwd; release: next to the binary).
    let candidates = [
        std::path::PathBuf::from("web/dist"),
        std::path::PathBuf::from("web_dist"),
        std::path::PathBuf::from("apps/code-ide/web/dist"),
        exe_dir.join("web_dist"),
        exe_dir.join("web").join("dist"),
    ];
    let dist_path = candidates
        .iter()
        .find(|c| c.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("web/dist"));

    let serve_dir =
        ServeDir::new(&dist_path).not_found_service(ServeFile::new(dist_path.join("index.html")));

    // In-process DeepWiki: its Axum router under /api/deepwiki, its UI at /deepwiki.
    let deepwiki_api = deepwiki::api::api_router();
    let mut app = Router::new()
        .nest("/api", api_router)
        .nest("/api/deepwiki", deepwiki_api);

    if let Some(dw_dist) = deepwiki_dist(&exe_dir) {
        let dw_serve =
            ServeDir::new(&dw_dist).not_found_service(ServeFile::new(dw_dist.join("index.html")));
        app = app.nest_service("/deepwiki", dw_serve);
        println!(
            "DeepWiki UI mounted at /deepwiki (from {})",
            dw_dist.display()
        );
    }

    let app = app
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
    println!("SenClaw Code (IDE) running on http://{host}:{port}");
    axum::serve(listener, app).await.unwrap();
}
