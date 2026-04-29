//! SenClaw — multi-group AI gateway (Rust port).
//!
//! Module layout mirrors the original TypeScript tree under `src-old/`.
//! The daemon boot sequence (`run_daemon`) follows `src-old/index.ts`.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;

pub mod agent;
pub mod channels;
pub mod clawhub;
pub mod cli;
pub mod config;
pub mod db;
pub mod gateway;
pub mod mcp;
pub mod memory;
pub mod scheduler;
pub mod setup;
pub mod skills;
pub mod subagents;
pub mod types;
pub mod tools;
pub mod util;
pub mod wiki;
pub mod zen_core;

use channels::Channel;

/// Boot the SenClaw daemon. Mirrors `src-old/index.ts`.
///
/// Startup sequence:
///   1. SQLite init (WAL, schema, memory tables)
///   2. GroupManager — load group bindings from DB + config.json
///   3. Channel adapters connect (Telegram → Feishu → QQ → WeChat)
///   4. AgentPool + GroupQueue + MessageRouter — blocked by sema-core
///   5. TaskScheduler — wired for standalone task execution
///   6. DispatchBridge, PersonaRegistry, VirtualWorkerPool
///   7. WebSocketGateway + UIServer — axum server
///   8. WikiManager + builtin personas
///   9. Graceful shutdown on SIGINT/SIGTERM
// ===== RealWsApi: bridges WS messages → GroupQueue → AgentPool =====

struct RealWsApi {
    group_queue: Arc<agent::group_queue::GroupQueue>,
    agent_pool: Arc<agent::agent_pool::AgentPool>,
}

struct RealPermissionApi {
    agent_pool: Arc<agent::agent_pool::AgentPool>,
}

impl agent::permission_bridge::PermissionBridgeApi for RealPermissionApi {
    fn is_web_jid(&self, chat_jid: &str) -> bool {
        chat_jid.starts_with("web:")
    }

    fn respond_to_tool_permission(&self, group_jid: &str, tool_name: &str, selected: &str) {
        self.agent_pool
            .respond_to_tool_permission(group_jid, tool_name, selected);
    }

    fn respond_to_ask_question(
        &self,
        group_jid: &str,
        agent_id: &str,
        answers: HashMap<String, String>,
    ) {
        self.agent_pool
            .respond_to_ask_question(group_jid, agent_id, answers);
    }
}

#[async_trait]
impl gateway::websocket_gateway::WsGatewayApi for RealWsApi {
    fn enqueue_and_process(&self, group_jid: &str, group: &crate::types::GroupBinding, text: &str) {
        let agent_pool = Arc::clone(&self.agent_pool);
        let jid = group_jid.to_string();
        let g = group.clone();
        let t = text.to_string();
        let gq = Arc::clone(&self.group_queue);
        let jid_key = jid.clone();
        tokio::spawn(async move {
            gq.enqueue(&jid_key, Box::pin(async move {
                let _ = gateway::message_router::AgentApi::process_and_wait(
                    agent_pool.as_ref(),
                    &jid,
                    &g,
                    &t,
                ).await;
            })).await;
        });
    }

    fn pause_agent(&self, group_jid: &str) {
        self.agent_pool.pause_agent(group_jid);
    }

    fn resolve_permission(&self, request_id: &str, option_key: &str) {
        let _ = self.agent_pool.resolve_permission(request_id, option_key);
    }

    fn resolve_ask_question(
        &self,
        request_id: &str,
        answers: &serde_json::Value,
        other_texts: Option<&serde_json::Value>,
    ) {
        let _ = self
            .agent_pool
            .resolve_ask_question_batch(request_id, answers, other_texts);
    }

    fn resume_agent(&self, group_jid: &str, query: Option<&str>) {
        self.agent_pool.resume_agent(group_jid, query);
    }

    async fn stop_agent(&self, group_jid: &str) {
        self.agent_pool.stop_agent(group_jid).await;
    }

