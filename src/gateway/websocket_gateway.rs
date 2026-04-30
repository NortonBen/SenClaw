//! WebSocket gateway. Port target: src-old/gateway/WebSocketGateway.ts
//!
//! Provides real-time event streaming and bidirectional interaction for Web UI / CLI.
//! Listens on 127.0.0.1:{port} (default 18789), not exposed externally.
//!
//! Client → Server protocol:
//!   { type: 'connect', token?: string }
//!   { type: 'subscribe', groupJid: string }
//!   { type: 'unsubscribe', groupJid: string }
//!   { type: 'message', groupJid: string, text: string }
//!   { type: 'list:groups' }
//!   { type: 'register:group', jid, folder, name, ... }
//!   { type: 'unregister:group', jid }
//!   { type: 'update:group', jid, ...fields }
//!   { type: 'list:tasks', groupJid?: string }
//!   { type: 'list:task-logs', taskId: string, limit?: number }
//!   { type: 'manage:task', taskId: string, action: 'pause'|'resume'|'cancel' }
//!   { type: 'permission:response', requestId, optionKey }
//!   { type: 'question:response', requestId, answers, otherTexts? }
//!   { type: 'register:feishu-app', appId, appSecret, domain? }
//!   { type: 'unregister:feishu-app', appId }
//!   { type: 'register:qq-app', appId, appSecret, sandbox? }
//!   { type: 'unregister:qq-app', appId }
//!   { type: 'list:feishu-apps' }
//!   { type: 'list:dispatch' }
//!   { type: 'agent:control', groupJid, action, query? }

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::Mutex;

use uuid::Uuid;

use crate::config::Config;
use crate::db::Db;
use crate::gateway::command_dispatcher::dispatch_command;
use crate::util::local_time::local_iso_string_now;
use crate::gateway::group_manager::{
    delete_feishu_app, delete_qq_app, delete_telegram_bot, get_feishu_apps, save_feishu_app,
    save_qq_app, save_telegram_bot, GroupBindingUpdate, GroupManager,
};
use crate::types::{GroupBinding, TaskStatus};

// ===== Trait for AgentPool / channel dependencies =====

#[async_trait]
pub trait WsGatewayApi: Send + Sync {
    /// Enqueue a message to the group queue for agent processing.
    fn enqueue_and_process(&self, _group_jid: &str, _group: &GroupBinding, _text: &str) {}
    /// Resolve a pending permission request.
    fn resolve_permission(&self, _request_id: &str, _option_key: &str) {}
    /// Resolve a pending ask-question batch.
    fn resolve_ask_question(
        &self,
        _request_id: &str,
        _answers: &serde_json::Value,
        _other_texts: Option<&serde_json::Value>,
    ) {
    }
    /// Pause the agent for a group.
    fn pause_agent(&self, _group_jid: &str) {}
    /// Resume the agent for a group, with optional follow-up query.
    fn resume_agent(&self, _group_jid: &str, _query: Option<&str>) {}
    /// Stop the agent for a group.
    async fn stop_agent(&self, _group_jid: &str) {}

    /// Get current dispatch parents (for admin clients on subscribe).
    fn get_dispatch_parents(&self) -> serde_json::Value {
        serde_json::Value::Null
    }
    /// Get current agent todos (for admin clients on subscribe).
    fn get_agent_todos(&self) -> serde_json::Value {
        serde_json::Value::Null
    }
    /// Get current per-agent tool rosters (for admin clients on subscribe).
    /// Returned shape: `{ "<agentJid>": { "agentName": ..., "tools": [...] } }`.
    fn get_agent_tools(&self) -> serde_json::Value {
        serde_json::Value::Null
    }

    // Channel management (default no-ops — real impl provided by daemon wiring).
    async fn add_telegram_bot(&self, _token: &str) -> Result<()> {
        Ok(())
    }
    fn get_telegram_bot_user_id(&self, _token: &str) -> Option<String> {
        None
    }
    async fn ensure_feishu_channel(&self) -> Result<()> {
        Ok(())
    }
    async fn add_feishu_app_runtime(
        &self,
        _app_id: &str,
        _app_secret: &str,
        _domain: Option<&str>,
    ) -> Result<bool> {
        Ok(true)
    }
    fn add_qq_app_runtime(&self, _app_id: &str, _app_secret: &str, _sandbox: bool) {}
}

// ===== WsClient =====

struct WsClient {
    sender: tokio::sync::mpsc::UnboundedSender<Message>,
    authenticated: bool,
    is_admin: bool,
    subscriptions: HashSet<String>,
}

// ===== GroupInfo (wire format, camelCase) =====

#[derive(Debug, Clone, Serialize)]
struct GroupInfo {
    jid: String,
    folder: String,
    name: String,
    #[serde(rename = "isAdmin")]
    is_admin: bool,
    channel: String,
    #[serde(rename = "requiresTrigger")]
    requires_trigger: bool,
    #[serde(rename = "allowedTools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(rename = "allowedWorkDirs")]
    allowed_work_dirs: Option<Vec<String>>,
    #[serde(rename = "botToken")]
    bot_token: Option<String>,
    #[serde(rename = "maxMessages")]
    max_messages: Option<u32>,
}

fn to_group_info(g: &GroupBinding) -> GroupInfo {
    GroupInfo {
        jid: g.jid.clone(),
        folder: g.folder.clone(),
        name: g.name.clone(),
        is_admin: g.is_admin,
        channel: g.channel.clone(),
        requires_trigger: g.requires_trigger,
        allowed_tools: g.allowed_tools.clone(),
        allowed_work_dirs: g.allowed_work_dirs.clone(),
        bot_token: g.bot_token.clone(),
        max_messages: g.max_messages,
    }
}

// ===== Entity wire format (camelCase) =====

