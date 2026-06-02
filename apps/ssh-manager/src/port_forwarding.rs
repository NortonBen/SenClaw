use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use std::collections::HashMap;

use crate::api::AppState;
use crate::models::PortForwardingRule;

pub struct PortForwardingManager {
    // Maps rule ID to a cancellation sender
    active_tunnels: Mutex<HashMap<String, tokio::sync::oneshot::Sender<()>>>,
}

impl PortForwardingManager {
    pub fn new() -> Self {
        Self {
            active_tunnels: Mutex::new(HashMap::new()),
        }
    }

    pub async fn start_tunnel(&self, state: Arc<AppState>, rule: PortForwardingRule) -> Result<(), String> {
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        
        let bind_addr = format!("{}:{}", rule.bind_address, rule.local_port);
        let listener = TcpListener::bind(&bind_addr).await.map_err(|e| e.to_string())?;
        
        let mut tunnels = self.active_tunnels.lock().await;
        if tunnels.contains_key(&rule.id) {
            return Err("Tunnel already active".to_string());
        }
        tunnels.insert(rule.id.clone(), tx);
        drop(tunnels);

        let rule_id = rule.id.clone();
        
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok((mut stream, _addr)) = listener.accept() => {
                        let state_clone = state.clone();
                        let rule_clone = rule.clone();
                        
                        tokio::spawn(async move {
                            // First, get the SSH connection
                            let hosts = state_clone.hosts.get_all();
                            if let Some(host) = hosts.into_iter().find(|h| h.id == rule_clone.host_id) {
                                // For now, we will just use the client logic to open a channel.
                                // In a real scenario, we might want to reuse a connection or start a new one.
                                // Since we already have ConnectionManager logic, we can try to find an active connection or start one.
                                // For simplicity, we can reuse the connection map or establish a dedicated one.
                                
                                // Let's try to get an existing connection
                                if let Some(client_arc) = state_clone.connections.get(&host.id).await {
                                    let mut client = client_arc.lock().await;
                                    if let Ok(channel) = client.handle.channel_open_direct_tcpip(&rule_clone.destination_address, rule_clone.destination_port as u32, "localhost", 0).await {
                                        let mut channel_stream = channel.into_stream();
                                        // Pipe traffic
                                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut channel_stream).await;
                                    }
                                } else {
                                    // Connection not found, ideally we should connect here, but for now we just error out.
                                    // Let's implement a quick connect
                                    let mut password = host.password.clone();
                                    let mut key_pair = None;
                                    if let Some(keychain_id) = &host.keychain_id {
                                        if let Some(item) = state_clone.keychain.get(keychain_id) {
                                            if item.item_type == crate::keychain::KeychainItemType::Password {
                                                password = Some(item.value.clone());
                                            } else if item.item_type == crate::keychain::KeychainItemType::PrivateKey {
                                                if let Ok(keys) = russh_keys::decode_secret_key(&item.value, None) {
                                                    key_pair = Some(keys);
                                                }
                                            }
                                        }
                                    }
                                    if let Ok(mut client) = crate::client::SshClient::connect(&host.host, host.port, &host.user, password.as_deref(), key_pair).await {
                                        if let Ok(channel) = client.handle.channel_open_direct_tcpip(&rule_clone.destination_address, rule_clone.destination_port as u32, "localhost", 0).await {
                                            let mut channel_stream = channel.into_stream();
                                            let _ = tokio::io::copy_bidirectional(&mut stream, &mut channel_stream).await;
                                        }
                                    }
                                }
                            }
                        });
                    }
                    _ = &mut rx => {
                        // Cancellation received
                        break;
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn stop_tunnel(&self, rule_id: &str) {
        let mut tunnels = self.active_tunnels.lock().await;
        if let Some(tx) = tunnels.remove(rule_id) {
            let _ = tx.send(());
        }
    }
}

pub fn port_forwarding_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/port-forwarding", get(list_rules).post(create_rule))
        .route("/port-forwarding/:id", put(update_rule).delete(delete_rule))
        .route("/port-forwarding/:id/start", post(start_rule))
        .route("/port-forwarding/:id/stop", post(stop_rule))
}

async fn list_rules(State(state): State<Arc<AppState>>) -> axum::response::Result<Json<Vec<PortForwardingRule>>, axum::http::StatusCode> {
    Ok(Json(state.port_forwarding_store.get_all()))
}

async fn create_rule(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PortForwardingRule>,
) -> axum::response::Result<Json<PortForwardingRule>, axum::http::StatusCode> {
    let rule = state.port_forwarding_store.add(payload);
    Ok(Json(rule))
}

async fn update_rule(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PortForwardingRule>,
) -> axum::response::Result<Json<PortForwardingRule>, axum::http::StatusCode> {
    if let Some(rule) = state.port_forwarding_store.update(&id, payload) {
        Ok(Json(rule))
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}

async fn delete_rule(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> axum::response::Result<Json<bool>, axum::http::StatusCode> {
    // Stop tunnel if active
    state.port_forwarding_manager.stop_tunnel(&id).await;
    
    if state.port_forwarding_store.delete(&id) {
        Ok(Json(true))
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}

async fn start_rule(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> axum::response::Result<Json<bool>, axum::http::StatusCode> {
    if let Some(mut rule) = state.port_forwarding_store.get(&id) {
        if let Err(e) = state.port_forwarding_manager.start_tunnel(state.clone(), rule.clone()).await {
            // Probably should return 400 or 500 with error
            return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
        }
        
        rule.active = true;
        state.port_forwarding_store.update(&id, rule);
        Ok(Json(true))
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}

async fn stop_rule(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> axum::response::Result<Json<bool>, axum::http::StatusCode> {
    if let Some(mut rule) = state.port_forwarding_store.get(&id) {
        state.port_forwarding_manager.stop_tunnel(&id).await;
        
        rule.active = false;
        state.port_forwarding_store.update(&id, rule);
        Ok(Json(true))
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}
