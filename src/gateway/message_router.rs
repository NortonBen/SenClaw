//! Message routing core — bridges channels, agent pool, and group management.
//! Mirrors `src-old/gateway/MessageRouter.ts`.

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::agent::group_queue::GroupQueue;
use crate::agent::session_bridge::build_prompt_for_group;
use crate::config::Config;
use crate::db::Db;
use crate::gateway::binding_manager::BindingManager;
use crate::gateway::command_dispatcher::dispatch_command;
use crate::gateway::group_manager::{ensure_app_group, ensure_wechat_admin_group, GroupManager};
use crate::gateway::trigger_checker::{should_trigger, should_trigger_entity};
use crate::gateway::websocket_gateway::WebSocketGateway;
use crate::types::{BindingWithRelations, GroupBinding, IncomingMessage, StoredMessage};

// ===== Agent API trait =====

/// Operations MessageRouter needs from AgentPool.
#[async_trait]
pub trait AgentApi: Send + Sync {
    /// Send a direct reply to a chat (for admin commands and unregistered Feishu notices).
    async fn broadcast_reply(&self, chat_jid: &str, text: &str, bot_token: Option<&str>);

    /// Process a prompt through the agent. Blocks until the agent finishes.
    async fn process_and_wait(
        &self,
        jid: &str,
        group: &GroupBinding,
        prompt: &str,
    ) -> Result<()>;

    /// Destroy/cleanup agent state for a JID (after JID migration).
    async fn destroy(&self, jid: &str);
}

/// No-op stub — used before AgentPool is ported.
pub struct NoopAgentApi;

#[async_trait]
impl AgentApi for NoopAgentApi {
    async fn broadcast_reply(&self, _jid: &str, _text: &str, _token: Option<&str>) {}
    async fn process_and_wait(
        &self,
        _jid: &str,
        _group: &GroupBinding,
        _prompt: &str,
    ) -> Result<()> {
        tracing::warn!("[MessageRouter] NoopAgentApi::process_and_wait — agent not wired");
        Ok(())
    }
    async fn destroy(&self, _jid: &str) {}
}

// ===== JID migration callback =====

pub type OnJidMigrated = Arc<dyn Fn(&str, &GroupBinding) + Send + Sync + 'static>;

// ===== MessageRouter =====

pub struct MessageRouter {
    group_manager: Arc<GroupManager>,
    binding_manager: Arc<BindingManager>,
    agent_api: Arc<dyn AgentApi>,
    group_queue: Arc<GroupQueue>,
    db: Arc<Db>,
    config: Arc<Config>,
    wechat_agent_folder: String,
    notified_jids: Mutex<HashSet<String>>,
    on_jid_migrated: Mutex<Option<OnJidMigrated>>,
    ws_gateway: Mutex<Option<Arc<WebSocketGateway>>>,
}

impl MessageRouter {
    pub fn new(
        group_manager: Arc<GroupManager>,
        binding_manager: Arc<BindingManager>,
        agent_api: Arc<dyn AgentApi>,
        group_queue: Arc<GroupQueue>,
        db: Arc<Db>,
        config: Arc<Config>,
    ) -> Self {
        Self {
            group_manager,
            binding_manager,
            agent_api,
            group_queue,
            db,
            config,
            wechat_agent_folder: "main".to_string(),
            notified_jids: Mutex::new(HashSet::new()),
            on_jid_migrated: Mutex::new(None),
            ws_gateway: Mutex::new(None),
        }
    }

    pub async fn set_ws_gateway(&self, gw: Arc<WebSocketGateway>) {
        *self.ws_gateway.lock().await = Some(gw);
    }

    pub async fn set_on_jid_migrated(&self, cb: OnJidMigrated) {
        let mut guard = self.on_jid_migrated.lock().await;
        *guard = Some(cb);
    }

    /// Resolve a [`GroupBinding`] for the incoming message JID.
    /// Tries the new entity model first (bindings table), then falls back to
    /// the legacy groups table.
    async fn resolve_binding(&self, msg: &IncomingMessage) -> Option<GroupBinding> {
        // 1. Try new entity model via BindingManager.
        if let Ok(Some(br)) = self
            .binding_manager
            .get_with_relations(&self.db, &msg.chat_jid)
        {
            tracing::info!(
                "[MessageRouter] Resolved via entity model: agent={} channel={}",
                br.agent.folder,
                br.channel.name
            );
            return Some(to_group_binding(&br));
        }

        // 2. Fall back to legacy GroupManager.
        self.group_manager.get(&self.db, &msg.chat_jid)
    }