    /// Snapshot of all dispatch parents — sent to admin clients on subscribe.
    fn get_dispatch_parents(&self) -> serde_json::Value {
        let bridge = self.agent_pool.dispatch_bridge_snapshot();
        let parents = match bridge {
            Some(b) => b.get_parents(),
            None => Vec::new(),
        };
        serde_json::to_value(
            parents
                .iter()
                .map(dispatch_parent_to_json)
                .collect::<Vec<_>>(),
        )
        .unwrap_or(serde_json::Value::Null)
    }

    /// Snapshot of cached agent todos — sent to admin clients on subscribe.
    fn get_agent_todos(&self) -> serde_json::Value {
        let cached = self.agent_pool.get_all_cached_todos();
        let map: serde_json::Map<String, serde_json::Value> = cached
            .into_iter()
            .map(|(jid, entry)| (jid, serde_json::to_value(entry).unwrap_or(serde_json::Value::Null)))
            .collect();
        serde_json::Value::Object(map)
    }

    /// Snapshot of per-agent tool rosters — sent to admin clients on subscribe
    /// so the Agent Console can render currently-online agents and their tools.
    fn get_agent_tools(&self) -> serde_json::Value {
        let cached = self.agent_pool.get_all_cached_tools();
        let map: serde_json::Map<String, serde_json::Value> = cached
            .into_iter()
            .map(|(jid, entry)| (jid, serde_json::to_value(entry).unwrap_or(serde_json::Value::Null)))
            .collect();
        serde_json::Value::Object(map)
    }
}

fn dispatch_parent_to_json(p: &agent::dispatch_bridge::DispatchParent) -> serde_json::Value {
    serde_json::json!({
        "id": p.id,
        "goal": p.goal,
        "adminFolder": p.admin_folder,
        "status": p.status,
        "tasks": p.tasks.iter().map(|t| serde_json::json!({
            "id": t.id,
            "label": t.label,
            "agentId": t.agent_id,
            "prompt": t.prompt,
            "status": t.status.label(),
        })).collect::<Vec<_>>(),
    })
}

// ===== WsAgentEventSink: forwards AgentPool events → WebSocket gateway =====

struct WsAgentEventSink {
    gateway: Arc<gateway::websocket_gateway::WebSocketGateway>,
}

impl agent::agent_pool::AgentEventSink for WsAgentEventSink {
    fn notify_agent_reply(&self, chat_jid: &str, text: &str) {
        let gw = Arc::clone(&self.gateway);
        let jid = chat_jid.to_string();
        let text = text.to_string();
        tokio::spawn(async move { gw.notify_agent_reply(&jid, &text).await; });
    }

    fn notify_agent_state(&self, chat_jid: &str, state: &str) {
        let gw = Arc::clone(&self.gateway);
        let jid = chat_jid.to_string();
        let state = state.to_string();
        tokio::spawn(async move { gw.notify_agent_state(&jid, &state).await; });
    }

    fn notify_permission_request(
        &self,
        chat_jid: &str,
        request_id: &str,
        payload: agent::permission_bridge::PermissionPayload,
    ) {
        let gw = Arc::clone(&self.gateway);
        let jid = chat_jid.to_string();
        let req = request_id.to_string();
        let payload = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
        tokio::spawn(async move {
            gw.notify_permission_request(&jid, &req, &payload).await;
        });
    }