#[derive(Debug, Clone, Serialize)]
struct ChannelInfoWire {
    id: i64,
    #[serde(rename = "platformType")]
    platform_type: String,
    name: String,
    #[serde(rename = "credentialsJson")]
    credentials_json: String,
    #[serde(rename = "connectionState")]
    connection_state: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

fn to_channel_info(ch: &crate::types::Channel) -> ChannelInfoWire {
    ChannelInfoWire {
        id: ch.id,
        platform_type: ch.platform_type.clone(),
        name: ch.name.clone(),
        credentials_json: ch.credentials_json.clone(),
        connection_state: ch.connection_state.clone(),
        created_at: ch.created_at.clone(),
        updated_at: ch.updated_at.clone(),
    }
}

#[derive(Debug, Clone, Serialize)]
struct AgentInfoWire {
    id: i64,
    folder: String,
    name: String,
    #[serde(rename = "requiresTrigger")]
    requires_trigger: bool,
    #[serde(rename = "allowedTools")]
    allowed_tools: Option<Vec<String>>,
    #[serde(rename = "allowedWorkDirs")]
    allowed_work_dirs: Option<Vec<String>>,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

fn to_agent_info(a: &crate::types::Agent) -> AgentInfoWire {
    AgentInfoWire {
        id: a.id,
        folder: a.folder.clone(),
        name: a.name.clone(),
        requires_trigger: a.requires_trigger,
        allowed_tools: a.allowed_tools.clone(),
        allowed_work_dirs: a.allowed_work_dirs.clone(),
        created_at: a.created_at.clone(),
        updated_at: a.updated_at.clone(),
    }
}

#[derive(Debug, Clone, Serialize)]
struct BindingWithRelationsWire {
    id: i64,
    jid: Option<String>,
    #[serde(rename = "agentId")]
    agent_id: i64,
    #[serde(rename = "channelId")]
    channel_id: i64,
    #[serde(rename = "isAdmin")]
    is_admin: bool,
    #[serde(rename = "botTokenOverride")]
    bot_token_override: Option<String>,
    #[serde(rename = "maxMessages")]
    max_messages: Option<u32>,
    #[serde(rename = "lastActive")]
    last_active: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: String,
    agent: AgentInfoWire,
    channel: ChannelInfoWire,
}

fn to_binding_with_relations(br: &crate::types::BindingWithRelations) -> BindingWithRelationsWire {
    BindingWithRelationsWire {
        id: br.binding.id,
        jid: br.binding.jid.clone(),
        agent_id: br.binding.agent_id,
        channel_id: br.binding.channel_id,
        is_admin: br.binding.is_admin,
        bot_token_override: br.binding.bot_token_override.clone(),
        max_messages: br.binding.max_messages,
        last_active: br.binding.last_active.clone(),
        created_at: br.binding.created_at.clone(),
        agent: to_agent_info(&br.agent),
        channel: to_channel_info(&br.channel),
    }
}

// ===== Shared state passed through to handlers =====

pub struct WsState {
    pub config: Arc<Config>,
    pub db: Arc<Db>,
    pub group_manager: Arc<GroupManager>,
    pub agent_manager: Arc<crate::gateway::agent_manager::AgentManager>,
    pub binding_manager: Arc<crate::gateway::binding_manager::BindingManager>,
    pub channel_manager: Arc<crate::gateway::channel_manager::ChannelManager>,
    pub api: Arc<dyn WsGatewayApi>,
}

// ===== WebSocketGateway =====

pub struct WebSocketGateway {
    pub port: u16,
    token: Option<String>,
    clients: Arc<Mutex<Vec<WsClient>>>,
    last_known_states: Arc<Mutex<HashMap<String, String>>>,
}

impl WebSocketGateway {
    pub fn new(port: u16, token: Option<String>) -> Self {
        Self {
            port,
            token,
            clients: Arc::new(Mutex::new(Vec::new())),
            last_known_states: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Build the axum route for WebSocket upgrade at `/ws`.
    /// Returns the router and handles for external event injection.
    pub fn route(
        &self,
        state: Arc<WsState>,
    ) -> axum::Router {
        let clients = self.clients.clone();
        let states = self.last_known_states.clone();
        let token = self.token.clone();

        axum::Router::new().route(
            "/",
            axum::routing::get(move |ws: axum::extract::WebSocketUpgrade| {
                let clients = clients.clone();
                let states = states.clone();
                let token = token.clone();
                let state = state.clone();
                async move {
                    ws.on_upgrade(move |socket| {
                        handle_connection(socket, clients, states, token, state)
                    })
                }
            }),
        )
    }

    // ===== Event injection (called externally) =====

    pub async fn notify_incoming(&self, msg: &crate::types::IncomingMessage) {
        let payload = serde_json::json!({
            "type": "incoming",
            "groupJid": msg.chat_jid,
            "senderName": msg.sender_name,
            "text": msg.content,
            "timestamp": msg.timestamp,
            "isFromMe": msg.is_from_me,
        });
        self.broadcast(&msg.chat_jid, &payload).await;
    }

    pub async fn notify_agent_reply(&self, chat_jid: &str, text: &str) {
        let payload = serde_json::json!({
            "type": "agent:reply",
            "groupJid": chat_jid,
            "text": text,
        });
        self.broadcast(chat_jid, &payload).await;
    }

    pub async fn notify_agent_state(&self, chat_jid: &str, state: &str) {
        self.last_known_states
            .lock()
            .await
            .insert(chat_jid.to_string(), state.to_string());
        let payload = serde_json::json!({
            "type": "agent:state",
            "groupJid": chat_jid,
            "state": state,
        });
        self.broadcast(chat_jid, &payload).await;
    }

    pub async fn notify_agent_compacting(&self, chat_jid: &str, is_compacting: bool) {
        let payload = serde_json::json!({
            "type": "agent:compacting",
            "groupJid": chat_jid,
            "isCompacting": is_compacting,
        });
        self.broadcast(chat_jid, &payload).await;
    }

    pub async fn notify_permission_request(
        &self,
        chat_jid: &str,
        request_id: &str,
        payload: &serde_json::Value,
    ) {
        let mut msg = payload.clone();
        if let Some(obj) = msg.as_object_mut() {
            obj.insert("type".into(), "permission:request".into());
            obj.insert("groupJid".into(), chat_jid.into());
            obj.insert("requestId".into(), request_id.into());
        }
        if chat_jid.starts_with("virtual:") {
            self.broadcast_to_admins(&msg).await;
        } else {
            self.broadcast(chat_jid, &msg).await;
        }
    }

    pub async fn notify_task_backlog(
        &self,
        task_id: &str,
        chat_jid: &str,
        prompt: &str,
        interval_ms: u64,
        overdue_ms: u64,
    ) {
        let msg = serde_json::json!({
            "type": "task:backlog",
            "taskId": task_id,
            "chatJid": chat_jid,
            "prompt": prompt,
            "intervalMs": interval_ms,
            "overdueMs": overdue_ms,
            "suggestedIntervalMs": interval_ms + overdue_ms,
        });
        self.broadcast_to_admins(&msg).await;
    }

    pub async fn notify_ask_question_request(
        &self,
        chat_jid: &str,
        request_id: &str,
        payload: &serde_json::Value,
    ) {
        let mut msg = payload.clone();
        if let Some(obj) = msg.as_object_mut() {
            obj.insert("type".into(), "question:request".into());
            obj.insert("groupJid".into(), chat_jid.into());
            obj.insert("requestId".into(), request_id.into());
        }
        self.broadcast(chat_jid, &msg).await;
    }

    pub async fn notify_permission_resolved(
        &self,
        chat_jid: &str,
        request_id: &str,
        option_key: &str,
        option_label: &str,
    ) {
        let msg = serde_json::json!({
            "type": "permission:resolved",
            "groupJid": chat_jid,
            "requestId": request_id,
            "optionKey": option_key,
            "optionLabel": option_label,
        });
        if chat_jid.starts_with("virtual:") {
            self.broadcast_to_admins(&msg).await;
        } else {
            self.broadcast(chat_jid, &msg).await;
        }
    }

    pub async fn notify_ask_question_resolved(
        &self,
        chat_jid: &str,
        request_id: &str,
        answers: &serde_json::Value,
    ) {
        let msg = serde_json::json!({
            "type": "question:resolved",
            "groupJid": chat_jid,
            "requestId": request_id,
            "answers": answers,
        });
        self.broadcast(chat_jid, &msg).await;
    }

    pub async fn notify_dispatch_update(&self, parents: &serde_json::Value) {
        let msg = serde_json::json!({
            "type": "dispatch:update",
            "parents": parents,
        });
        self.broadcast_to_admins(&msg).await;
    }

    pub async fn notify_agent_todos(
        &self,
        agent_jid: &str,
        agent_name: &str,
        todos: &serde_json::Value,
    ) {
        let msg = serde_json::json!({
            "type": "agent:todos",
            "agentJid": agent_jid,
            "agentName": agent_name,
            "todos": todos,
        });
        self.broadcast_to_admins(&msg).await;
    }

    pub async fn notify_agent_tools(
        &self,
        agent_jid: &str,
        agent_name: &str,
        tools: &serde_json::Value,
    ) {
        let msg = serde_json::json!({
            "type": "agent:tools",
            "agentJid": agent_jid,
            "agentName": agent_name,
            "tools": tools,
        });
        self.broadcast_to_admins(&msg).await;
    }

    pub async fn notify_group_migrated(&self, old_jid: &str, new_binding: &GroupBinding) {
        self.broadcast_to_all(
            &serde_json::json!({"type": "group:unregistered", "jid": old_jid}),
        )
        .await;
        self.broadcast_to_all(
            &serde_json::json!({"type": "group:registered", "group": to_group_info(new_binding)}),
        )
        .await;
    }

    /// Push last-known agent state to a newly subscribed client.
    pub async fn push_last_known_state(
        &self,
        sender: &tokio::sync::mpsc::UnboundedSender<Message>,
        jid: &str,
    ) {
        let states = self.last_known_states.lock().await;
        if let Some(state) = states.get(jid) {
            let msg = serde_json::json!({
                "type": "agent:state",
                "groupJid": jid,
                "state": state,
            });
            let _ = sender.send(Message::Text(msg.to_string().into()));
        }
    }

    // ===== Broadcast helpers =====

    async fn broadcast_to_admins(&self, msg: &serde_json::Value) {
        let raw = msg.to_string();
        let clients = self.clients.lock().await;
        let total = clients.len();
        let mut sent = 0usize;
        for client in clients.iter() {
            if client.authenticated && client.is_admin {
                let _ = client.sender.send(Message::Text(raw.clone().into()));
                sent += 1;
            }
        }
        let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("?");
        tracing::info!(
            "[WsGateway] broadcast_to_admins type={msg_type} sent={sent}/{total} client(s)"
        );
        if sent == 0 && (msg_type == "dispatch:update" || msg_type == "agent:todos") {
            tracing::warn!(
                "[WsGateway] {msg_type} fired but NO admin clients connected — \
                 web client must subscribe to an is_admin group first"
            );
        }
    }

    async fn broadcast_to_all(&self, msg: &serde_json::Value) {
        let raw = msg.to_string();
        let clients = self.clients.lock().await;
        for client in clients.iter() {
            if client.authenticated {
                let _ = client.sender.send(Message::Text(raw.clone().into()));
            }
        }
    }

    async fn broadcast(&self, group_jid: &str, msg: &serde_json::Value) {
        let raw = msg.to_string();
        let clients = self.clients.lock().await;
        for client in clients.iter() {
            if client.authenticated && client.subscriptions.contains(group_jid) {
                let _ = client.sender.send(Message::Text(raw.clone().into()));
            }
        }
    }
}

// ===== Connection handler =====

async fn handle_connection(
    ws: WebSocket,
    clients: Arc<Mutex<Vec<WsClient>>>,
    last_known_states: Arc<Mutex<HashMap<String, String>>>,
    token: Option<String>,
    state: Arc<WsState>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let (mut ws_sender, mut ws_receiver) = ws.split();

    // Register client.
    let auto_auth = token.is_none();
    if auto_auth {
        tracing::warn!(
            "[WsGateway] GATEWAY_TOKEN not set — client auto-authenticated. \
             Set GATEWAY_TOKEN via env for production."
        );
    }
    {
        let mut guard = clients.lock().await;
        guard.push(WsClient {
            sender: tx.clone(),
            authenticated: auto_auth,
            is_admin: false,
            subscriptions: HashSet::new(),
        });
    }
    let client_idx: usize = {
        let guard = clients.lock().await;
        guard.len() - 1
    };

    if auto_auth {
        let _ = tx.send(Message::Text(r#"{"type":"auth:ok"}"#.to_string().into()));
    }

    // Forward channel messages → WebSocket sink.
    let mut rx = rx;
    let forward_handle = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    // Read loop.
    while let Some(Ok(msg)) = ws_receiver.next().await {
        match msg {
            Message::Text(text) => {
                let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&text) else {
                    send_error(&clients, client_idx, "Invalid JSON").await;
                    continue;
                };
                handle_message(
                    client_idx,
                    &parsed,
                    &clients,
                    &last_known_states,
                    &token,
                    &state,
                )
                .await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    forward_handle.abort();
    {
        let mut guard = clients.lock().await;
        if client_idx < guard.len() {
            guard.remove(client_idx);
        }
    }
}

// ===== Message dispatch =====

async fn handle_message(
    client_idx: usize,
    msg: &serde_json::Value,
    clients: &Arc<Mutex<Vec<WsClient>>>,
    last_known_states: &Arc<Mutex<HashMap<String, String>>>,
    token: &Option<String>,
    state: &Arc<WsState>,
) {
    let msg_type = msg["type"].as_str().unwrap_or("");

    let sender = {
        let guard = clients.lock().await;
        guard.get(client_idx).map(|c| c.sender.clone())
    };
    let Some(sender) = sender else { return };

    match msg_type {
        "connect" => handle_connect(clients, client_idx, &sender, token, msg).await,
        "subscribe" => {
            handle_subscribe(clients, client_idx, &sender, last_known_states, state, msg)
                .await
        }
        "unsubscribe" => handle_unsubscribe(clients, client_idx, &sender, msg).await,
        "list:groups" => handle_list_groups(clients, client_idx, &sender, state).await,
        "register:group" => {
            handle_register_group(clients, client_idx, &sender, state, msg).await
        }
        "unregister:group" => {
            handle_unregister_group(clients, client_idx, &sender, state, msg).await
        }
        "update:group" => handle_update_group(clients, client_idx, &sender, state, msg).await,
        "message" => handle_message_send(clients, client_idx, &sender, state, msg).await,
        "permission:response" => {
            handle_permission_response(clients, client_idx, &sender, state, msg).await
        }
        "question:response" => {
            handle_question_response(clients, client_idx, &sender, state, msg).await
        }
        "list:tasks" => handle_list_tasks(clients, client_idx, &sender, state, msg).await,
        "list:task-logs" => handle_task_logs(clients, client_idx, &sender, state, msg).await,
        "manage:task" => handle_manage_task(clients, client_idx, &sender, state, msg).await,
        "register:feishu-app" => {
            handle_register_feishu_app(clients, client_idx, &sender, state, msg).await
        }
        "unregister:feishu-app" => {
            handle_unregister_feishu_app(clients, client_idx, &sender, state, msg).await
        }
        "register:qq-app" => handle_register_qq_app(clients, client_idx, &sender, state, msg).await,
        "unregister:qq-app" => {
            handle_unregister_qq_app(clients, client_idx, &sender, state, msg).await
        }
        "list:feishu-apps" => handle_list_feishu_apps(clients, client_idx, &sender, state).await,
        "list:dispatch" => handle_list_dispatch(clients, client_idx, &sender, state).await,
        "agent:control" => handle_agent_control(clients, client_idx, &sender, state, msg).await,
        "list:channels" => handle_list_channels(clients, client_idx, &sender, state).await,
        "list:agents" => handle_list_agents(clients, client_idx, &sender, state).await,
        "list:bindings" => handle_list_bindings(clients, client_idx, &sender, state).await,
        "register:channel" => handle_register_channel(clients, client_idx, &sender, state, msg).await,
        "register:agent" => handle_register_agent(clients, client_idx, &sender, state, msg).await,
        "register:binding" => handle_register_binding(clients, client_idx, &sender, state, msg).await,
        "unregister:channel" => handle_unregister_channel(clients, client_idx, &sender, state, msg).await,
        "unregister:agent" => handle_unregister_agent(clients, client_idx, &sender, state, msg).await,
        "unregister:binding" => handle_unregister_binding(clients, client_idx, &sender, state, msg).await,
        "update:channel" => handle_update_channel(clients, client_idx, &sender, state, msg).await,
        "update:agent" => handle_update_agent(clients, client_idx, &sender, state, msg).await,
        "update:binding" => handle_update_binding(clients, client_idx, &sender, state, msg).await,
        _ => {
            send_json(
                &sender,
                &serde_json::json!({"type": "error", "message": format!("Unknown message type: {msg_type}")}),
            );
        }
    }
}

// ===== Individual message handlers =====

async fn handle_connect(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    token: &Option<String>,
    msg: &serde_json::Value,
) {
    if let Some(ref required_token) = token {
        let provided = msg["token"].as_str().unwrap_or("");
        if provided != required_token {
            send_json(
                sender,
                &serde_json::json!({"type": "auth:error", "message": "Invalid token"}),
            );
            return;
        }
    }
    let mut guard = clients.lock().await;
    if let Some(client) = guard.get_mut(client_idx) {
        client.authenticated = true;
    }
    send_json(sender, &serde_json::json!({"type": "auth:ok"}));
}

async fn handle_subscribe(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    last_known_states: &Arc<Mutex<HashMap<String, String>>>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let jid = msg["groupJid"].as_str().unwrap_or("").to_string();
    if jid.is_empty() {
        send_json(sender, &serde_json::json!({"type": "error", "message": "groupJid required"}));
        return;
    }
    {
        let mut guard = clients.lock().await;
        if let Some(client) = guard.get_mut(client_idx) {
            client.subscriptions.insert(jid.clone());
            // Upgrade to admin client when subscribing to admin group.
            if let Some(group) = state.group_manager.get(&state.db, &jid) {
                if group.is_admin {
                    client.is_admin = true;
                    tracing::info!(
                        "[WsGateway] client #{client_idx} subscribed to {jid} (admin) — \
                         will receive dispatch:update / agent:todos"
                    );
                } else {
                    tracing::info!(
                        "[WsGateway] client #{client_idx} subscribed to {jid} (non-admin)"
                    );
                }
            }
        }
    }

    // Push current dispatch state + agent todos for admin groups.
    let is_admin = {
        let guard = clients.lock().await;
        guard
            .get(client_idx)
            .map(|c| c.is_admin)
            .unwrap_or(false)
    };
    if is_admin {
        let parents = state.api.get_dispatch_parents();
        let parent_count = parents.as_array().map(|a| a.len()).unwrap_or(0);
        tracing::info!(
            "[WsGateway] subscribe snapshot client #{client_idx}: dispatch:update with {parent_count} parent(s)"
        );
        if !parents.is_null() {
            send_json(
                sender,
                &serde_json::json!({"type": "dispatch:update", "parents": parents}),
            );
        }
        let todos = state.api.get_agent_todos();
        if let serde_json::Value::Object(map) = &todos {
            tracing::info!(
                "[WsGateway] subscribe snapshot client #{client_idx}: agent:todos for {} agent(s)",
                map.len()
            );
            for (agent_jid, entry) in map {
                let agent_name =
                    entry.get("agentName").and_then(|v| v.as_str()).unwrap_or(agent_jid);
                let todos_arr = entry.get("todos").cloned().unwrap_or(serde_json::Value::Null);
                send_json(
                    sender,
                    &serde_json::json!({
                        "type": "agent:todos",
                        "agentJid": agent_jid,
                        "agentName": agent_name,
                        "todos": todos_arr,
                    }),
                );
            }
        } else if !todos.is_null() {
            tracing::info!(
                "[WsGateway] subscribe snapshot client #{client_idx}: agent:todos (legacy format)"
            );
            send_json(
                sender,
                &serde_json::json!({"type": "agent:todos", "todos": todos}),
            );
        }

        // Push agent tool rosters so the Agent Console can render every
        // currently-online agent and the tools it can use.
        let tools = state.api.get_agent_tools();
        if let serde_json::Value::Object(map) = &tools {
            for (agent_jid, entry) in map {
                let agent_name =
                    entry.get("agentName").and_then(|v| v.as_str()).unwrap_or(agent_jid);
                let tools_arr = entry.get("tools").cloned().unwrap_or(serde_json::Value::Null);
                send_json(
                    sender,
                    &serde_json::json!({
                        "type": "agent:tools",
                        "agentJid": agent_jid,
                        "agentName": agent_name,
                        "tools": tools_arr,
                    }),
                );
            }
        }
    }

    // Push last-known agent state (fix stale frontend state on reconnect).
    {
        let states = last_known_states.lock().await;
        if let Some(known) = states.get(&jid) {
            send_json(
                sender,
                &serde_json::json!({"type": "agent:state", "groupJid": jid, "state": known}),
            );
        }
    }

    // Load and push chat history so the Web UI shows past conversation.
    if let Ok(messages) = state.db.get_messages(&jid, None) {
        if !messages.is_empty() {
            let history: Vec<serde_json::Value> = messages
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.message_id,
                        "role": if m.is_bot_reply { "agent" } else { "user" },
                        "senderName": m.sender_name,
                        "text": m.content,
                        "timestamp": m.timestamp,
                    })
                })
                .collect();
            send_json(
                sender,
                &serde_json::json!({
                    "type": "history:load",
                    "groupJid": jid,
                    "messages": history,
                }),
            );
        }
    }

    send_json(
        sender,
        &serde_json::json!({"type": "subscribed", "groupJid": jid}),
    );
}

async fn handle_unsubscribe(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let jid = msg["groupJid"].as_str().unwrap_or("").to_string();
    let mut guard = clients.lock().await;
    if let Some(client) = guard.get_mut(client_idx) {
        client.subscriptions.remove(&jid);
    }
}

async fn handle_list_groups(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let groups: Vec<GroupInfo> = state
        .group_manager
        .list(&state.db)
        .unwrap_or_default()
        .iter()
        .map(to_group_info)
        .collect();
    send_json(sender, &serde_json::json!({"type": "groups", "groups": groups}));
}

async fn handle_register_group(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }

    let folder = msg["folder"].as_str().unwrap_or("");
    let name = msg["name"].as_str().unwrap_or("");
    if folder.is_empty() || name.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "folder and name are required"}),
        );
        return;
    }

    let channel = msg["channel"].as_str().unwrap_or("");
    let bot_token = msg["botToken"].as_str().map(|s| s.to_string());
    let mut jid = msg["jid"].as_str().unwrap_or("").to_string();

    // Feishu pending binding.
    if jid.is_empty() && channel == "feishu" {
        let app_id = bot_token.as_deref().unwrap_or("").to_string();
        if app_id.is_empty() {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": "feishu:pending binding requires App ID (botToken)"}),
            );
            return;
        }
        jid = format!("feishu:pending:{app_id}");
    }
    // QQ pending binding.
    if jid.is_empty() && channel == "qq" {
        let app_id = bot_token.as_deref().unwrap_or("").to_string();
        if app_id.is_empty() {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": "qq:pending binding requires App ID (botToken)"}),
            );
            return;
        }
        jid = format!("qq:pending:{app_id}");
    }
    if jid.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "jid is required (or leave empty for feishu/qq pending binding)"}),
        );
        return;
    }

    let mut resolved_jid = jid.clone();
    let resolved_folder = folder.to_string();
    let resolved_name = name.to_string();
    let resolved_channel = channel.to_string();
    let resolved_token = bot_token.clone();
    let token_for_check = resolved_token.clone(); // saved for check after move

    // Telegram: addBot to get botUserId for bot-aware JID.
    if resolved_channel == "telegram" && resolved_token.is_some() {
        let tok = resolved_token.as_deref().unwrap_or("");
        if let Err(e) = state.api.add_telegram_bot(tok).await {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": format!("register:group failed: {e}")}),
            );
            return;
        }
        if let Some(bot_user_id) = state.api.get_telegram_bot_user_id(tok) {
            // Upgrade bare tg:user:{id} → tg:{botUserId}:user:{id}
            if let Some(caps) = regex::Regex::new(r"^tg:(user|group):(-?\d+)$")
                .ok()
                .and_then(|re| {
                    let c = re.captures(&resolved_jid)?;
                    Some((c.get(1)?.as_str().to_string(), c.get(2)?.as_str().to_string()))
                })
            {
                resolved_jid = format!("tg:{bot_user_id}:{}:{}", caps.0, caps.1);
            }
            // Persist to config.json.
            if let Some(chat_caps) = regex::Regex::new(r"^tg:(?:\d+:)?user:(\d+)$")
                .ok()
                .and_then(|re| re.captures(&resolved_jid).and_then(|c| c.get(1).map(|m| m.as_str().to_string())))
            {
                let _ = save_telegram_bot(
                    &state.config.paths.global_config_path,
                    crate::gateway::group_manager::TelegramBotConfig {
                        token: tok.to_string(),
                        admin_user_id: chat_caps,
                        folder: resolved_folder.clone(),
                        name: Some(resolved_name.clone()),
                    },
                );
            }
        }
    }

    let now = now_iso();
    let existing = state.group_manager.get(&state.db, &resolved_jid);
    let binding = GroupBinding {
        jid: resolved_jid.clone(),
        folder: resolved_folder.clone(),
        name: resolved_name.clone(),
        channel: if resolved_channel.is_empty() {
            existing.as_ref().map(|e| e.channel.clone()).unwrap_or_default()
        } else {
            resolved_channel.clone()
        },
        is_admin: false,
        requires_trigger: msg["requiresTrigger"]
            .as_bool()
            .unwrap_or(existing.as_ref().map(|e| e.requires_trigger).unwrap_or(true)),
        allowed_tools: if msg.get("allowedTools").is_some() {
            msg["allowedTools"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        } else {
            existing.as_ref().and_then(|e| e.allowed_tools.clone())
        },
        allowed_paths: None,
        allowed_work_dirs: if msg.get("allowedWorkDirs").is_some() {
            msg["allowedWorkDirs"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        } else {
            existing.as_ref().and_then(|e| e.allowed_work_dirs.clone())
        },
        bot_token: resolved_token.or(existing.as_ref().and_then(|e| e.bot_token.clone())),
        max_messages: if msg.get("maxMessages").is_some() {
            msg["maxMessages"].as_u64().map(|n| n as u32)
        } else {
            existing.as_ref().and_then(|e| e.max_messages)
        },
        last_active: existing.as_ref().and_then(|e| e.last_active.clone()),
        added_at: existing
            .as_ref()
            .map(|e| e.added_at.clone())
            .unwrap_or_else(|| now.clone()),
    };

    state
        .group_manager
        .register(&state.db, &state.config, &binding);

    // Telegram group bindings without dedicated token: fire-and-forget addBot.
    if resolved_channel == "telegram" && token_for_check.is_none() {
        if let Some(ref bt) = binding.bot_token {
            let bt = bt.clone();
            let api = state.api.clone();
            tokio::spawn(async move {
                if let Err(e) = api.add_telegram_bot(&bt).await {
                    tracing::error!("[WsGateway] Failed to register bot token: {e}");
                }
            });
        }
    }
    // Feishu: lazy-connect when group is registered.
    if binding.jid.starts_with("feishu:") {
        let api = state.api.clone();
        tokio::spawn(async move {
            if let Err(e) = api.ensure_feishu_channel().await {
                tracing::error!("[WsGateway] Failed to ensure FeishuChannel: {e}");
            }
        });
    }

    let info = to_group_info(&binding);
    broadcast_to_all_inner(clients, &serde_json::json!({"type": "group:registered", "group": info}))
        .await;
}

async fn handle_unregister_group(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let jid = msg["jid"].as_str().unwrap_or("").to_string();
    if jid.is_empty() {
        send_json(sender, &serde_json::json!({"type": "error", "message": "jid required"}));
        return;
    }
    let Some(group) = state.group_manager.get(&state.db, &jid) else {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": format!("Group not found: {jid}")}),
        );
        return;
    };
    if group.is_admin {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "Cannot unregister admin group"}),
        );
        return;
    }

    // Clean up channel-specific config entries.
    if group.channel == "telegram"
        && group.bot_token.is_some()
        && regex::Regex::new(r"^tg:(?:\d+:)?user:")
            .ok()
            .and_then(|re| re.is_match(&group.jid).then_some(()))
            .is_some()
    {
        let _ = delete_telegram_bot(
            &state.config.paths.global_config_path,
            group.bot_token.as_deref().unwrap_or(""),
        );
    }
    if group.channel == "feishu" && group.bot_token.is_some() {
        let app_id = group.bot_token.as_deref().unwrap_or("");
        let all = state.group_manager.list(&state.db).unwrap_or_default();
        let still_used = all
            .iter()
            .any(|g| g.jid != jid && g.channel == "feishu" && g.bot_token.as_deref() == Some(app_id));
        if !still_used {
            let _ = delete_feishu_app(&state.config.paths.global_config_path, app_id);
        }
    }
    if group.channel == "qq" && group.bot_token.is_some() {
        let app_id = group.bot_token.as_deref().unwrap_or("");
        let all = state.group_manager.list(&state.db).unwrap_or_default();
        let still_used = all
            .iter()
            .any(|g| g.jid != jid && g.channel == "qq" && g.bot_token.as_deref() == Some(app_id));
        if !still_used {
            let _ = delete_qq_app(&state.config.paths.global_config_path, app_id);
        }
    }

    state
        .group_manager
        .unregister(&state.db, &state.config, &jid);
    broadcast_to_all_inner(
        clients,
        &serde_json::json!({"type": "group:unregistered", "jid": jid}),
    )
    .await;
}

