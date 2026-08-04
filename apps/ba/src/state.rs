use crate::db::Db;
use crate::engine::Jobs;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub jobs: Jobs,
    /// Kênh đẩy JSON-RPC reply lên SSE cho MCP transport http.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

impl AppState {
    pub fn new(db: Db) -> Self {
        let (mcp_tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            db,
            jobs: Jobs::default(),
            mcp_tx,
        }
    }
}
