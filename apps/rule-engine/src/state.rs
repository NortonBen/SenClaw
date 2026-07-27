//! Boot wiring: database → services → registry → engine.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::broadcast;

use crate::db::Db;
use crate::engine::registry::Registry;
use crate::engine::services::{EventBus, Services};
use crate::engine::Engine;
use crate::model::ChainStatus;

pub struct AppState {
    pub db: Arc<Db>,
    pub engine: Arc<Engine>,
    /// Mirrors MCP JSON-RPC replies to any attached SSE client.
    pub mcp_tx: broadcast::Sender<String>,
}

pub fn boot() -> Result<Arc<AppState>> {
    let db = Arc::new(Db::open(&crate::config::db_path())?);
    let bus = EventBus::new();
    let svc = Arc::new(Services::new(db.clone(), bus));

    let mut registry = Registry::new();
    crate::rules::register(&mut registry);
    let engine = Engine::start(Arc::new(registry), svc);

    let (mcp_tx, _) = broadcast::channel(128);
    Ok(Arc::new(AppState { db, engine, mcp_tx }))
}

/// Deploy every chain marked ACTIVE.
///
/// Runs *after* the HTTP listener is up: the daemon health-gates a Space App
/// for 30s and a slow source (a cron with a far-off first tick, a flaky HTTP
/// dependency) must not push us past it.
pub async fn resume_active_chains(state: &Arc<AppState>) {
    let chains = match state.db.list_chains() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[rule-engine] không đọc được danh sách luồng: {e}");
            return;
        }
    };
    for chain in chains
        .into_iter()
        .filter(|c| c.status == ChainStatus::Active)
    {
        let nodes = state.db.list_nodes(chain.id).unwrap_or_default();
        let edges = state.db.list_edges(chain.id).unwrap_or_default();
        match state.engine.deploy(&chain, &nodes, &edges).await {
            Ok(_) => {}
            Err(e) => {
                eprintln!("[rule-engine] luồng `{}` không nạp được: {e}", chain.name);
                let _ = state.db.set_chain_status(chain.id, ChainStatus::Error);
                state.db.insert_log(&crate::model::LogRow {
                    id: 0,
                    chain_id: chain.id,
                    run_id: None,
                    level: "error".into(),
                    node: None,
                    message: format!("nạp lại khi khởi động thất bại: {e}"),
                    ts: crate::engine::types::now_ms(),
                });
            }
        }
    }
}

/// Periodic housekeeping so the trace tables stay bounded.
pub fn spawn_janitor(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(600));
        loop {
            tick.tick().await;
            let _ = state.db.prune_runs(200);
            let _ = state.db.prune_logs(5000);
        }
    });
}
