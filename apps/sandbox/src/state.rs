//! Shared process state for axum handlers.

use crate::db::Db;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    /// MCP SSE fan-out channel.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}