async fn handle_update_group(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let jid = msg["jid"].as_str().unwrap_or("").to_string();
    if jid.is_empty() {
        send_json(sender, &serde_json::json!({"type": "error", "message": "jid required"}));
        return;
    }

    let mut updates = GroupBindingUpdate::default();
    if let Some(v) = msg["name"].as_str() {
        updates.name = Some(v.to_string());
    }
    if let Some(v) = msg["channel"].as_str() {
        updates.channel = Some(v.to_string());
    }
    if let Some(v) = msg["requiresTrigger"].as_bool() {
        updates.requires_trigger = Some(v);
    }
    if msg.get("allowedTools").is_some() {
        updates.allowed_tools = Some(
            msg["allowedTools"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()),
        );
    }
    if msg.get("allowedWorkDirs").is_some() {
        updates.allowed_work_dirs = Some(
            msg["allowedWorkDirs"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()),
        );
    }
    if msg.get("botToken").is_some() {
        updates.bot_token = Some(msg["botToken"].as_str().map(String::from));
    }
    if msg.get("maxMessages").is_some() {
        updates.max_messages = Some(msg["maxMessages"].as_u64().map(|n| n as u32));
    }

    match state
        .group_manager
        .update(&state.db, &state.config, &jid, updates)
    {
        Ok(updated) => {
            let info = to_group_info(&updated);
            broadcast_to_all_inner(
                clients,
                &serde_json::json!({"type": "group:updated", "group": info}),
            )
            .await;
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": format!("update:group failed: {e}")}),
            );
        }
    }
}

