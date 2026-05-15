//! SenClaw — multi-group AI gateway (Rust port).
//!
//! Module layout mirrors the original TypeScript tree under `src-old/`.
//! The daemon boot sequence (`run_daemon`) follows `src-old/index.ts`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use async_trait::async_trait;

pub mod agent;
pub mod browser;
pub mod code_engine;
pub mod code_graph;
pub mod channels;
pub mod clawhub;
pub mod cli;
pub mod config;
pub mod cowork;
pub mod db;
pub mod gateway;
pub mod local_model;
pub mod marketplace;
pub mod mcp;
pub mod memory;
pub mod proto;
pub mod scheduler;
pub mod setup;
pub mod plugins;
pub mod skills;
pub mod subagents;
pub mod tools;
pub mod types;
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
    /// Pending virtual-agent permission responses: key = "virtual_jid::tool_name"
    virtual_perm_senders: Arc<Mutex<HashMap<String, std::sync::mpsc::SyncSender<String>>>>,
}

impl agent::permission_bridge::PermissionBridgeApi for RealPermissionApi {
    fn is_web_jid(&self, chat_jid: &str) -> bool {
        // virtual: jids are also "web-style" — they broadcast to admins and have no
        // channel buttons, so they follow the same code path as web: jids.
        chat_jid.starts_with("web:") || chat_jid.starts_with("virtual:")
    }

