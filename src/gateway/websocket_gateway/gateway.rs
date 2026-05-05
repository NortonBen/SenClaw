//! WebSocket gateway core: WsGatewayApi trait, WebSocketGateway struct, route, broadcast.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use axum::extract::ws::Message;
use tokio::sync::Mutex;

use crate::types::GroupBinding;

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

// ===== WebSocketGateway =====

pub struct WebSocketGateway {
    pub port: u16,
    pub(crate) token: Option<String>,
    pub(crate) clients: Arc<Mutex<Vec<super::state::WsClient>>>,
    pub(crate) last_known_states: Arc<Mutex<HashMap<String, String>>>,
    /// Pending permission:request and question:request messages keyed by requestId.
    /// Stored so they can be replayed as a snapshot when an admin client (re)connects.
    pub(crate) pending_interactions: Arc<Mutex<HashMap<String, serde_json::Value>>>,
}

impl WebSocketGateway {
    pub fn new(port: u16, token: Option<String>) -> Self {
        Self {
            port,
            token,
            clients: Arc::new(Mutex::new(Vec::new())),
            last_known_states: Arc::new(Mutex::new(HashMap::new())),
            pending_interactions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Build the axum route for WebSocket upgrade at `/ws`.
    /// Returns the router and handles for external event injection.
    pub fn route(&self, state: Arc<super::state::WsState>) -> axum::Router {
        let clients = self.clients.clone();
        let states = self.last_known_states.clone();
        let pending_interactions = self.pending_interactions.clone();
        let token = self.token.clone();

        let main_route = {
            let clients = clients.clone();
            let states = states.clone();
            let pending_interactions = pending_interactions.clone();
            let token = token.clone();
            let state = state.clone();
            axum::routing::get(move |ws: axum::extract::WebSocketUpgrade| {
                let clients = clients.clone();
                let states = states.clone();
                let pending_interactions = pending_interactions.clone();
                let token = token.clone();
                let state = state.clone();
                async move {
                    ws.on_upgrade(move |socket| {
                        super::connection::handle_connection(
                            socket,
                            clients,
                            states,
                            pending_interactions,
                            token,
                            state,
                        )
                    })
                }
            })
        };

        let browser_relay = state.browser_relay.clone();
        let browser_route = axum::routing::get(move |ws: axum::extract::WebSocketUpgrade| {
            let relay = browser_relay.clone();
            async move {
                ws.on_upgrade(move |socket| {
                    super::browser::handle_browser_connection(socket, relay)
                })
            }
        });

        let browser_relay2 = state.browser_relay.clone();
        let browser_mcp_route = axum::routing::get(move |ws: axum::extract::WebSocketUpgrade| {
            let relay = browser_relay2.clone();
            async move {
                ws.on_upgrade(move |socket| {
                    super::browser::handle_browser_mcp_connection(socket, relay)
                })
            }
        });

        axum::Router::new()
            .route("/", main_route)
            .route("/browser", browser_route)
            .route("/browser-mcp", browser_mcp_route)
    }

    // ===== Broadcast helpers =====

    pub(crate) async fn broadcast_to_admins(&self, msg: &serde_json::Value) {
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
        if sent == 0
            && (msg_type == "dispatch:update"
                || msg_type == "agent:todos"
                || msg_type == "task:backlog")
        {
            tracing::warn!(
                "[WsGateway] {msg_type} fired but NO admin clients connected — \
                 web client must subscribe to an is_admin group first"
            );
        }
    }

    /// Broadcast to admin clients that are NOT already subscribed to `skip_jid`.
    /// Used for permission events on non-admin groups: group subscribers get it via
    /// `broadcast(jid, ...)`, and admins watching the Agent Console get it here
    /// without receiving a duplicate if they happen to also subscribe to that group.
    pub(crate) async fn broadcast_to_admins_excluding(
        &self,
        skip_jid: &str,
        msg: &serde_json::Value,
    ) {
        let raw = msg.to_string();
        let clients = self.clients.lock().await;
        for client in clients.iter() {
            if client.authenticated && client.is_admin && !client.subscriptions.contains(skip_jid) {
                let _ = client.sender.send(Message::Text(raw.clone().into()));
            }
        }
    }

    pub(crate) async fn broadcast_to_all(&self, msg: &serde_json::Value) {
        let raw = msg.to_string();
        let clients = self.clients.lock().await;
        for client in clients.iter() {
            if client.authenticated {
                let _ = client.sender.send(Message::Text(raw.clone().into()));
            }
        }
    }

    pub(crate) async fn broadcast(&self, group_jid: &str, msg: &serde_json::Value) {
        let raw = msg.to_string();
        let clients = self.clients.lock().await;
        for client in clients.iter() {
            if client.authenticated && client.subscriptions.contains(group_jid) {
                let _ = client.sender.send(Message::Text(raw.clone().into()));
            }
        }
    }
}
