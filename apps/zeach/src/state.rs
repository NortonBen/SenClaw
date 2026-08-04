//! Shared application state.

use crate::config;
use crate::db::Db;
use crate::sources::discover::{self, Detection, Suggestion};
use crate::sources::mcp_source::{McpSource, McpTarget};
use crate::sources::{
    corpus::CorpusSource, knowledge::KnowledgeSource, web::WebSource, wiki::WikiSource, Registry,
    SourceOrigin,
};
use crate::transport::Transports;
use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};

pub struct Core {
    pub db: Db,
    pub transports: Arc<Transports>,
    pub registry: RwLock<Registry>,
    /// Search tools found by rule but needing user-supplied arguments —
    /// rebuilt on every sync, served by `zeach_source_templates`.
    pub discovered_suggestions: RwLock<Vec<Suggestion>>,
}

impl Core {
    pub fn boot() -> Result<Arc<Core>> {
        let dir = config::data_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("tạo thư mục dữ liệu {}", dir.display()))?;
        let db = Db::open(&config::db_path()).context("mở SQLite")?;
        let transports = Transports::from_config();

        // Core tier: these four always exist — everything else is optional and
        // appears only when the matching app/MCP is installed.
        let mut registry = Registry::new();
        registry.register(
            Arc::new(WebSource::new(transports.browser.clone())),
            SourceOrigin::Builtin,
        );
        registry.register(
            Arc::new(KnowledgeSource::new(transports.core.clone())),
            SourceOrigin::Builtin,
        );
        registry.register(
            Arc::new(WikiSource::new(transports.core.clone())),
            SourceOrigin::Builtin,
        );
        registry.register(Arc::new(CorpusSource::new(db.clone())), SourceOrigin::Builtin);

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
            discovered_suggestions: RwLock::new(Vec::new()),
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

        if let Some(peers) = &peers {
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
                        self.registry.write().await.register(
                            Arc::new(McpSource::new(spec, self.transports.apps.clone())),
                            SourceOrigin::Preset,
                        );
                        report.push(SourceSyncReport {
                            id,
                            registered: true,
                            reason: "preset".into(),
                        });
                    }
                }
            }
        }

        // Rule-based discovery over every other installed app: any app whose
        // MCP exposes a `*_search` tool becomes a source without a preset.
        // Discovered sources start DISABLED — private corpora (CRM, email…)
        // join the fan-out only when the user opts in; the saved config below
        // re-applies that opt-in on every sync.
        if let Some(peers) = &peers {
            let mut apps: Vec<_> = peers.values().filter(|p| p.enabled).cloned().collect();
            apps.sort_by(|a, b| a.id.cmp(&b.id));

            let probes = apps.iter().map(|app| {
                let transport = self.transports.apps.clone();
                let url = app.rpc_url();
                async move { transport.list_tools(&url, Duration::from_secs(4)).await }
            });
            let tool_lists = futures_util::future::join_all(probes).await;

            let mut suggestions = Vec::new();
            for (app, tools) in apps.iter().zip(tool_lists) {
                // Ids already claimed by a builtin, preset or user source stay
                // theirs; only a previous discovery may be refreshed.
                let claimed = {
                    let reg = self.registry.read().await;
                    reg.get(&app.id)
                        .map(|rs| rs.origin != SourceOrigin::Discovered)
                        .unwrap_or(false)
                };
                if claimed {
                    continue;
                }
                let tools = match tools {
                    Ok(t) => t,
                    // An unreachable MCP is the norm for a stopped app — not
                    // worth a report line per app on every rescan.
                    Err(_) => continue,
                };
                match discover::detect(app, &tools) {
                    Detection::Auto(spec) => {
                        let id = spec.id.clone();
                        let tool = spec.tool.clone();
                        {
                            let mut reg = self.registry.write().await;
                            reg.register(
                                Arc::new(McpSource::new(spec, self.transports.apps.clone())),
                                SourceOrigin::Discovered,
                            );
                            // Opt-in default; overridden below by saved config.
                            reg.set_config(&id, Some(false), None, None, None);
                        }
                        report.push(SourceSyncReport {
                            id,
                            registered: true,
                            reason: format!("tự phát hiện (`{tool}`) — đang tắt, bật trong Cài đặt"),
                        });
                    }
                    Detection::Needs(s) => {
                        report.push(SourceSyncReport {
                            id: s.app_id.clone(),
                            registered: false,
                            reason: format!(
                                "`{}` cần thêm: {} — xem mục “Cần bạn cấu hình thêm”",
                                s.tool,
                                s.required_args
                                    .iter()
                                    .map(|(k, _)| k.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                        });
                        suggestions.push(s);
                    }
                    Detection::Denied(reason) => report.push(SourceSyncReport {
                        id: app.id.clone(),
                        registered: false,
                        reason: reason.to_string(),
                    }),
                    Detection::None => {}
                }
            }
            *self.discovered_suggestions.write().await = suggestions;
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
                        reg.register(
                            Arc::new(McpSource::new(spec, self.transports.apps.clone())),
                            SourceOrigin::User,
                        );
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