    /// Main entry point — called by channels when a message arrives.
    pub async fn handle_incoming(&self, msg: IncomingMessage) {
        tracing::info!(
            "[MessageRouter] Incoming from {}: \"{}\"",
            msg.chat_jid,
            &msg.content.chars().take(60).collect::<String>(),
        );

        // 1. Find registered group binding (entity model first, then legacy)
        let mut group = self.resolve_binding(&msg).await;

        if group.is_none() {
            if msg.chat_jid.starts_with("wx:") {
                if msg.bot_token.is_some() {
                    group = self.complete_pending_wechat_binding(&msg).await;
                }
                if group.is_none() {
                    ensure_wechat_admin_group(
                        &self.db,
                        &self.group_manager,
                        &self.config,
                        &msg.chat_jid,
                        &self.wechat_agent_folder,
                    );
                    group = self.group_manager.get(&self.db, &msg.chat_jid);
                }
            }
            if group.is_none() && msg.chat_jid.starts_with("tg:") {
                group = self.complete_pending_telegram_binding(&msg).await;
            }
            if group.is_none() && msg.chat_jid.starts_with("feishu:") {
                group = self.complete_pending_feishu_binding(&msg).await;
            }
            if group.is_none() && msg.chat_jid.starts_with("qq:") {
                group = self.complete_pending_qq_binding(&msg).await;
            }
            if group.is_none() && msg.chat_jid.starts_with("app:") {
                ensure_app_group(&self.db, &self.group_manager, &self.config, &msg.chat_jid);
                group = self.group_manager.get(&self.db, &msg.chat_jid);
            }
            if group.is_none() {
                tracing::info!(
                    "[MessageRouter] No registered group for {}, ignoring",
                    msg.chat_jid
                );
                self.notify_unregistered_feishu(&msg).await;
                return;
            }
        }

        let group = group.unwrap();

        // 2. Persist message
        self.store_message(&msg);

        // 2b. Notify WebSocket clients of the incoming message (real-time update).
        if let Some(gw) = self.ws_gateway.lock().await.clone() {
            gw.notify_incoming(&msg).await;
        }

        // 3. Trigger check
        if !should_trigger(&msg, &group) {
            tracing::info!(
                "[MessageRouter] Trigger check failed for {}",
                msg.chat_jid
            );
            return;
        }

        // 4. Admin command interception
        if group.is_admin {
            if let Some(result) = dispatch_command(&self.db, &msg.content, Some(&msg.chat_jid)) {
                tracing::info!("[MessageRouter] Command handled for {}", msg.chat_jid);
                self.agent_api
                    .broadcast_reply(&msg.chat_jid, &result, group.bot_token.as_deref())
                    .await;
                return;
            }
        }

        tracing::info!("[MessageRouter] Triggering agent for {}", msg.chat_jid);

        // 5. Update last-active
        self.group_manager
            .touch_active(&self.db, &msg.chat_jid, &chrono_now());

        // 6. Build prompt and enqueue
        let agent_api = Arc::clone(&self.agent_api);
        let db = Arc::clone(&self.db);
        let jid = msg.chat_jid.clone();
        let g = group.clone();

        let jid_key = jid.clone();
        self.group_queue
            .enqueue(
                &jid_key,
                Box::pin(async move {
                    run_agent(agent_api, db, jid, g).await;
                }),
            )
            .await;
    }