async fn handle_message_send(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let group_jid = msg["groupJid"].as_str().unwrap_or("").to_string();
    let text = msg["text"].as_str().unwrap_or("").trim().to_string();
    if group_jid.is_empty() || text.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "groupJid and text required"}),
        );
        return;
    }

    // Admin command interception.
    let is_admin = {
        let guard = clients.lock().await;
        guard
            .get(client_idx)
            .map(|c| c.is_admin)
            .unwrap_or(false)
    };
    if is_admin {
        let is_reset = text.trim() == "/reset" || text.trim() == "reset" || text.trim().starts_with("/reset ") || text.trim().starts_with("reset ");
        if let Some(output) =
            dispatch_command(&state.db, &text, Some(&group_jid))
        {
            send_json(
                sender,
                &serde_json::json!({"type": "agent:reply", "groupJid": group_jid, "text": output}),
            );
            // After reset, push empty history so the frontend clears its local messages.
            if is_reset {
                send_json(
                    sender,
                    &serde_json::json!({"type": "history:load", "groupJid": group_jid, "messages": []}),
                );
            }
            return;
        }
    }

    // Pending binding check.
    if group_jid.contains(":pending:") {
        let ch = if group_jid.starts_with("qq:") {
            "QQ"
        } else {
            "Feishu"
        };
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": format!("{ch} binding is not complete. Please send the first message from {ch} to complete JID binding.")}),
        );
        return;
    }

    let Some(group) = state.group_manager.get(&state.db, &group_jid) else {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": format!("Group not found: {group_jid}")}),
        );
        return;
    };

    // Persist user message to conversation history so it appears in history:load.
    let stored = crate::types::StoredMessage {
        message_id: format!("web:{}", Uuid::new_v4()),
        chat_jid: group_jid.clone(),
        sender_jid: String::new(),
        sender_name: String::new(),
        content: text.clone(),
        timestamp: local_iso_string_now(),
        is_from_me: false,
        is_bot_reply: false,
        reply_to_id: None,
        media_type: None,
    };
    let limit = state
        .config
        .agent
        .max_messages_per_group;
    if let Err(e) = state.db.insert_message(&stored, limit) {
        tracing::warn!("[WebSocketGateway] Failed to persist web message for {group_jid}: {e}");
    }

    state.api.enqueue_and_process(&group_jid, &group, &text);
}

