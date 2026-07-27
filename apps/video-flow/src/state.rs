//! Shared process state. `Core` is everything below the agent layer (DB, the
//! two bridges, dirs); `AppState` adds the agent pool + DAG engine handles and
//! is what axum handlers receive.

use crate::dashws::DashHub;
use crate::db::Db;
use crate::extbridge::ExtBridge;
use std::path::PathBuf;
use std::sync::Arc;

pub struct Core {
    pub db: Db,
    pub dash: DashHub,
    pub ext: ExtBridge,
    pub souls_dir: PathBuf,
    pub playbooks_dir: PathBuf,
    pub media_dir: PathBuf,
}

impl Core {
    pub fn boot() -> anyhow::Result<Arc<Core>> {
        // Rescue data written by an older build that kept it in the install dir
        // (which every zip install wipes) before opening the DB.
        crate::config::migrate_legacy_data_dir();
        let data_dir = crate::config::data_dir();
        std::fs::create_dir_all(&data_dir).ok();
        let db = Db::open(&crate::config::db_path())?;
        let media_dir = crate::config::media_dir();
        std::fs::create_dir_all(&media_dir).ok();
        // Seed the LLM profile for the SenClaw bridge from app_kv.
        crate::llm::set_profile(&db.kv_get("llm.profile"));
        let core = Arc::new(Core {
            db,
            dash: DashHub::new(),
            ext: ExtBridge::new(),
            souls_dir: crate::config::souls_dir(),
            playbooks_dir: crate::config::playbooks_dir(),
            media_dir,
        });
        Ok(core)
    }
}

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<Core>,
    pub pool: Arc<crate::agents::Pool>,
    pub engine: Arc<crate::dag::Engine>,
    /// MCP SSE fan-out channel.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}
