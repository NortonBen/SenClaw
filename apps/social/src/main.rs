// The MCP `tools_list()` json! literal can exceed the default recursion budget.
#![recursion_limit = "512"]

mod api;
mod cadence;
mod channels;
mod config;
mod db;
mod extbridge;
mod gate;
mod mcp;
mod state;
mod web_ops;

use axum::response::IntoResponse;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let core = state::Core::boot().expect("boot core");
    let (mcp_tx, _) = tokio::sync::broadcast::channel(64);
    let ext = extbridge::ExtBridge::new();

    // Persist a login/session history from extension heartbeat transitions.
    {
        let db = core.db.clone();
        ext.set_hosts_change_handler(move |came_online, went_offline| {
            for p in &came_online {
                db.log_session_event(p, "online");
            }
            for p in &went_offline {
                db.log_session_event(p, "offline");
            }
        });
    }
    let app_state = state::AppState {
        core,
        mcp_tx,
        ext: ext.clone(),
        cadence: Arc::new(cadence::Cadence::new()),
    };

    // Dedicated WS server the shared Chrome extension dials.
    let ext_port = config::ext_ws_port();
    tokio::spawn(extbridge::serve_ws(ext, ext_port));

    let api_router = api::api_router(app_state.clone());
    let root_router = api::root_router(app_state);

    // App-specific and packaged (`web_dist`) paths come FIRST; the generic cwd
    // `web/dist` is checked LAST so running from the repo root doesn't pick up
    // SenClaw's own `web/dist`.
    let exe_dir = std::env::current_exe()
        .map(|p| p.parent().unwrap().to_path_buf())
        .unwrap_or_default();
    let candidates = [
        std::path::PathBuf::from("apps/social/web/dist"),
        std::path::PathBuf::from("web_dist"),
        exe_dir.join("web_dist"),
        exe_dir.join("web").join("dist"),
        std::path::PathBuf::from("web/dist"),
    ];
    let dist_path = candidates
        .iter()
        .find(|c| c.join("index.html").exists())
        .cloned()
        .unwrap_or_else(|| std::path::PathBuf::from("web/dist"));

    let index_path = dist_path.join("index.html");
    let spa_index = axum::routing::get(move || {
        let p = index_path.clone();
        async move {
            match tokio::fs::read(&p).await {
                Ok(bytes) => (
                    [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    bytes,
                )
                    .into_response(),
                Err(_) => (
                    axum::http::StatusCode::NOT_FOUND,
                    "Social UI chưa được build (thiếu web_dist/index.html)",
                )
                    .into_response(),
            }
        }
    });
    let serve_dir = ServeDir::new(&dist_path).fallback(spa_index);

    let app = axum::Router::new()
        .nest("/api", api_router)
        .merge(root_router)
        .fallback_service(serve_dir)
        .layer(CorsLayer::permissive());

    let port = config::http_port();
    // Loopback by default. A Space App authenticates nothing of its own — the
    // daemon reaches it over 127.0.0.1 and the UI is same-origin — so binding
    // 0.0.0.0 hands the whole REST + MCP surface to anyone on the LAN. Set
    // SENCLAW_BIND_HOST=0.0.0.0 to opt in to that explicitly.
    let host = std::env::var("SENCLAW_BIND_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let listener = tokio::net::TcpListener::bind(format!("{host}:{port}"))
        .await
        .unwrap();
    println!("SenClaw Social running on http://{host}:{port}");
    println!("  data: {}", config::data_dir().display());
    println!("  ext:  ws://{host}:{ext_port}");
    println!("  ui:   {}", dist_path.display());
    axum::serve(listener, app).await.unwrap();
}