async fn handle_permission_response(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let request_id = msg["requestId"].as_str().unwrap_or("").to_string();
    let option_key = msg["optionKey"].as_str().unwrap_or("").to_string();
    if request_id.is_empty() || option_key.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "requestId and optionKey required"}),
        );
        return;
    }
    state.api.resolve_permission(&request_id, &option_key);
}

async fn handle_question_response(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let request_id = msg["requestId"].as_str().unwrap_or("").to_string();
    let answers = &msg["answers"];
    if request_id.is_empty() || answers.is_null() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "requestId and answers required"}),
        );
        return;
    }
    let other_texts = msg.get("otherTexts");
    state
        .api
        .resolve_ask_question(&request_id, answers, other_texts);
}

async fn handle_list_tasks(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let jid = msg["groupJid"].as_str();
    let tasks = if let Some(jid) = jid {
        if let Some(group) = state.group_manager.get(&state.db, jid) {
            state.db.get_tasks_by_group(&group.folder).unwrap_or_default()
        } else {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": format!("Group not found: {jid}")}),
            );
            return;
        }
    } else {
        state.db.list_all_tasks().unwrap_or_default()
    };
    let tasks_json: Vec<serde_json::Value> = tasks
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "groupFolder": t.group_folder,
                "chatJid": t.chat_jid,
                "prompt": t.prompt,
                "scheduleType": t.schedule_type.as_str(),
                "scheduleValue": t.schedule_value,
                "contextMode": t.context_mode.as_str(),
                "scriptCommand": t.script_command,
                "nextRun": t.next_run,
                "lastRun": t.last_run,
                "lastResult": t.last_result,
                "status": t.status.as_str(),
                "createdAt": t.created_at,
            })
        })
        .collect();
    send_json(
        sender,
        &serde_json::json!({"type": "tasks", "tasks": tasks_json, "groupJid": jid}),
    );
}