    fn notify_ask_question_request(
        &self,
        chat_jid: &str,
        request_id: &str,
        payload: agent::permission_bridge::AskQuestionPayload,
    ) {
        let gw = Arc::clone(&self.gateway);
        let jid = chat_jid.to_string();
        let req = request_id.to_string();
        let payload = serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null);
        tokio::spawn(async move {
            gw.notify_ask_question_request(&jid, &req, &payload).await;
        });
    }

    fn notify_permission_resolved(
        &self,
        chat_jid: &str,
        request_id: &str,
        option_key: &str,
        option_label: &str,
    ) {
        let gw = Arc::clone(&self.gateway);
        let jid = chat_jid.to_string();
        let req = request_id.to_string();
        let key = option_key.to_string();
        let label = option_label.to_string();
        tokio::spawn(async move {
            gw.notify_permission_resolved(&jid, &req, &key, &label).await;
        });
    }

    fn notify_ask_question_resolved(
        &self,
        chat_jid: &str,
        request_id: &str,
        answers: std::collections::HashMap<String, String>,
    ) {
        let gw = Arc::clone(&self.gateway);
        let jid = chat_jid.to_string();
        let req = request_id.to_string();
        let answers = serde_json::to_value(&answers).unwrap_or(serde_json::Value::Null);
        tokio::spawn(async move {
            gw.notify_ask_question_resolved(&jid, &req, &answers).await;
        });
    }

    fn notify_agent_todos(
        &self,
        agent_jid: &str,
        agent_name: &str,
        todos: &[agent::agent_pool::TodoSnapshot],
    ) {
        let gw = Arc::clone(&self.gateway);
        let jid = agent_jid.to_string();
        let name = agent_name.to_string();
        let todos = serde_json::to_value(todos).unwrap_or(serde_json::Value::Null);
        tokio::spawn(async move {
            gw.notify_agent_todos(&jid, &name, &todos).await;
        });
    }

    fn notify_agent_compacting(&self, chat_jid: &str, is_compacting: bool) {
        let gw = Arc::clone(&self.gateway);
        let jid = chat_jid.to_string();
        tokio::spawn(async move {
            gw.notify_agent_compacting(&jid, is_compacting).await;
        });
    }

    fn notify_agent_tools(
        &self,
        agent_jid: &str,
        agent_name: &str,
        tools: &[agent::agent_pool::AgentToolInfo],
    ) {
        let gw = Arc::clone(&self.gateway);
        let jid = agent_jid.to_string();
        let name = agent_name.to_string();
        let tools = serde_json::to_value(tools).unwrap_or(serde_json::Value::Null);
        tokio::spawn(async move {
            gw.notify_agent_tools(&jid, &name, &tools).await;
        });
    }
}

