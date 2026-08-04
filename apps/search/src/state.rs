//! Shared application state.

use crate::config;
use crate::db::Db;
use crate::sources::mcp_source::{McpSource, McpTarget};
use crate::sources::{
    corpus::CorpusSource, knowledge::KnowledgeSource, web::WebSource, wiki::WikiSource, Registry,
};
use crate::transport::Transports;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

pub struct Core {
    pub db: Db,
    pub transports: Arc<Transports>,
    pub registry: RwLock<Registry>,
}

impl Core {
    pub fn boot() -> Result<Arc<Core>> {
        let dir = config::data_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("tạo thư mục dữ liệu {}", dir.display()))?;
        let db = Db::open(&config::db_path()).context("mở SQLite")?;
        let transports = Transports::from_config();

        let mut registry = Registry::new();
        registry.register(Arc::new(WebSource::new(transports.browser.clone())));
        registry.register(Arc::new(KnowledgeSource::new(transports.core.clone())));
        registry.register(Arc::new(WikiSource::new(transports.core.clone())));
        registry.register(Arc::new(CorpusSource::new(db.clone())));

        // Re-apply the user's saved per-source tuning. A source that has since
        // disappeared is simply skipped — stale config must not fail boot.
        match db.load_source_config() {
            Ok(rows) => {
                for (id, enabled, weight, max_results, timeout_ms) in rows {
                    registry.set_config(
                        &id,
                        enabled,
                        weight,
                        max_results.map(|v| v as usize),
                        timeout_ms.map(|v| v as u64),
                    );
                }
            }
            Err(e) => eprintln!("[search] không đọc được source_config: {e}"),
        }

        Ok(Arc::new(Core {
            db,
            transports,
            registry: RwLock::new(registry),
        }))
    }

    /// Register every MCP-backed source: built-in presets for peer Space Apps
    /// that are actually installed, plus the user's own registered sources.
    ///
    /// Returns a per-spec report. Registration is the first place a source can
    /// silently vanish, so "app not installed" is *reported*, never just
    /// skipped — the same rule the pipeline applies to failing sources.
    pub async fn sync_mcp_sources(&self) -> Vec<SourceSyncReport> {
        let mut report = Vec::new();

        // Presets are conditional on the peer app existing. If the daemon
        // itself is unreachable we cannot tell "not installed" from "cannot
        // ask", and saying the wrong one would be worse than saying neither.
        let peers = match self.transports.apps.discover().await {
            Ok(p) => Some(p),
            Err(e) => {
                report.push(SourceSyncReport {
                    id: "*".into(),
                    registered: false,
                    reason: format!(
                        "không hỏi được daemon về danh sách app ({e}) — bỏ qua toàn bộ preset"
                    ),
                });
                None
            }
        };

        if let Some(peers) = peers {
            for spec in crate::sources::presets::auto_specs() {
                let app_id = match &spec.target {
                    McpTarget::App { app_id } => app_id.clone(),
                    McpTarget::Url { .. } => String::new(),
                };
                match peers.get(&app_id) {
                    None => report.push(SourceSyncReport {
                        id: spec.id.clone(),
                        registered: false,
                        reason: format!("app `{app_id}` chưa được cài"),
                    }),
                    Some(p) if !p.enabled => report.push(SourceSyncReport {
                        id: spec.id.clone(),
                        registered: false,
                        reason: format!("app `{app_id}` đang tắt"),
                    }),
                    Some(_) => {
                        let id = spec.id.clone();
                        self.registry
                            .write()
                            .await
                            .register(Arc::new(McpSource::new(spec, self.transports.apps.clone())));
                        report.push(SourceSyncReport {
                            id,
                            registered: true,
                            reason: "preset".into(),
                        });
                    }
                }
            }
        }

        // User-registered sources are registered unconditionally — the user
        // asked for them, so a broken one must show up as an unhealthy source
        // rather than disappear from the list.
        match self.db.list_mcp_sources() {
            Ok(rows) => {
                for (spec, enabled) in rows {
                    let id = spec.id.clone();
                    {
                        let mut reg = self.registry.write().await;
                        reg.register(Arc::new(McpSource::new(spec, self.transports.apps.clone())));
                        reg.set_config(&id, Some(enabled), None, None, None);
                    }
                    report.push(SourceSyncReport {
                        id,
                        registered: true,
                        reason: "do người dùng đăng ký".into(),
                    });
                }
            }
            Err(e) => report.push(SourceSyncReport {
                id: "*".into(),
                registered: false,
                reason: format!("không đọc được bảng mcp_sources: {e}"),
            }),
        }

        // Persisted per-source tuning must also apply to sources that only
        // exist after this sync.
        if let Ok(rows) = self.db.load_source_config() {
            let mut reg = self.registry.write().await;
            for (id, enabled, weight, max_results, timeout_ms) in rows {
                reg.set_config(
                    &id,
                    enabled,
                    weight,
                    max_results.map(|v| v as usize),
                    timeout_ms.map(|v| v as u64),
                );
            }
        }

        report
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SourceSyncReport {
    pub id: String,
    pub registered: bool,
    pub reason: String,
}

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<Core>,
    pub mcp_tx: broadcast::Sender<String>,
}