async fn handle_task_logs(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let task_id = msg["taskId"].as_str().unwrap_or("").to_string();
    if task_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "taskId required"}),
        );
        return;
    }
    let limit = msg["limit"].as_u64().unwrap_or(20) as u32;
    let logs = state.db.get_task_run_logs(&task_id, limit).unwrap_or_default();
    let logs_json: Vec<serde_json::Value> = logs
        .iter()
        .map(|l| {
            serde_json::json!({
                "id": l.id,
                "taskId": l.task_id,
                "runAt": l.run_at,
                "durationMs": l.duration_ms,
                "status": l.status.as_str(),
                "result": l.result,
                "error": l.error,
            })
        })
        .collect();
    send_json(
        sender,
        &serde_json::json!({"type": "task-logs", "taskId": task_id, "logs": logs_json}),
    );
}

async fn handle_manage_task(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let task_id = msg["taskId"].as_str().unwrap_or("").to_string();
    let action = msg["action"].as_str().unwrap_or("").to_string();
    if task_id.is_empty() || action.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "taskId and action required"}),
        );
        return;
    }
    let new_status = match action.as_str() {
        "pause" => TaskStatus::Paused,
        "resume" => TaskStatus::Active,
        "cancel" => TaskStatus::Completed,
        _ => {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": format!("Unknown action: {action}")}),
            );
            return;
        }
    };
    let _ = state.db.update_task_status(&task_id, new_status);
    send_json(
        sender,
        &serde_json::json!({"type": "task:updated", "taskId": task_id, "status": new_status.as_str()}),
    );
}

async fn handle_register_feishu_app(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let app_id = msg["appId"].as_str().unwrap_or("");
    let app_secret = msg["appSecret"].as_str().unwrap_or("");
    if app_id.is_empty() || app_secret.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "appId and appSecret required"}),
        );
        return;
    }
    let domain = msg["domain"].as_str();
    if let Err(e) = save_feishu_app(
        &state.config.paths.global_config_path,
        app_id,
        app_secret,
        domain,
    ) {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": format!("register:feishu-app failed: {e}")}),
        );
        return;
    }
    // Hot-register into FeishuChannel at runtime.
    match state
        .api
        .add_feishu_app_runtime(app_id, app_secret, domain)
        .await
    {
        Ok(false) => {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": format!("Feishu app {app_id} connection failed: invalid credentials or network issue. Check App ID/App Secret and ensure the app is published.")}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": format!("register:feishu-app failed: {e}")}),
            );
        }
        Ok(true) => {
            send_json(
                sender,
                &serde_json::json!({"type": "feishu-app:registered", "appId": app_id}),
            );
        }
    }
}

async fn handle_unregister_feishu_app(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let app_id = msg["appId"].as_str().unwrap_or("").to_string();
    if app_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "appId required"}),
        );
        return;
    }
    let _ = delete_feishu_app(&state.config.paths.global_config_path, &app_id);
    send_json(
        sender,
        &serde_json::json!({"type": "feishu-app:unregistered", "appId": app_id}),
    );
}

async fn handle_register_qq_app(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let app_id = msg["appId"].as_str().unwrap_or("");
    let app_secret = msg["appSecret"].as_str().unwrap_or("");
    if app_id.is_empty() || app_secret.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "appId and appSecret required"}),
        );
        return;
    }
    let sandbox = msg["sandbox"].as_bool().unwrap_or(false);
    let _ = save_qq_app(
        &state.config.paths.global_config_path,
        app_id,
        app_secret,
        sandbox.then_some(true),
    );
    state.api.add_qq_app_runtime(app_id, app_secret, sandbox);
    send_json(
        sender,
        &serde_json::json!({"type": "qq-app:registered", "appId": app_id}),
    );
}