    /// Dispatch a task directly (bypasses trigger/command checks).
    pub async fn dispatch_task(&self, jid: &str, prompt: &str, callbacks: Option<DispatchTaskCallbacks>) {
        let Some(group) = self.group_manager.get(&self.db, jid) else {
            tracing::warn!("[MessageRouter] dispatchTask: no group for {jid}");
            return;
        };
        self.group_manager
            .touch_active(&self.db, jid, &chrono_now());

        let agent_api = Arc::clone(&self.agent_api);
        let jid_owned = jid.to_string();
        let g = group.clone();
        let p = prompt.to_string();

        let jid_key = jid_owned.clone();
        self.group_queue
            .enqueue(
                &jid_key,
                Box::pin(async move {
                    if let Some(ref cb) = callbacks {
                        (cb.on_started)();
                    }
                    if let Err(e) = agent_api.process_and_wait(&jid_owned, &g, &p).await {
                        tracing::error!("[MessageRouter] dispatchTask agent error for {jid_owned}: {e:#}");
                    }
                    if let Some(ref cb) = callbacks {
                        (cb.on_completed)();
                    }
                }),
            )
            .await;
    }

    // ===== Internal =====

    async fn complete_pending_binding(
        &self,
        msg: &IncomingMessage,
        pending: Option<GroupBinding>,
    ) -> Option<GroupBinding> {
        let pending = pending?;
        let old_jid = pending.jid.clone();
        let new_binding = self.group_manager.migrate_jid(
            &self.db,
            &self.config.paths.global_config_path,
            &old_jid,
            &msg.chat_jid,
        )?;
        tracing::info!("[MessageRouter] Pending binding completed: {old_jid} → {}", msg.chat_jid);
        self.agent_api.destroy(&old_jid).await;
        let guard = self.on_jid_migrated.lock().await;
        if let Some(ref cb) = *guard {
            cb(&old_jid, &new_binding);
        }
        Some(new_binding)
    }

    /// For Telegram: find any channel whose credentials contain the incoming bot token
    /// and that has a pending (jid=NULL) binding, then complete it with the real JID.
    async fn complete_pending_telegram_binding(
        &self,
        msg: &IncomingMessage,
    ) -> Option<GroupBinding> {
        let bot_token = msg.bot_token.as_deref().unwrap_or("");
        if bot_token.is_empty() {
            return None;
        }

        // Find all telegram channels, check which one owns this token
        let tg_channels = self.db.find_channels_by_platform("telegram").ok()?;
        for ch in &tg_channels {
            let creds: serde_json::Value =
                serde_json::from_str(&ch.credentials_json).unwrap_or_default();
            let channel_token = creds["botToken"].as_str().unwrap_or("");
            // Match: explicit token in creds, or empty (uses default env token)
            if channel_token == bot_token || channel_token.is_empty() {
                if let Ok(count) = self.binding_manager.complete_pending_new_model(
                    &self.db,
                    ch.id,
                    &msg.chat_jid,
                ) {
                    if count > 0 {
                        tracing::info!(
                            "[MessageRouter] Telegram pending binding completed for {} on channel '{}'",
                            msg.chat_jid, ch.name
                        );
                        // Now resolve via entity model
                        if let Ok(Some(br)) = self
                            .binding_manager
                            .get_with_relations(&self.db, &msg.chat_jid)
                        {
                            return Some(to_group_binding(&br));
                        }
                    }
                }
            }
        }

        // Also try legacy groups table (tg:pending:{token})
        let pending = self
            .group_manager
            .find_pending_telegram_binding(&self.db, bot_token);
        self.complete_pending_binding(msg, pending).await
    }

    async fn complete_pending_feishu_binding(
        &self,
        msg: &IncomingMessage,
    ) -> Option<GroupBinding> {
        let app_id = msg.bot_token.as_deref().unwrap_or("");
        if app_id.is_empty() {
            return None;
        }
        let pending = self
            .group_manager
            .find_pending_feishu_binding(&self.db, app_id);
        self.complete_pending_binding(msg, pending).await
    }

    async fn complete_pending_qq_binding(&self, msg: &IncomingMessage) -> Option<GroupBinding> {
        let app_id = msg.bot_token.as_deref().unwrap_or("");
        if app_id.is_empty() {
            return None;
        }
        let pending = self
            .group_manager
            .find_pending_qq_binding(&self.db, app_id);
        self.complete_pending_binding(msg, pending).await
    }

    async fn complete_pending_wechat_binding(
        &self,
        msg: &IncomingMessage,
    ) -> Option<GroupBinding> {
        let folder = msg.bot_token.as_deref().unwrap_or("");
        if folder.is_empty() {
            return None;
        }
        let pending = self
            .group_manager
            .find_pending_wechat_binding(&self.db, folder);
        self.complete_pending_binding(msg, pending).await
    }