    fn respond_to_tool_permission(&self, group_jid: &str, tool_name: &str, selected: &str) {
        if group_jid.starts_with("virtual:") {
            // Deliver response to the waiting virtual agent thread via mpsc.
            let key = format!("{group_jid}::{tool_name}");
            if let Some(tx) = self.virtual_perm_senders.lock().unwrap().remove(&key) {
                let _: Result<(), std::sync::mpsc::SendError<String>> =
                    tx.send(selected.to_string());
            } else {
                tracing::warn!(
                    "[RealPermissionApi] no waiting sender for virtual permission: jid={group_jid} tool={tool_name}"
                );
            }
            return;
        }
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
    fn enqueue_and_process(&self, group_jid: &str, group: &crate::types::GroupBinding, text: &str, attachments: &[crate::agent::input_builder::ImageAttachment]) {
        let agent_pool = Arc::clone(&self.agent_pool);
        let jid = group_jid.to_string();
        let g = group.clone();
        let t = text.to_string();
        let att = attachments.to_vec();
        let gq = Arc::clone(&self.group_queue);
        let jid_key = jid.clone();
        tokio::spawn(async move {
            gq.enqueue(
                &jid_key,
                Box::pin(async move {
                    let _ =
                        types::AgentApi::process_and_wait_with_images(agent_pool.as_ref(), &jid, &g, &t, &att).await;
                }),
            )
            .await;
        });
    }

    fn pause_agent(&self, group_jid: &str) {
        self.agent_pool.pause_agent(group_jid);
    }

    fn resolve_permission(&self, request_id: &str, option_key: &str) {
        let _ = self.agent_pool.resolve_permission(request_id, option_key);
    }

    fn add_tool_rule(&self, rule: crate::agent::permission_bridge::types::ToolAutoAcceptRule) {
        if let Some(bridge) = self.agent_pool.permission_bridge() {
            bridge.add_rule(rule);
        }
    }

    fn remove_tool_rule(&self, rule_id: &str) {
        if let Some(bridge) = self.agent_pool.permission_bridge() {
            bridge.remove_rule(rule_id);
        }
    }

    fn update_tool_rule(&self, rule: crate::agent::permission_bridge::types::ToolAutoAcceptRule) {
        if let Some(bridge) = self.agent_pool.permission_bridge() {
            bridge.update_rule(rule);
        }
    }

    fn set_accept_all(&self, enabled: bool) {
        if let Some(bridge) = self.agent_pool.permission_bridge() {
            bridge.set_accept_all(enabled);
        }
    }

    fn get_tool_rules(&self) -> Vec<crate::agent::permission_bridge::types::ToolAutoAcceptRule> {
        self.agent_pool.permission_bridge()
            .map(|b| b.get_rules())
            .unwrap_or_default()
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
            .map(|(jid, entry)| {
                (
                    jid,
                    serde_json::to_value(entry).unwrap_or(serde_json::Value::Null),
                )
            })
            .collect();
        serde_json::Value::Object(map)
    }

    /// Snapshot of per-agent tool rosters — sent to admin clients on subscribe
    /// so the Agent Console can render currently-online agents and their tools.
    fn get_agent_tools(&self) -> serde_json::Value {
        let cached = self.agent_pool.get_all_cached_tools();
        let map: serde_json::Map<String, serde_json::Value> = cached
            .into_iter()
            .map(|(jid, entry)| {
                (
                    jid,
                    serde_json::to_value(entry).unwrap_or(serde_json::Value::Null),
                )
            })
            .collect();
        serde_json::Value::Object(map)
    }
}

fn dispatch_parent_to_json(p: &agent::dispatch_bridge::DispatchParent) -> serde_json::Value {
    serde_json::json!({
        "id": p.id,
        "goal": p.goal,
        "adminFolder": p.admin_folder,
        "sharedWorkspace": p.shared_workspace,
        "status": p.status,
        "createdAt": p.created_at,
        "completedAt": p.completed_at,
        "tasks": p.tasks.iter().map(|t| serde_json::json!({
            "id": t.id,
            "label": t.label,
            "agentId": t.agent_id,
            "agentJid": t.agent_jid,
            "dependsOn": t.depends_on,
            "prompt": t.prompt,
            "status": t.status.label(),
            "result": t.result,
            "createdAt": t.created_at,
            "startedAt": t.started_at,
            "timeoutAt": t.timeout_at,
            "completedAt": t.completed_at,
            "isVirtual": t.is_virtual,
            "personaName": t.persona_name,
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
        tokio::spawn(async move {
            gw.notify_agent_reply(&jid, &text).await;
        });
    }

    fn notify_agent_state(&self, chat_jid: &str, state: &str) {
        let gw = Arc::clone(&self.gateway);
        let jid = chat_jid.to_string();
        let state = state.to_string();
        tokio::spawn(async move {
            gw.notify_agent_state(&jid, &state).await;
        });
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
            gw.notify_permission_resolved(&jid, &req, &key, &label)
                .await;
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
        tracing::info!(
            "[WsAgentEventSink] notify_agent_todos jid={agent_jid} name={agent_name} count={}",
            todos.len()
        );
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

    fn notify_agent_usage(&self, agent_jid: &str, usage: crate::zen_core::ConversationUsageData) {
        let gw = Arc::clone(&self.gateway);
        let jid = agent_jid.to_string();
        tokio::spawn(async move {
            gw.notify_agent_usage(&jid, &usage).await;
        });
    }
}

// ===== App channel control flow wiring =====

/// Wire AGENT_LIST_REQ / AGENT_SELECT / HISTORY_REQ handlers onto an AppChannel.
/// Called before `connect()` so the handler is in place when the first control
/// frame arrives.
fn wire_app_channel_controls(
    app: &Arc<channels::app::AppChannel>,
    db: Arc<db::Db>,
    gm: Arc<gateway::group_manager::GroupManager>,
    cfg: Arc<config::Config>,
    db_channel_id: i64,
) {
    use channels::app::{
        CTRL_AGENT_LIST_REQ, CTRL_AGENT_LIST_RESP, CTRL_AGENT_SELECT, CTRL_HISTORY_REQ,
        CTRL_HISTORY_RESP,
    };

    let app_for_cb = Arc::clone(app);

    app.set_control_handler(Arc::new(move |sender_id, ctrl_type, metadata| {
        let app = Arc::clone(&app_for_cb);
        let db = Arc::clone(&db);
        let gm = Arc::clone(&gm);
        let cfg = Arc::clone(&cfg);

        match ctrl_type {
            // ── Agent list ──────────────────────────────────────────────────
            t if t == CTRL_AGENT_LIST_REQ => {
                tokio::spawn(async move {
                    // Debug: dump all bindings in DB to verify data exists
                    let all_bindings = db.list_bindings_with_relations().unwrap_or_default();
                    tracing::info!(
                        "[AppChannel] DEBUG AGENT_LIST_REQ from={} db_channel_id={} | total_bindings_in_db={}",
                        sender_id, db_channel_id, all_bindings.len()
                    );
                    for bwr in &all_bindings {
                        tracing::info!(
                            "[AppChannel] DEBUG binding: id={} channel_id={} channel_type={} channel_name={} agent_folder={} agent_name={}",
                            bwr.binding.id,
                            bwr.binding.channel_id,
                            bwr.channel.platform_type,
                            bwr.channel.name,
                            bwr.agent.folder,
                            bwr.agent.name,
                        );
                    }

                    // Only agents explicitly bound to this channel in the DB.
                    let bindings = db.list_bindings_for_channel(db_channel_id).unwrap_or_default();
                    tracing::info!(
                        "[AppChannel] list_bindings_for_channel({}): {} result(s)",
                        db_channel_id, bindings.len()
                    );

                    let payload: Vec<serde_json::Value> = bindings
                        .iter()
                        .map(|bwr| {
                            serde_json::json!({
                                "folder":  bwr.agent.folder,
                                "name":    bwr.agent.name,
                                "isAdmin": bwr.binding.is_admin,
                            })
                        })
                        .collect();
                    let json = serde_json::to_string(&payload).unwrap_or_default();
                    tracing::info!(
                        "[AppChannel] AGENT_LIST_RESP → {} ({} agent(s))",
                        sender_id, payload.len()
                    );
                    let _ = app.send_control(CTRL_AGENT_LIST_RESP, json).await;
                });
            }

            // ── Agent select — validate against channel<->agent bindings ────
            t if t == CTRL_AGENT_SELECT => {
                tokio::spawn(async move {
                    let val: serde_json::Value =
                        serde_json::from_str(&metadata).unwrap_or_default();
                    let folder = val["folder"].as_str().unwrap_or("").to_string();

                    // Validate: folder must be bound to this channel.
                    let bindings = db.list_bindings_for_channel(db_channel_id).unwrap_or_default();
                    let target = bindings.iter().find(|bwr| bwr.agent.folder == folder);

                    let Some(bwr) = target else {
                        tracing::warn!(
                            "[AppChannel] AGENT_SELECT: folder '{}' not bound to channel {} (sender={})",
                            folder, db_channel_id, sender_id
                        );
                        return;
                    };
                    let target_folder = bwr.agent.folder.clone();
                    let target_name   = bwr.agent.name.clone();

                    // Find the sender's own app group JID and update its folder binding.
                    let chat_jid = gm.list(&db).unwrap_or_default()
                        .into_iter()
                        .find(|g| g.jid.contains(&format!(":user:{}", sender_id)))
                        .map(|g| g.jid)
                        .unwrap_or_else(|| format!("app:unknown:user:{}", sender_id));

                    if let Some(mut binding) = gm.get(&db, &chat_jid) {
                        binding.folder = target_folder.clone();
                        binding.name   = target_name.clone();
                        gm.register(&db, &cfg, &binding);
                        tracing::info!(
                            "[AppChannel] AGENT_SELECT: {} → folder={} ({})",
                            chat_jid, target_folder, target_name
                        );
                    } else {
                        tracing::warn!(
                            "[AppChannel] AGENT_SELECT: no group binding for {}", chat_jid
                        );
                    }
                });
            }

            // ── History request ─────────────────────────────────────────────
            t if t == CTRL_HISTORY_REQ => {
                tokio::spawn(async move {
                    let val: serde_json::Value =
                        serde_json::from_str(&metadata).unwrap_or_default();
                    
                    // Support pagination
                    let page = val["page"].as_u64().unwrap_or(1) as u32;
                    let page_size = val["pageSize"].as_u64().unwrap_or(20) as u32;
                    let offset = (page.saturating_sub(1)) * page_size;

                    // Find the chat_jid for this sender.
                    let chat_jid = {
                        let all = gm.list(&db).unwrap_or_default();
                        all.into_iter()
                            .find(|g| {
                                g.channel == "app"
                                    && g.jid.contains(&format!(":user:{}", sender_id))
                            })
                            .map(|g| g.jid)
                    };

                    let Some(chat_jid) = chat_jid else {
                        tracing::warn!(
                            "[AppChannel] HISTORY_REQ: no group for sender {}", sender_id
                        );
                        return;
                    };

                    let messages = db
                        .get_group_messages_paginated(&chat_jid, page_size, offset)
                        .unwrap_or_default();

                    let payload: Vec<serde_json::Value> = messages
                        .iter()
                        .map(|m| {
                            // Keep mobile protocol explicit: only "user" or "agent".
                            // Non-bot messages are treated as user-side messages.
                            let role = if m.is_bot_reply { "agent" } else { "user" };

                            serde_json::json!({
                                "id":        m.message_id,
                                "sender":    m.sender_name,
                                "content":   m.content,
                                "timestamp": m.timestamp,
                                "isFromMe":  m.is_from_me,
                                "isBotReply": m.is_bot_reply,
                                "role":      role,
                            })
                        })
                        .collect();

                    let json = serde_json::to_string(&payload).unwrap_or_default();
                    tracing::info!(
                        "[AppChannel] HISTORY_RESP → {} ({} message(s), page={}, pageSize={})",
                        sender_id, payload.len(), page, page_size
                    );
                    let _ = app.send_control(CTRL_HISTORY_RESP, json).await;
                });
            }

            _ => {
                tracing::debug!("[AppChannel] Unhandled control type={} from {}", ctrl_type, sender_id);
            }
        }
    }));
}

pub async fn run_daemon(cfg: config::Config) -> Result<()> {
    // ===== 0. Setup wizard =====
    setup::run_setup_if_needed(&cfg.paths.global_config_path);

    tracing::info!("[SenClaw] Starting...");

    // ===== 1. Database =====
    let db = Arc::new(db::Db::open(&cfg).context("open database")?);
    tracing::info!("[SenClaw] DB initialized: {}", cfg.paths.db_path.display());

    // ===== 1b. MemoryManager =====
    let _memory_mgr = memory::manager::init(Arc::clone(&db), &cfg);
    tracing::info!("[SenClaw] MemoryManager initialized");

    // ===== 1c. Ensure main agent directory =====
    // Ensure main agent skeleton exists (missing dirs + SOUL.md/MEMORY.md templates),
    // avoiding the case where user accidentally deletes it and no group has isAdmin permissions.
    // Matches TypeScript: ensureAgentDirs('main') before GroupManager creation.
    gateway::group_manager::ensure_agent_dirs(&cfg, &cfg.telegram.agent_folder, &cfg.telegram.agent_folder);
    tracing::info!("[SenClaw] Main agent directory ensured");

    // ===== 2. GroupManager & Other Managers =====
    // Load group bindings from DB; reconcile with config.json
    let gm = Arc::new(gateway::group_manager::GroupManager::new());
    let am = Arc::new(gateway::agent_manager::AgentManager::new());
    let bm = Arc::new(gateway::binding_manager::BindingManager::new());
    let cm = Arc::new(gateway::channel_manager::ChannelManager::new());
    let cowork_mgr = Arc::new(cowork::CoworkManager::new());
    cowork_mgr.ensure_builtin_templates(&cfg);
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
        let reg =
            agent::persona_registry::PersonaRegistry::new(cfg.paths.virtual_agents_dir.clone());
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
    let tg = channels::telegram::TelegramChannel::new(cfg.telegram.bot_token.clone());
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
            tracing::error!(
                "[SenClaw] TelegramChannel connect failed, continuing without Telegram: {e}"
            );
        }
    }
    channels.push(Box::new(tg));

    // 3e. Reconcile channel adapters from DB channels table.
    // Entity migration creates channels from legacy groups; config.json may also
    // have entries. This step ensures any channel stored in the DB that isn't
    // already covered by a config-based adapter gets initialized.
    match cm.list(&db) {
        Ok(db_channels) => {
            for ch_record in &db_channels {
                let creds: serde_json::Value =
                    serde_json::from_str(&ch_record.credentials_json).unwrap_or_default();

                // Skip if a running adapter already covers this exact channel.
                // For Telegram we check by bot token so multiple bots can coexist.
                let already_running = {
                    let platform = ch_record.platform_type.as_str();
                    if platform == "telegram" {
                        let db_token = creds["botToken"].as_str().unwrap_or("").trim();
                        let effective = if db_token.is_empty() {
                            cfg.telegram.bot_token.as_str()
                        } else {
                            db_token
                        };
                        // Already running if a connected Telegram adapter was started with the same token.
                        channels.iter().any(|adapter| {
                            adapter.id() == "telegram"
                                && adapter.is_connected()
                                && !effective.is_empty()
                                && effective == cfg.telegram.bot_token.as_str()
                        })
                    } else {
                        channels
                            .iter()
                            .any(|adapter| adapter.id() == platform && adapter.is_connected())
                    }
                };
                if already_running {
                    continue;
                }

                match ch_record.platform_type.as_str() {
                    "telegram" => {
                        let token = creds["botToken"].as_str().unwrap_or("").trim().to_string();
                        // Use global default token if credentials didn't specify one.
                        let effective_token = if token.is_empty() {
                            cfg.telegram.bot_token.clone()
                        } else {
                            token
                        };
                        if effective_token.is_empty() {
                            tracing::warn!(
                                "[SenClaw] Telegram channel id={} has no bot token (set SENCLAW_TELEGRAM_BOT_TOKEN or enter token in channel settings)",
                                ch_record.id
                            );
                        } else {
                            // Re-use the existing TelegramChannel adapter if available,
                            // otherwise create a new one for this token.
                            let tg_new =
                                channels::telegram::TelegramChannel::new(effective_token.clone());
                            match tg_new.add_bot(&effective_token).await {
                                Ok(()) if tg_new.is_connected() => {
                                    tracing::info!(
                                        "[SenClaw] TelegramChannel from DB (id={}) connected",
                                        ch_record.id
                                    );
                                    channels.push(Box::new(tg_new));
                                }
                                Ok(()) => {
                                    tracing::warn!(
                                        "[SenClaw] TelegramChannel from DB (id={}) did not connect",
                                        ch_record.id
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "[SenClaw] TelegramChannel from DB (id={}) failed: {e}",
                                        ch_record.id
                                    );
                                }
                            }
                        }
                    }
                    "feishu" => {
                        let app_id = creds["appId"].as_str().unwrap_or("");
                        let app_secret = creds["appSecret"].as_str().unwrap_or("");
                        let domain = creds["domain"].as_str();
                        if !app_id.is_empty() && !app_secret.is_empty() {
                            let feishu = channels::feishu::FeishuChannel::new(
                                app_id.to_string(),
                                app_secret.to_string(),
                                domain.map(|s| s.to_string()),
                            );
                            match feishu.connect().await {
                                Ok(()) if feishu.is_connected() => {
                                    tracing::info!(
                                        "[SenClaw] FeishuChannel from DB (id={}) connected",
                                        ch_record.id
                                    );
                                    channels.push(Box::new(feishu));
                                }
                                Ok(()) => {
                                    tracing::warn!(
                                        "[SenClaw] FeishuChannel from DB (id={}) not connected",
                                        ch_record.id
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "[SenClaw] FeishuChannel from DB (id={}) failed: {e}",
                                        ch_record.id
                                    );
                                }
                            }
                        }
                    }
                    "qq" => {
                        let app_id = creds["appId"].as_str().unwrap_or("");
                        let app_secret = creds["appSecret"].as_str().unwrap_or("");
                        let sandbox = creds["sandbox"].as_bool().unwrap_or(false);
                        if !app_id.is_empty() && !app_secret.is_empty() {
                            let qq = channels::qq::QQChannel::new(
                                app_id.to_string(),
                                app_secret.to_string(),
                                sandbox,
                            );
                            match qq.connect().await {
                                Ok(()) if qq.is_connected() => {
                                    tracing::info!(
                                        "[SenClaw] QQChannel from DB (id={}) connected",
                                        ch_record.id
                                    );
                                    channels.push(Box::new(qq));
                                }
                                Ok(()) => {
                                    tracing::warn!(
                                        "[SenClaw] QQChannel from DB (id={}) not connected",
                                        ch_record.id
                                    );
                                }
                                Err(e) => {
                                    tracing::error!(
                                        "[SenClaw] QQChannel from DB (id={}) failed: {e}",
                                        ch_record.id
                                    );
                                }
                            }
                        }
                    }
                    "app" | "senclaw" => {
                        let hub_url = creds["hubUrl"].as_str().unwrap_or("http://localhost:50051");
                        let channel_id = creds["channelId"].as_str().unwrap_or("");
                        let enc_key_b64 = creds["encryptionKey"].as_str().unwrap_or("");
                        let access_token = creds["accessToken"].as_str().unwrap_or("");
                        if !channel_id.is_empty()
                            && !enc_key_b64.is_empty()
                            && !access_token.is_empty()
                        {
                            if let Ok(crypto) = util::crypto::Crypto::new_from_b64(enc_key_b64) {
                                let key = crypto.get_key();
                                let app_arc = Arc::new(channels::app::AppChannel::new(
                                    hub_url.to_string(),
                                    channel_id.to_string(),
                                    access_token.to_string(),
                                    key,
                                ));
                                wire_app_channel_controls(
                                    &app_arc,
                                    Arc::clone(&db),
                                    Arc::clone(&gm),
                                    Arc::new(cfg.clone()),
                                    ch_record.id,
                                );
                                channels::app::AppChannel::connect_nonblocking(Arc::clone(
                                    &app_arc,
                                ));
                                tracing::info!(
                                    "[SenClaw] AppChannel from DB (id={}) registered (relay in background)",
                                    ch_record.id
                                );
                                channels.push(Box::new(Arc::clone(&app_arc)));
                            }
                        }
                    }
                    _ => {
                        tracing::debug!(
                            "[SenClaw] Channel id={} type={}: no DB-based init needed",
                            ch_record.id,
                            ch_record.platform_type
                        );
                    }
                }
            }
            let db_init_count = db_channels
                .iter()
                .filter(|c| {
                    c.platform_type == "feishu"
                        || c.platform_type == "qq"
                        || c.platform_type == "app"
                        || c.platform_type == "senclaw"
                })
                .count();
            if db_init_count > 0 {
                tracing::info!(
                    "[SenClaw] DB channel reconciliation: checked {} channel(s)",
                    db_init_count
                );
            }
        }
        Err(e) => {
            tracing::error!("[SenClaw] Failed to list DB channels for reconciliation: {e}");
        }
    }

    let connected_count = channels.iter().filter(|ch| ch.is_connected()).count();
    if connected_count == 0 {
        tracing::warn!("[SenClaw] No channels are connected; running in WebUI-only mode.");
    } else {
        tracing::info!("[SenClaw] {connected_count} channel(s) connected");
    }

    // Wrap channels for shared access (callbacks + shutdown).
    let channels: Arc<tokio::sync::Mutex<Vec<Box<dyn Channel>>>> =
        Arc::new(tokio::sync::Mutex::new(channels));

    // ===== 3e. ensure admin group =====
    // Creates admin group (JID depends on configured admin IDs — Telegram > Feishu > web:main).
    // Must happen after channels connect so bot userId is known.
    gateway::group_manager::ensure_admin_group(&db, &gm, &cfg, None);
    tracing::info!("[SenClaw] Admin group ensured");

    // ===== 3f. MCP Manager =====
    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let user_config_dir = cfg
        .paths
        .global_config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".senclaw")
        });
    let mcp_manager = Arc::new(mcp::manager::McpManager::new(working_dir, user_config_dir));
    if let Err(e) = mcp_manager.init().await {
        tracing::warn!("[SenClaw] MCP manager init: {e}");
    }
    tracing::info!("[SenClaw] MCP manager initialized");

    // ===== 4. GroupQueue + AgentPool =====
    let group_queue = agent::group_queue::GroupQueue::new(cfg.agent.max_concurrent);
    let agent_pool = agent::agent_pool::AgentPool::new(Arc::new(
        agent::agent_pool::ZenCoreApi::new(Some(Arc::clone(&mcp_manager))),
    ));
    agent_pool.set_db(Arc::clone(&db));
    agent_pool.set_config(Arc::new(cfg.clone()));

    // Initialize marketplace manager for loading MCP servers from plugins
    let marketplace_manager = Arc::new(
        marketplace::manager::MarketplaceManager::new().unwrap_or_else(|_| {
            marketplace::manager::MarketplaceManager::with_paths(
                cfg.paths.marketplace_config_path.clone(),
                cfg.paths.marketplace_state_path.clone(),
                cfg.paths.marketplace_clones_dir.clone(),
            ).unwrap_or_else(|_| {
                marketplace::manager::MarketplaceManager::with_paths(
                    std::path::PathBuf::from("/tmp/senclaw-marketplace-config.json"),
                    std::path::PathBuf::from("/tmp/senclaw-marketplace-state.json"),
                    std::path::PathBuf::from("/tmp/senclaw-marketplace"),
                ).unwrap_or_else(|_| panic!("Failed to create marketplace manager"))
            })
        })
    );
    agent_pool.set_marketplace_manager(Arc::clone(&marketplace_manager));
    tracing::info!("[SenClaw] MarketplaceManager initialized and wired to AgentPool");

    let dispatch_bridge = Arc::new(agent::dispatch_bridge::DispatchBridge::new(
        cfg.paths.dispatch_state_path.clone(),
    ));
    agent_pool.set_dispatch_bridge(
        Arc::clone(&dispatch_bridge) as Arc<dyn agent::dispatch_bridge::DispatchBridgeApi>
    );
    // Shared map for routing virtual-agent permission responses back to their waiting thread.
    let virtual_perm_senders: Arc<Mutex<HashMap<String, std::sync::mpsc::SyncSender<String>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    agent_pool.set_permission_bridge(Arc::new(agent::permission_bridge::PermissionBridge::new(
        Arc::new(RealPermissionApi {
            agent_pool: agent_pool.clone(),
            virtual_perm_senders: Arc::clone(&virtual_perm_senders),
        }),
        None,
    )));

    // ===== DailyLogger for conversation history =====
    let daily_logger = Arc::new(memory::daily_logger::DailyLogger::new(
        cfg.paths.agents_dir.clone(),
    ));
    agent_pool.set_daily_logger(daily_logger);
    tracing::info!("[SenClaw] DailyLogger initialized");

    tracing::info!(
        "[SenClaw] AgentPool (zen-core engine) + GroupQueue (max_concurrent={}) ready",
        cfg.agent.max_concurrent
    );

    // Wire reply send through the correct channel.
    {
        let chs = Arc::clone(&channels);
        agent_pool.set_send_reply(Arc::new(
            move |jid: &str, text: &str, bot_token: Option<&str>| {
                let chs = Arc::clone(&chs);
                let jid = jid.to_string();
                let text = text.to_string();
                let bt = bot_token.map(|s| s.to_string());
                tokio::spawn(async move {
                    let guard = chs.lock().await;
                    for c in guard.iter() {
                        if c.owns_jid(&jid) {
                            let _ = c.send_message(&jid, &text, bt.as_deref()).await;
                            break;
                        }
                    }
                });
            },
        ));
    }
    tracing::info!("[SenClaw] Reply routing wired to channels");

    // Wire typing indicator through the correct channel.
    {
        let chs = Arc::clone(&channels);
        agent_pool.set_typing_fn(Arc::new(
            move |jid: &str, active: bool, bot_token: Option<&str>| {
                let chs = Arc::clone(&chs);
                let jid = jid.to_string();
                let bt = bot_token.map(|s| s.to_string());
                tokio::spawn(async move {
                    let guard = chs.lock().await;
                    for c in guard.iter() {
                        if c.owns_jid(&jid) {
                            let _ = c.set_typing(&jid, active, bt.as_deref()).await;
                            break;
                        }
                    }
                });
            },
        ));
    }

    // Start SendBridge (HTTP bridge for MCP send-server).
    let _send_bridge = {
        let chs_msg = Arc::clone(&channels);
        let chs_file = Arc::clone(&channels);
        let send_msg = Arc::new(
            move |jid: String, text: String, bot_token: Option<String>| {
                let chs = Arc::clone(&chs_msg);
                Box::pin(async move {
                    let guard = chs.lock().await;
                    for c in guard.iter() {
                        if c.owns_jid(&jid) {
                            let _ = c.send_message(&jid, &text, bot_token.as_deref()).await;
                            break;
                        }
                    }
                }) as futures::future::BoxFuture<'static, ()>
            },
        );
        let send_file = Arc::new(
            move |jid: String,
                  file_path: String,
                  caption: Option<String>,
                  bot_token: Option<String>| {
                let chs = Arc::clone(&chs_file);
                Box::pin(async move {
                    let guard = chs.lock().await;
                    for c in guard.iter() {
                        if c.owns_jid(&jid) {
                            let _ = c
                                .send_file(
                                    &jid,
                                    &file_path,
                                    caption.as_deref(),
                                    bot_token.as_deref(),
                                )
                                .await;
                            break;
                        }
                    }
                }) as futures::future::BoxFuture<'static, ()>
            },
        );
        match agent::send_bridge::SendBridge::start(send_msg, send_file).await {
            Ok(sb) => {
                tracing::info!("[SenClaw] SendBridge on port {}", sb.port());
                Some(sb)
            }
            Err(e) => {
                tracing::warn!("[SenClaw] SendBridge failed to start: {e}");
                None
            }
        }
    };

    // ===== 4b. MessageRouter =====
    let message_router = Arc::new(gateway::message_router::MessageRouter::new(
        Arc::clone(&gm),
        Arc::clone(&bm),
        agent_pool.clone() as Arc<dyn types::AgentApi>,
        Arc::clone(&group_queue),
        Arc::clone(&db),
        Arc::new(cfg.clone()),
    ));
    // Wire incoming messages from all channels → MessageRouter
    {
        let chs = channels.lock().await;
        for ch in chs.iter() {
            let router = Arc::clone(&message_router);
            ch.on_message(Box::new(move |msg| {
                let r = Arc::clone(&router);
                tokio::spawn(async move {
                    r.handle_incoming(msg).await;
                });
            }));
        }
    }
    tracing::info!("[SenClaw] MessageRouter wired to {connected_count} channel(s)");

    // ===== 5. TaskScheduler =====
    let task_executor = Arc::new(scheduler::DefaultTaskExecutor::new(Arc::clone(&db)));
    let _task_scheduler = scheduler::task_scheduler::TaskScheduler::new(
        Arc::clone(&db),
        task_executor,
        30, // poll interval in seconds
    )
    .start();
    tracing::info!("[SenClaw] TaskScheduler started (30s poll interval)");

    // ===== 5b. VirtualWorkerPool =====
    let virtual_worker_pool = Arc::new(agent::virtual_worker_pool::VirtualWorkerPool::new(
        Arc::new(agent::virtual_worker_pool::ZenVirtualCoreApi),
    ));
    // Wire permission config follow (mirrors main-agent skip-perms).
    {
        let pool = agent_pool.clone();
        virtual_worker_pool.set_permission_bind(
            move |_virtual_jid: &str, _persona_name: &str, _skip_perms: bool| {
                // Permission bridge for virtual agents: follow main-agent config.
                // Real implementation will register PermissionBridge handlers
                // on the virtual core's engine.
                None
            },
            Arc::new(move || pool.get_skip_perms_for_virtual()),
        );
    }
    // Wire virtual agent permission forwarding: when a virtual subagent needs user
    // approval, forward the request to the admin Web UI via PermissionBridge, then
    // block until the user responds (up to 10 minutes).
    {
        let pool_for_vw = agent_pool.clone();
        let senders_for_vw = Arc::clone(&virtual_perm_senders);
        virtual_worker_pool.set_virtual_permission_fn(Arc::new(
            move |virtual_jid: String,
                  tool_name: String,
                  title: String,
                  content: serde_json::Value,
                  options: HashMap<String, String>,
                  tx: std::sync::mpsc::SyncSender<String>| {
                let key = format!("{virtual_jid}::{tool_name}");
                senders_for_vw.lock().unwrap().insert(key, tx);
                pool_for_vw.handle_virtual_permission_request(
                    &virtual_jid,
                    &tool_name,
                    &title,
                    &content,
                    &options,
                );
            },
        ));
    }
    // Inject browser MCP server so browser-agent virtual instances have browser tools.
    // Use zen_core::McpServerConfig (not mcp::helper) since VirtualWorkerPool uses that type.
    virtual_worker_pool.set_extra_mcp_servers(vec![{
        let helper_cfg = crate::mcp::helper::browser_mcp_config(cfg.ws_port);
        crate::zen_core::McpServerConfig {
            name: helper_cfg.name,
            command: helper_cfg.command,
            args: helper_cfg.args,
            env: helper_cfg.env,
            request_timeout_secs: None,
        }
    }]);
    tracing::info!("[SenClaw] VirtualWorkerPool ready (browser-mcp injected)");

    // ===== 6. WebSocketGateway + UIServer =====
    // WS and UI listen on separate ports (matching TS config).

    // 5a. WebSocket gateway
    let ws_gateway = {
        let ws_api = Arc::new(RealWsApi {
            group_queue: Arc::clone(&group_queue),
            agent_pool: agent_pool.clone(),
        });

        let browser_relay = Arc::new(gateway::websocket_gateway::BrowserRelay::new());

        let ws_state = Arc::new(gateway::websocket_gateway::WsState {
            config: Arc::new(cfg.clone()),
            db: Arc::clone(&db),
            group_manager: Arc::clone(&gm),
            agent_manager: Arc::clone(&am),
            binding_manager: Arc::clone(&bm),
            channel_manager: Arc::clone(&cm),
            cowork_manager: Arc::clone(&cowork_mgr),
            api: ws_api,
            agent_api: Some(agent_pool.clone() as Arc<dyn types::AgentApi>),
            browser_relay,
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

        // Wire MessageRouter → WebSocket gateway for real-time incoming messages.
        message_router.set_ws_gateway(Arc::clone(&gw)).await;

        // Wire DispatchBridge → WebSocket gateway. Every state mutation pushes
        // a `dispatch:update` to admin clients so the Agent Console reflects
        // current parents/tasks without polling.
        {
            let gw_for_dispatch = Arc::clone(&gw);
            dispatch_bridge.set_ws_notify(Arc::new(move |parents: &serde_json::Value| {
                let gw = Arc::clone(&gw_for_dispatch);
                let parents = parents.clone();
                tokio::spawn(async move {
                    gw.notify_dispatch_update(&parents).await;
                });
            }));
        }

        // Wire CoworkManager → WebSocket gateway. Every mutation fires
        // a cowork:changed event so the Cowork UI auto-refreshes.
        {
            let gw_for_cowork = Arc::clone(&gw);
            cowork_mgr.set_on_changed(Box::new(move || {
                let gw = Arc::clone(&gw_for_cowork);
                tokio::spawn(async move {
                    gw.broadcast_to_all(&serde_json::json!({
                        "type": "cowork:changed",
                    }))
                    .await;
                });
            }));
        }

        // Wire task result event — broadcast cowork:task:result with full payload.
        {
            let gw_for_result = Arc::clone(&gw);
            cowork_mgr.set_on_task_result(Box::new(move |evt| {
                let gw = Arc::clone(&gw_for_result);
                let payload = serde_json::json!({
                    "type": "cowork:task:result",
                    "taskId": evt.task_id,
                    "workspaceId": evt.workspace_id,
                    "title": evt.title,
                    "inputSummary": evt.input_summary,
                    "resultOutput": evt.result_output,
                    "references": evt.references,
                    "artifacts": evt.artifacts,
                    "completedAt": evt.completed_at,
                });
                tokio::spawn(async move {
                    gw.broadcast_to_all(&payload).await;
                });
            }));
        }

        // Wire resource-changed event — broadcast cowork:resource:changed.
        {
            let gw_for_res = Arc::clone(&gw);
            cowork_mgr.set_on_resource_changed(Box::new(move |workspace_id| {
                let gw = Arc::clone(&gw_for_res);
                let payload = serde_json::json!({
                    "type": "cowork:resource:changed",
                    "workspaceId": workspace_id,
                });
                tokio::spawn(async move {
                    gw.broadcast_to_all(&payload).await;
                });
            }));
        }

        // Wire DispatchBridge → CoworkManager. Routes cowork tasks through the
        // DAG dispatch system instead of direct process_and_wait. A lifecycle
        // callback keeps CoworkTask status in sync with DispatchTask transitions.
        cowork_mgr.set_dispatch_bridge(Arc::clone(&dispatch_bridge));
        {
            let mgr = Arc::clone(&cowork_mgr);
            let db = Arc::clone(&db);
            let api = Arc::clone(&agent_pool) as Arc<dyn types::AgentApi>;
            dispatch_bridge.set_task_lifecycle_callback(Arc::new(
                move |task_id: &str, status: &str, label: &str, goal: &str, result: Option<String>| {
                    mgr.on_dispatch_task_lifecycle(
                        &db,
                        task_id,
                        status,
                        label,
                        goal,
                        result.as_deref(),
                        Some(Arc::clone(&api)),
                        Arc::clone(&mgr),
                    );
                },
            ));
        }

        // Wire DispatchBridge → AgentPool. The scheduler hands off augmented
        // prompts to sub-agents via GroupQueue + process_and_wait, mirroring
        // the inbound message path. Workspace overrides are applied before
        // enqueue so the sub-agent picks them up.
        {
            let pool = agent_pool.clone();
            let gm = Arc::clone(&gm);
            let gq = Arc::clone(&group_queue);
            let db = Arc::clone(&db);
            dispatch_bridge.set_send_to_agent(Arc::new(
                move |jid: &str, task_id: &str, prompt: &str, workspace_dir: &str| {
                    tracing::info!(
                        "[DispatchBridge] send_to_agent: jid={jid} task={task_id} ws={workspace_dir} prompt_len={}",
                        prompt.len()
                    );
                    let binding: types::GroupBinding = match gm.get(&db, jid) {
                        Some(b) => b,
                        None => {
                            // Cowork agents don't have GroupManager entries — their
                            // JID is synthetic (cowork:{ws_id}:{member_id}). Build a
                            // GroupBinding on the fly so the agent can execute.
                            if jid.starts_with("cowork:") {
                                let parts: Vec<&str> = jid.splitn(3, ':').collect();
                                let member_id = parts.get(2).unwrap_or(&"agent");
                                types::GroupBinding {
                                    jid: jid.to_string(),
                                    folder: member_id.to_string(),
                                    name: format!("cowork-{member_id}"),
                                    channel: "web".to_string(),
                                    group_type: "cowork".to_string(),
                                    is_admin: false,
                                    requires_trigger: false,
                                    allowed_tools: None,
                                    allowed_paths: None,
                                    allowed_work_dirs: if workspace_dir.is_empty() {
                                        None
                                    } else {
                                        Some(vec![workspace_dir.to_string()])
                                    },
                                    bot_token: None,
                                    max_messages: None,
                                    last_active: None,
                                    added_at: chrono::Utc::now().to_rfc3339(),
                                }
                            } else {
                                tracing::warn!(
                                    "[DispatchBridge] send_to_agent: no binding for {jid}, dropping task {task_id}"
                                );
                                return;
                            }
                        }
                    };
                    if !workspace_dir.is_empty() {
                        pool.set_dispatch_workspace(jid, workspace_dir);
                    }
                    pool.set_current_dispatch_task_id(jid, task_id);
                    pool.mark_dispatch_executing(jid);

                    let pool = pool.clone();
                    let gq = Arc::clone(&gq);
                    let jid_owned = jid.to_string();
                    let task_id_owned = task_id.to_string();
                    let prompt_owned = prompt.to_string();
                    tokio::spawn(async move {
                        let pool_inner = pool.clone();
                        let jid_run = jid_owned.clone();
                        let task_id_run = task_id_owned.clone();
                        gq.enqueue(
                            &jid_owned,
                            Box::pin(async move {
                                tracing::info!(
                                    "[DispatchBridge] queue task start: jid={jid_run} task={task_id_run}"
                                );
                                let result = types::AgentApi::process_and_wait(
                                    pool_inner.as_ref(),
                                    &jid_run,
                                    &binding,
                                    &prompt_owned,
                                )
                                .await;
                                match result {
                                    Ok(()) => tracing::info!(
                                        "[DispatchBridge] queue task done: jid={jid_run} task={task_id_run}"
                                    ),
                                    Err(e) => tracing::warn!(
                                        "[DispatchBridge] queue task error: jid={jid_run} task={task_id_run}: {e}"
                                    ),
                                }
                            }),
                        )
                        .await;
                    });
                },
            ));
        }
        {
            let pool = agent_pool.clone();
            dispatch_bridge.set_revert_workspace(Arc::new(move |jid: &str| {
                pool.revert_dispatch_workspace(jid);
            }));
        }
        {
            let pool = agent_pool.clone();
            dispatch_bridge.set_abort_agent(Arc::new(move |jid: &str, reason: &str| {
                let pool = pool.clone();
                let jid = jid.to_string();
                let reason = reason.to_string();
                tokio::spawn(async move {
                    tracing::warn!("[DispatchBridge] aborting {jid}: {reason}");
                    pool.destroy_inner(&jid).await;
                });
            }));
        }
        // Wire virtual-agent dispatch (Phase 5): persona registry + worker pool.
        dispatch_bridge.set_virtual_workers(
            Arc::clone(&persona_registry),
            Arc::clone(&virtual_worker_pool),
        );
        // Wire virtual-agent todos → WebSocket gateway (mirrors TS
        // virtualWorkerPool.setTodosNotify).
        {
            let gw_for_todos = Arc::clone(&gw);
            virtual_worker_pool.set_todos_notify(Arc::new(
                move |jid: &str, name: &str, todos: &[agent::virtual_worker_pool::TodoItem]| {
                    let todos = serde_json::to_value(todos).unwrap_or(serde_json::Value::Null);
                    let jid = jid.to_string();
                    let name = name.to_string();
                    let gw = Arc::clone(&gw_for_todos);
                    tokio::spawn(async move {
                        gw.notify_agent_todos(&jid, &name, &todos).await;
                    });
                },
            ));
        }
        // Initial agent sync — without this, MCP `dispatch_task` can't resolve
        // agent name → jid (state.agents stays empty) and tasks never leave
        // `registered`. Re-sync periodically to pick up groups added/removed
        // through the Web UI without needing per-handler hooks.
        {
            let groups = gm.list(&db).unwrap_or_default();
            dispatch_bridge.update_agents(&groups);
            tracing::info!(
                "[SenClaw] DispatchBridge agents synced ({} group(s))",
                groups.len()
            );
        }
        {
            let bridge_for_sync = Arc::clone(&dispatch_bridge);
            let gm_for_sync = Arc::clone(&gm);
            let db_for_sync = Arc::clone(&db);
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
                tick.tick().await; // skip immediate
                loop {
                    tick.tick().await;
                    let groups = gm_for_sync.list(&db_for_sync).unwrap_or_default();
                    bridge_for_sync.update_agents(&groups);
                }
            });
        }
        dispatch_bridge.start();

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

    // 5c. EventNotifier — polls space_events for reminders and status transitions.
    //     Wired after ws_gateway so it can push events to connected clients.
    {
        // Arc<WebSocketGateway> implements EventNotifySink; wrap in a second Arc
        // to get the Arc<dyn EventNotifySink> the EventNotifier expects.
        struct WsEventSinkWrapper(Arc<gateway::websocket_gateway::WebSocketGateway>);
        impl scheduler::EventNotifySink for WsEventSinkWrapper {
            fn notify_event_reminder(&self, event_id: &str, title: &str, start_at_ms: i64, kind: &str) {
                let gw = Arc::clone(&self.0);
                let id = event_id.to_string();
                let t = title.to_string();
                let k = kind.to_string();
                tokio::spawn(async move {
                    gw.push_event_reminder(&id, &t, start_at_ms, &k).await;
                });
            }
        }
        let event_sink: Arc<dyn scheduler::EventNotifySink> =
            Arc::new(WsEventSinkWrapper(Arc::clone(&ws_gateway)));
        let _event_notifier = scheduler::EventNotifier::new(
            Arc::clone(&db),
            event_sink,
            60, // poll every 60 seconds
        )
        .start();
        let tz_name = chrono::Local::now().format("%Z %z").to_string();
        tracing::info!("[SenClaw] EventNotifier started (60s poll, local TZ: {tz_name})");
    }

    // 7b. WikiManager
    let wiki_mgr = Arc::new(wiki::manager::WikiManager::new(cfg.paths.wiki_dir.clone()));
    if let Err(e) = wiki_mgr.ensure_init().await {
        tracing::warn!("[SenClaw] Wiki init failed (non-fatal): {e}");
    } else {
        tracing::info!(
            "[SenClaw] WikiManager initialized: {}",
            cfg.paths.wiki_dir.display()
        );
    }

    // 7c. UI HTTP server
    {
        struct RealUiApi {
            agent_pool: Arc<agent::agent_pool::AgentPool>,
        }
        impl gateway::ui_server::UiApi for RealUiApi {
            fn reload_all_skills(&self) {
                self.agent_pool.reload_all_skills();
            }
            fn get_thinking_enabled(&self) -> bool {
                self.agent_pool.get_thinking_enabled()
            }
            fn set_thinking_enabled(&self, enabled: bool) {
                self.agent_pool.set_thinking_enabled(enabled);
            }
            fn get_permissions_config(&self) -> gateway::ui_server::AdminPermissionsConfig {
                let cfg = self.agent_pool.get_permissions_config();
                gateway::ui_server::AdminPermissionsConfig {
                    skip_main_agent_permissions: cfg.skip_main_agent_permissions,
                    skip_all_agents_permissions: cfg.skip_all_agents_permissions,
                }
            }
            fn set_permissions_config(&self, config: gateway::ui_server::AdminPermissionsConfig) {
                self.agent_pool
                    .set_permissions_config(agent::agent_pool::PermissionsConfig {
                        skip_main_agent_permissions: config.skip_main_agent_permissions,
                        skip_all_agents_permissions: config.skip_all_agents_permissions,
                    });
            }
        }

        let ui_state = Arc::new(gateway::ui_server::UiState {
            config: Arc::new(cfg.clone()),
            db: Some(Arc::clone(&db)),
            cowork_manager: Some(Arc::clone(&cowork_mgr)),
            wiki_manager: Some(Arc::clone(&wiki_mgr)),
            persona_registry: Some(Arc::clone(&persona_registry)),
            agent_api: Some(Arc::new(RealUiApi {
                agent_pool: agent_pool.clone(),
            })),
            cowork_agent_api: Some(agent_pool.clone() as Arc<dyn types::AgentApi>),
            mcp_manager: Some(Arc::clone(&mcp_manager)),
            marketplace_manager: Some(Arc::new(std::sync::Mutex::new(
                marketplace::manager::MarketplaceManager::new()
                    .unwrap_or_else(|e| {
                        tracing::warn!("[SenClaw] Failed to initialize marketplace manager: {e}");
                        marketplace::manager::MarketplaceManager::with_paths(
                            cfg.paths.marketplace_config_path.clone(),
                            cfg.paths.marketplace_state_path.clone(),
                            cfg.paths.marketplace_clones_dir.clone(),
                        ).unwrap_or_else(|e2| {
                            tracing::error!("[SenClaw] Failed to create marketplace manager with custom paths: {e2}");
                            // Create a dummy manager that will fail gracefully
                            marketplace::manager::MarketplaceManager::with_paths(
                                std::path::PathBuf::from("/tmp/senclaw-marketplace-config.json"),
                                std::path::PathBuf::from("/tmp/senclaw-marketplace-state.json"),
                                std::path::PathBuf::from("/tmp/senclaw-marketplace"),
                            ).unwrap_or_else(|_| {
                                panic!("Failed to create even a dummy marketplace manager")
                            })
                        })
                    })
            ))),
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
    {
        let chs = channels.lock().await;
        for ch in chs.iter() {
            let id = ch.id();
            if let Err(e) = ch.disconnect().await {
                tracing::warn!("[SenClaw] Error disconnecting {id}: {e}");
            }
        }
    }

    // Drop ws_gateway to close all client connections
    drop(ws_gateway);

    tracing::info!("[SenClaw] Goodbye.");
    Ok(())
}