pub async fn run_daemon(cfg: config::Config) -> Result<()> {
    // ===== 0. Setup wizard =====
    setup::run_setup_if_needed(&cfg.paths.global_config_path);

    tracing::info!("[SenClaw] Starting...");

    // ===== 1. Database =====
    let db = Arc::new(db::Db::open(&cfg).context("open database")?);
    tracing::info!("[SenClaw] DB initialized: {}", cfg.paths.db_path.display());

    // ===== 2. GroupManager =====
    // Load group bindings from DB; reconcile with config.json
    let gm = Arc::new(gateway::group_manager::GroupManager::new());
    // Sync groups from config.json into DB on startup
    let (sync_added, sync_updated, sync_removed) =
        gateway::group_manager::sync_groups_from_config(&db, &gm, &cfg);
    if sync_added > 0 || sync_updated > 0 || sync_removed > 0 {
        tracing::info!(
            "[SenClaw] Group sync: +{sync_added} added, ~{sync_updated} updated, -{sync_removed} removed"
        );
    }
    let groups = db.list_groups()?;
    tracing::info!("[SenClaw] GroupManager: {} group(s) loaded", groups.len());

    // ===== 2b. PersonaRegistry =====
    let persona_registry = {
        let reg = agent::persona_registry::PersonaRegistry::new(
            cfg.paths.virtual_agents_dir.clone(),
        );
        let reg = Arc::new(std::sync::Mutex::new(reg));
        // Spawn file watcher for hot-reload
        agent::persona_registry::PersonaRegistry::spawn_watcher(Arc::clone(&reg));
        reg
    };
    tracing::info!(
        "[SenClaw] PersonaRegistry: {} persona(s) loaded",
        persona_registry.lock().unwrap().list().len()
    );

    // ===== 3. Channel adapters =====
    let mut channels: Vec<Box<dyn channels::Channel>> = Vec::new();

    // 3a. Telegram
    let mut tg = channels::telegram::TelegramChannel::new(cfg.telegram.bot_token.clone());
    match tg.connect().await {
        Ok(()) => {
            if tg.is_connected() {
                tracing::info!("[SenClaw] TelegramChannel connected");
            } else {
                tracing::warn!(
                    "[SenClaw] TelegramChannel not connected (token missing or invalid)"
                );
            }
        }
        Err(e) => {
            tracing::error!("[SenClaw] TelegramChannel connect failed, continuing without Telegram: {e}");
        }
    }
    channels.push(Box::new(tg));

    // 3b. Feishu
    if !cfg.feishu.app_id.is_empty() && !cfg.feishu.app_secret.is_empty() {
        let mut feishu = channels::feishu::FeishuChannel::new(
            cfg.feishu.app_id.clone(),
            cfg.feishu.app_secret.clone(),
            Some(cfg.feishu.domain.clone()),
        );
        match feishu.connect().await {
            Ok(()) => {
                if feishu.is_connected() {
                    tracing::info!("[SenClaw] FeishuChannel connected");
                }
            }
            Err(e) => {
                tracing::error!("[SenClaw] FeishuChannel connect failed, continuing without Feishu: {e}");
            }
        }
        channels.push(Box::new(feishu));
    } else {
        tracing::info!("[SenClaw] FeishuChannel: no credentials configured, skipped");
    }

    // 3c. QQ
    if !cfg.qq.app_id.is_empty() && !cfg.qq.app_secret.is_empty() {
        let mut qq = channels::qq::QQChannel::new(
            cfg.qq.app_id.clone(),
            cfg.qq.app_secret.clone(),
            cfg.qq.sandbox,
        );
        match qq.connect().await {
            Ok(()) => {
                if qq.is_connected() {
                    tracing::info!("[SenClaw] QQChannel connected");
                }
            }
            Err(e) => {
                tracing::error!("[SenClaw] QQChannel connect failed, continuing without QQ: {e}");
            }
        }
        channels.push(Box::new(qq));
    } else {
        tracing::info!("[SenClaw] QQChannel: no credentials configured, skipped");
    }

    // 3d. WeChat
    if cfg.wechat.enabled {
        let mut wx = channels::wechat::WeChatChannel::new(
            "default".to_string(),
            Some(cfg.wechat.api_base_url.clone()),
        );
        match wx.connect().await {
            Ok(()) => {
                if wx.is_connected() {
                    tracing::info!("[SenClaw] WeChatChannel connected");
                }
            }
            Err(e) => {
                tracing::error!("[SenClaw] WeChatChannel connect failed, continuing without WeChat: {e}");
            }
        }
        channels.push(Box::new(wx));
    } else {
        tracing::info!("[SenClaw] WeChatChannel: not enabled, skipped");
    }

    let connected_count = channels.iter().filter(|ch| ch.is_connected()).count();
    if connected_count == 0 {
        tracing::warn!("[SenClaw] No channels are connected; running in WebUI-only mode.");
    } else {
        tracing::info!("[SenClaw] {connected_count} channel(s) connected");
    }

    // ===== 3e. ensure admin group =====
    // Creates admin group (JID depends on configured admin IDs — Telegram > Feishu > web:main).
    // Must happen after channels connect so bot userId is known.
    gateway::group_manager::ensure_admin_group(&db, &gm, &cfg, None);
    tracing::info!("[SenClaw] Admin group ensured");

    // ===== 4. GroupQueue + AgentPool =====
    let group_queue = agent::group_queue::GroupQueue::new(cfg.agent.max_concurrent);
    let agent_pool =
        agent::agent_pool::AgentPool::new(Arc::new(agent::agent_pool::ZenCoreApi::new()));
    agent_pool.set_db(Arc::clone(&db));
    agent_pool.set_config(Arc::new(cfg.clone()));
    agent_pool.set_dispatch_bridge(Arc::new(agent::dispatch_bridge::NoopDispatchBridge));
    agent_pool.set_permission_bridge(Arc::new(agent::permission_bridge::PermissionBridge::new(
        Arc::new(RealPermissionApi {
            agent_pool: agent_pool.clone(),
        }),
        None,
    )));
    tracing::info!(
        "[SenClaw] AgentPool (zen-core engine) + GroupQueue (max_concurrent={}) ready",
        cfg.agent.max_concurrent
    );

    // ===== 5. WebSocketGateway + UIServer =====
    // WS and UI listen on separate ports (matching TS config).

    // 5a. WebSocket gateway
    let ws_gateway = {
        let ws_api = Arc::new(RealWsApi {
            group_queue: Arc::clone(&group_queue),
            agent_pool: agent_pool.clone(),
        });

        let ws_state = Arc::new(gateway::websocket_gateway::WsState {
            config: Arc::new(cfg.clone()),
            db: Arc::clone(&db),
            group_manager: Arc::clone(&gm),
            api: ws_api,
        });

        let gw = Arc::new(gateway::websocket_gateway::WebSocketGateway::new(
            cfg.ws_port,
            cfg.ui_server.ws_token.clone(),
        ));

        // Wire full event sink: AgentPool → WebSocket gateway.
        // Forwards reply / state / todos / permission / ask-question events,
        // populating the gateway's last-known state map so newly subscribed
        // clients (Agent Console) see currently-running agents.
        agent_pool.set_agent_event_sink(Arc::new(WsAgentEventSink {
            gateway: Arc::clone(&gw),
        }));

        let ws_router = gw.route(ws_state);
        let ws_port = cfg.ws_port;
        let ws_addr = format!("127.0.0.1:{ws_port}");
        tracing::info!("[SenClaw] WebSocket gateway at ws://{ws_addr}");
        tokio::spawn(async move {
            let listener = match tokio::net::TcpListener::bind(&ws_addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("[SenClaw] WS bind {ws_addr}: {e}");
                    return;
                }
            };
            if let Err(e) = axum::serve(listener, ws_router).await {
                tracing::error!("[SenClaw] WS server error: {e}");
            }
        });
        gw
    };

    // 7b. WikiManager
    let wiki_mgr = Arc::new(wiki::manager::WikiManager::new(cfg.paths.wiki_dir.clone()));
    if let Err(e) = wiki_mgr.ensure_init().await {
        tracing::warn!("[SenClaw] Wiki init failed (non-fatal): {e}");
    } else {
        tracing::info!("[SenClaw] WikiManager initialized: {}", cfg.paths.wiki_dir.display());
    }

    // 7c. UI HTTP server
    {
        struct StubUiApi;
        impl gateway::ui_server::UiApi for StubUiApi {}

        let ui_state = Arc::new(gateway::ui_server::UiState {
            config: Arc::new(cfg.clone()),
            wiki_manager: Some(Arc::clone(&wiki_mgr)),
            persona_registry: Some(Arc::clone(&persona_registry)),
            agent_api: Some(Arc::new(StubUiApi)),
            ws_port: cfg.ws_port,
            ws_token: cfg.ui_server.ws_token.clone().unwrap_or_default(),
        });

        let ui_router = gateway::ui_server::build_router(ui_state);
        let http_port = cfg.ui_server.port;
        let http_addr = format!("127.0.0.1:{http_port}");
        let listener = tokio::net::TcpListener::bind(&http_addr)
            .await
            .with_context(|| format!("bind {http_addr}"))?;
        tracing::info!("[SenClaw] Web UI at http://{http_addr}");
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, ui_router).await {
                tracing::error!("[SenClaw] UI server error: {e}");
            }
        });
    }

    // 7d. Builtin personas
    subagents::builtin_personas::install_builtin_personas(&cfg.paths.virtual_agents_dir);

    // ===== 9. Graceful shutdown =====
    tracing::info!("[SenClaw] Daemon running. Press Ctrl-C to stop.");

    tokio::signal::ctrl_c().await.ok();
    tracing::info!("[SenClaw] Shutting down...");

    // Disconnect all channels
    for ch in channels.iter_mut() {
        let id = ch.id();
        if let Err(e) = ch.disconnect().await {
            tracing::warn!("[SenClaw] Error disconnecting {id}: {e}");
        }
    }

    // Drop ws_gateway to close all client connections
    drop(ws_gateway);

    tracing::info!("[SenClaw] Goodbye.");
    Ok(())
}
