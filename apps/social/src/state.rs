//! Shared application state.

use crate::cadence::Cadence;
use crate::config;
use crate::db::Db;
use crate::extbridge::ExtBridge;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::broadcast;

pub struct Core {
    pub db: Db,
}

impl Core {
    pub fn boot() -> Result<Arc<Core>> {
        let dir = config::data_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("tạo thư mục dữ liệu {}", dir.display()))?;
        let db = Db::open(&config::db_path()).context("mở SQLite")?;
        Ok(Arc::new(Core { db }))
    }
}

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<Core>,
    pub mcp_tx: broadcast::Sender<String>,
    pub ext: ExtBridge,
    pub cadence: Arc<Cadence>,
}