async fn handle_unregister_qq_app(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let app_id = msg["appId"].as_str().unwrap_or("").to_string();
    if app_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "appId required"}),
        );
        return;
    }
    let _ = delete_qq_app(&state.config.paths.global_config_path, &app_id);
    send_json(
        sender,
        &serde_json::json!({"type": "qq-app:unregistered", "appId": app_id}),
    );
}

async fn handle_list_feishu_apps(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let apps = get_feishu_apps(&state.config.paths.global_config_path);
    let list: Vec<serde_json::Value> = apps
        .iter()
        .map(|(app_id, cfg)| {
            serde_json::json!({
                "appId": app_id,
                "domain": cfg.domain.as_deref().unwrap_or("feishu"),
            })
        })
        .collect();
    send_json(sender, &serde_json::json!({"type": "feishu-apps", "apps": list}));
}

async fn handle_list_dispatch(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let parents = state.api.get_dispatch_parents();
    let parent_count = parents.as_array().map(|a| a.len()).unwrap_or(0);
    tracing::info!(
        "[WsGateway] list:dispatch client #{client_idx}: {parent_count} parent(s)"
    );
    send_json(
        sender,
        &serde_json::json!({"type": "dispatch:update", "parents": parents}),
    );
}

async fn handle_agent_control(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let group_jid = msg["groupJid"].as_str().unwrap_or("").to_string();
    let action = msg["action"].as_str().unwrap_or("").to_string();
    if !group_jid.is_empty() {
        let subscribed = {
            let guard = clients.lock().await;
            guard
                .get(client_idx)
                .map(|c| c.subscriptions.contains(&group_jid))
                .unwrap_or(false)
        };
        if !subscribed {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": "Subscribe to the group before controlling its agent"}),
            );
            return;
        }
    }
    if group_jid.is_empty() || action.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "groupJid and action required"}),
        );
        return;
    }
    if state.group_manager.get(&state.db, &group_jid).is_none() {
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": format!("Group not found: {group_jid}")}),
        );
        return;
    }
    match action.as_str() {
        "pause" => state.api.pause_agent(&group_jid),
        "resume" => {
            let query = msg["query"].as_str();
            state.api.resume_agent(&group_jid, query);
        }
        "stop" => {
            let api = state.api.clone();
            let jid = group_jid;
            tokio::spawn(async move {
                api.stop_agent(&jid).await;
            });
        }
        _ => {
            send_json(
                sender,
                &serde_json::json!({"type": "error", "message": format!("Unknown agent:control action: {action}")}),
            );
        }
    }
}

// ===== Entity CRUD handlers =====

async fn handle_list_channels(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let channels: Vec<ChannelInfoWire> = state
        .channel_manager
        .list(&state.db)
        .unwrap_or_default()
        .iter()
        .map(|c| to_channel_info(c))
        .collect();
    send_json(sender, &serde_json::json!({"type": "channels", "channels": channels}));
}

async fn handle_list_agents(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let agents: Vec<AgentInfoWire> = state
        .agent_manager
        .list(&state.db)
        .unwrap_or_default()
        .iter()
        .map(|a| to_agent_info(a))
        .collect();
    send_json(sender, &serde_json::json!({"type": "agents", "agents": agents}));
}

async fn handle_list_bindings(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    let bindings: Vec<BindingWithRelationsWire> = state
        .binding_manager
        .list_with_relations(&state.db)
        .unwrap_or_default()
        .iter()
        .map(|b| to_binding_with_relations(b))
        .collect();
    send_json(sender, &serde_json::json!({"type": "bindings", "bindings": bindings}));
}

async fn handle_register_channel(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let platform_type = msg["platformType"].as_str().unwrap_or("");
    let name = msg["name"].as_str().unwrap_or("");
    if platform_type.is_empty() || name.is_empty() {
        send_json(sender, &serde_json::json!({"type": "error", "message": "platformType and name are required"}));
        return;
    }
    let credentials = msg["credentials"].clone();
    let now = local_iso_string_now();
    match state.channel_manager.create(&state.db, platform_type, name, &credentials.to_string(), &now) {
        Ok(ch) => {
            let wire = to_channel_info(&ch);
            send_json(sender, &serde_json::json!({"type": "channel:registered", "channel": wire}));
            broadcast_to_all_inner(clients, &serde_json::json!({"type": "channel:registered", "channel": wire})).await;
        }
        Err(e) => {
            send_json(sender, &serde_json::json!({"type": "error", "message": format!("{e}")}));
        }
    }
}