    async fn notify_unregistered_feishu(&self, msg: &IncomingMessage) {
        if !msg.chat_jid.starts_with("feishu:") {
            return;
        }
        {
            let mut jids = self.notified_jids.lock().await;
            if !jids.insert(msg.chat_jid.clone()) {
                return;
            }
        }
        let text = format!(
            "👋 Hello!\n\nThis conversation is not bound to SemaClaw yet.\n\n\
             Your JID is: `{}`\n\n\
             Please add an Agent in the Web admin UI and paste the JID above into the Chat JID field.",
            msg.chat_jid
        );
        self.agent_api
            .broadcast_reply(&msg.chat_jid, &text, msg.bot_token.as_deref())
            .await;
    }

    fn store_message(&self, msg: &IncomingMessage) {
        let stored = StoredMessage {
            message_id: msg.id.clone(),
            chat_jid: msg.chat_jid.clone(),
            sender_jid: msg.sender_jid.clone(),
            sender_name: msg.sender_name.clone(),
            content: msg.content.clone(),
            timestamp: msg.timestamp.clone(),
            is_from_me: msg.is_from_me,
            is_bot_reply: false,
            reply_to_id: None,
            media_type: None,
        };
        if let Err(e) = self
            .db
            .insert_message(&stored, self.config.agent.max_messages_per_group)
        {
            tracing::error!("[MessageRouter] Failed to store message {}: {e:#}", msg.id);
        }
    }
}

// ===== Dispatch task callbacks =====

pub struct DispatchTaskCallbacks {
    pub on_started: Box<dyn Fn() + Send + 'static>,
    pub on_completed: Box<dyn Fn() + Send + 'static>,
}

// ===== Standalone agent runner =====

async fn run_agent(agent_api: Arc<dyn AgentApi>, db: Arc<Db>, jid: String, group: GroupBinding) {
    let prompt_built_at = chrono_now();
    let (prompt, last_msg_timestamp) = build_prompt_for_group(&db, &jid);

    if prompt.is_empty() {
        tracing::warn!("[MessageRouter] Empty prompt for {jid}, skipping");
        return;
    }

    let cursor = match last_msg_timestamp {
        Some(ref last_ts) if last_ts.as_str() > prompt_built_at.as_str() => Some(last_ts.clone()),
        _ => Some(prompt_built_at),
    };

    if let Err(e) = agent_api.process_and_wait(&jid, &group, &prompt).await {
        tracing::error!("[MessageRouter] Agent error for {jid}: {e:#}");
    }

    if let Some(ts) = cursor {
        let _ = db.set_last_agent_timestamp(&jid, &ts);
    }
}

/// Synthesize a legacy [`GroupBinding`] from the new entity model so the
/// existing AgentPool / trigger-checker / dispatch paths work without changes.
fn to_group_binding(br: &BindingWithRelations) -> GroupBinding {
    GroupBinding {
        jid: br.binding.jid.clone().unwrap_or_default(),
        folder: br.agent.folder.clone(),
        name: br.agent.name.clone(),
        channel: br.channel.platform_type.clone(),
        is_admin: br.binding.is_admin,
        requires_trigger: br.agent.requires_trigger,
        allowed_tools: br.agent.allowed_tools.clone(),
        allowed_paths: br.agent.allowed_paths.clone(),
        allowed_work_dirs: br.agent.allowed_work_dirs.clone(),
        bot_token: br.binding.bot_token_override.clone(),
        max_messages: br.binding.max_messages,
        last_active: br.binding.last_active.clone(),
        added_at: br.binding.created_at.clone(),
    }
}

// ===== Helpers =====

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format_iso(now.as_secs())
}

fn format_iso(secs: u64) -> String {
    let days = secs / 86400;
    let tod = secs % 86400;
    let h = tod / 3600;
    let m = (tod % 3600) / 60;
    let s = tod % 60;
    let (y, mo, d) = days_to_ymd(days as i64);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.000Z")
}

fn days_to_ymd(mut days: i64) -> (i64, u32, u32) {
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