async fn handle_register_agent(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let folder = msg["folder"].as_str().unwrap_or("");
    let name = msg["name"].as_str().unwrap_or("");
    if folder.is_empty() || name.is_empty() {
        send_json(sender, &serde_json::json!({"type": "error", "message": "folder and name are required"}));
        return;
    }
    let requires_trigger = msg["requiresTrigger"].as_bool().unwrap_or(true);
    let allowed_tools: Option<Vec<String>> = msg["allowedTools"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let allowed_work_dirs: Option<Vec<String>> = msg["allowedWorkDirs"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let now = local_iso_string_now();
    match state.agent_manager.create(&state.db, &state.config, folder, name, requires_trigger, allowed_tools.as_ref(), allowed_work_dirs.as_ref(), &now) {
        Ok(a) => {
            let wire = to_agent_info(&a);
            send_json(sender, &serde_json::json!({"type": "agent:registered", "agent": wire}));
            broadcast_to_all_inner(clients, &serde_json::json!({"type": "agent:registered", "agent": wire})).await;
        }
        Err(e) => {
            send_json(sender, &serde_json::json!({"type": "error", "message": format!("{e}")}));
        }
    }
}

async fn handle_register_binding(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let agent_id = msg["agentId"].as_i64().unwrap_or(0);
    let channel_id = msg["channelId"].as_i64().unwrap_or(0);
    if agent_id == 0 || channel_id == 0 {
        send_json(sender, &serde_json::json!({"type": "error", "message": "agentId and channelId are required"}));
        return;
    }
    let jid = msg["jid"].as_str();
    let is_admin = msg["isAdmin"].as_bool().unwrap_or(false);
    let bot_token_override = msg["botTokenOverride"].as_str();
    let max_messages = msg["maxMessages"].as_u64().map(|n| n as u32);
    let now = local_iso_string_now();
    match state.binding_manager.create(&state.db, jid, agent_id, channel_id, is_admin, bot_token_override, max_messages, &now) {
        Ok(b) => {
            // Fetch with relations for the full response
            if let Ok(Some(br)) = state.binding_manager.get_with_relations(&state.db, &b.jid.clone().unwrap_or_default()) {
                let wire = to_binding_with_relations(&br);
                send_json(sender, &serde_json::json!({"type": "binding:registered", "binding": wire}));
                broadcast_to_all_inner(clients, &serde_json::json!({"type": "binding:registered", "binding": wire})).await;
            } else {
                // Fallback: send just the binding without relations
                send_json(sender, &serde_json::json!({"type": "binding:registered", "binding": {"id": b.id}}));
            }
        }
        Err(e) => {
            send_json(sender, &serde_json::json!({"type": "error", "message": format!("{e}")}));
        }
    }
}

async fn handle_unregister_channel(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let id = msg["id"].as_i64().unwrap_or(0);
    if id == 0 {
        send_json(sender, &serde_json::json!({"type": "error", "message": "id is required"}));
        return;
    }
    if let Err(e) = state.channel_manager.delete(&state.db, id) {
        send_json(sender, &serde_json::json!({"type": "error", "message": format!("{e}")}));
        return;
    }
    send_json(sender, &serde_json::json!({"type": "channel:unregistered", "id": id}));
    broadcast_to_all_inner(clients, &serde_json::json!({"type": "channel:unregistered", "id": id})).await;
}

async fn handle_unregister_agent(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let id = msg["id"].as_i64().unwrap_or(0);
    if id == 0 {
        send_json(sender, &serde_json::json!({"type": "error", "message": "id is required"}));
        return;
    }
    if let Err(e) = state.agent_manager.delete(&state.db, id) {
        send_json(sender, &serde_json::json!({"type": "error", "message": format!("{e}")}));
        return;
    }
    send_json(sender, &serde_json::json!({"type": "agent:unregistered", "id": id}));
    broadcast_to_all_inner(clients, &serde_json::json!({"type": "agent:unregistered", "id": id})).await;
}

async fn handle_unregister_binding(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let id = msg["id"].as_i64().unwrap_or(0);
    if id == 0 {
        send_json(sender, &serde_json::json!({"type": "error", "message": "id is required"}));
        return;
    }
    if let Err(e) = state.binding_manager.delete(&state.db, id) {
        send_json(sender, &serde_json::json!({"type": "error", "message": format!("{e}")}));
        return;
    }
    send_json(sender, &serde_json::json!({"type": "binding:unregistered", "id": id}));
    broadcast_to_all_inner(clients, &serde_json::json!({"type": "binding:unregistered", "id": id})).await;
}

async fn handle_update_channel(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let id = msg["id"].as_i64().unwrap_or(0);
    if id == 0 {
        send_json(sender, &serde_json::json!({"type": "error", "message": "id is required"}));
        return;
    }
    let name = msg["name"].as_str();
    let credentials = msg["credentials"].as_object().map(|c| serde_json::to_string(&c).unwrap_or_default());
    let now = local_iso_string_now();
    if let Err(e) = state.channel_manager.update(&state.db, id, name, credentials.as_deref(), &now) {
        send_json(sender, &serde_json::json!({"type": "error", "message": format!("{e}")}));
        return;
    }
    if let Ok(Some(ch)) = state.channel_manager.get(&state.db, id) {
        let wire = to_channel_info(&ch);
        send_json(sender, &serde_json::json!({"type": "channel:updated", "channel": wire}));
        broadcast_to_all_inner(clients, &serde_json::json!({"type": "channel:updated", "channel": wire})).await;
    }
}

async fn handle_update_agent(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let id = msg["id"].as_i64().unwrap_or(0);
    if id == 0 {
        send_json(sender, &serde_json::json!({"type": "error", "message": "id is required"}));
        return;
    }
    let name = msg["name"].as_str();
    let requires_trigger = msg["requiresTrigger"].as_bool();
    let allowed_tools: Option<Vec<String>> = msg["allowedTools"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let allowed_work_dirs: Option<Vec<String>> = msg["allowedWorkDirs"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
    let now = local_iso_string_now();
    if let Err(e) = state.agent_manager.update(&state.db, id, name, requires_trigger, allowed_tools.as_ref(), allowed_work_dirs.as_ref(), &now) {
        send_json(sender, &serde_json::json!({"type": "error", "message": format!("{e}")}));
        return;
    }
    if let Ok(Some(a)) = state.agent_manager.get(&state.db, id) {
        let wire = to_agent_info(&a);
        send_json(sender, &serde_json::json!({"type": "agent:updated", "agent": wire}));
        broadcast_to_all_inner(clients, &serde_json::json!({"type": "agent:updated", "agent": wire})).await;
    }
}

async fn handle_update_binding(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    if !require_admin(clients, client_idx, sender).await {
        return;
    }
    let id = msg["id"].as_i64().unwrap_or(0);
    if id == 0 {
        send_json(sender, &serde_json::json!({"type": "error", "message": "id is required"}));
        return;
    }
    let jid = msg["jid"].as_str();
    let bot_token_override = msg["botTokenOverride"].as_str();
    let max_messages = msg["maxMessages"].as_u64().map(|n| n as u32);
    if let Err(e) = state.binding_manager.update(&state.db, id, jid, bot_token_override, max_messages) {
        send_json(sender, &serde_json::json!({"type": "error", "message": format!("{e}")}));
        return;
    }
    if let Ok(Some(b)) = state.binding_manager.get(&state.db, id) {
        // Fetch with relations for full info
        if let Ok(Some(br)) = state.binding_manager.get_with_relations(&state.db, &b.jid.clone().unwrap_or_default()) {
            let wire = to_binding_with_relations(&br);
            send_json(sender, &serde_json::json!({"type": "binding:updated", "binding": wire}));
            broadcast_to_all_inner(clients, &serde_json::json!({"type": "binding:updated", "binding": wire})).await;
        }
    }
}

// ===== Helpers =====

async fn require_auth(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
) -> bool {
    let guard = clients.lock().await;
    let Some(client) = guard.get(client_idx) else {
        return false;
    };
    if !client.authenticated {
        drop(guard);
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "Not authenticated"}),
        );
        return false;
    }
    true
}

async fn require_admin(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
) -> bool {
    let guard = clients.lock().await;
    let Some(client) = guard.get(client_idx) else {
        return false;
    };
    if !client.is_admin {
        drop(guard);
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "Admin subscription required"}),
        );
        return false;
    }
    true
}

fn send_json(sender: &tokio::sync::mpsc::UnboundedSender<Message>, msg: &serde_json::Value) {
    let _ = sender.send(Message::Text(msg.to_string().into()));
}

async fn send_error(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    message: &str,
) {
    let guard = clients.lock().await;
    if let Some(client) = guard.get(client_idx) {
        let _ = client
            .sender
            .send(Message::Text(
                serde_json::json!({"type": "error", "message": message}).to_string().into(),
            ));
    }
}

async fn broadcast_to_all_inner(clients: &Arc<Mutex<Vec<WsClient>>>, msg: &serde_json::Value) {
    let raw = msg.to_string();
    let guard = clients.lock().await;
    for client in guard.iter() {
        if client.authenticated {
            let _ = client.sender.send(Message::Text(raw.clone().into()));
        }
    }
}

fn now_iso() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    // Simple ISO timestamp matching the project style.
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;
    let (year, month, day) = days_to_ymd(days_since_epoch as i64);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.000Z")
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

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_info_conversion() {
        let g = GroupBinding {
            jid: "tg:group:1".into(),
            folder: "team-a".into(),
            name: "Team A".into(),
            channel: "telegram".into(),
            is_admin: false,
            requires_trigger: true,
            allowed_tools: Some(vec!["Read".into(), "Write".into()]),
            allowed_paths: None,
            allowed_work_dirs: Some(vec!["/tmp".into()]),
            bot_token: Some("tok123".into()),
            max_messages: Some(50),
            last_active: None,
            added_at: "2026-01-01T00:00:00Z".into(),
        };
        let info = to_group_info(&g);
        assert_eq!(info.jid, "tg:group:1");
        assert_eq!(info.folder, "team-a");
        assert!(!info.is_admin);
        assert_eq!(
            info.allowed_tools.as_deref(),
            Some(&["Read".into(), "Write".into()][..])
        );
        assert_eq!(info.max_messages, Some(50));
    }

    #[test]
    fn gateway_new_defaults() {
        let gw = WebSocketGateway::new(18789, Some("secret".into()));
        assert_eq!(gw.port, 18789);
        assert_eq!(gw.token.as_deref(), Some("secret"));
    }

    #[test]
    fn gateway_no_token() {
        let gw = WebSocketGateway::new(18789, None);
        assert_eq!(gw.token, None);
    }
}
