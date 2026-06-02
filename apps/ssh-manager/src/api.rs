use crate::client::SshClient;
use crate::models::{Host, HostStore};
use crate::keychain::{KeychainItem, KeychainStore};
use crate::security::CommandFilter;
use axum::{
    extract::{Path, State, ws::{WebSocketUpgrade, WebSocket}},
    response::IntoResponse,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct SshExecuteRequest {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Option<String>,
    pub command: String,
}

#[derive(Serialize)]
pub struct SshExecuteResponse {
    pub output: String,
    pub error: Option<String>,
}

pub struct AppState {
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
    pub filter: CommandFilter,
    pub hosts: HostStore,
    pub keychain: KeychainStore,
    pub connections: crate::connection::ConnectionManager,
    pub port_forwarding_store: crate::models::PortForwardingStore,
    pub port_forwarding_manager: crate::port_forwarding::PortForwardingManager,
}

pub fn api_router() -> Router {
    let allowed_commands_env = std::env::var("ALLOWED_COMMANDS")
        .unwrap_or_else(|_| "ls,pwd,whoami,df,free,uname,cat,echo,ps,top".to_string());

    let allowed: Vec<String> = allowed_commands_env
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let filter = CommandFilter::new(allowed);
    let data_dir = dirs::home_dir().unwrap().join(".senclaw").join("space-apps-data").join("ssh-manager");
    std::fs::create_dir_all(&data_dir).unwrap();
    let hosts_path = data_dir.join("hosts.json");
    let hosts = HostStore::new(hosts_path.to_str().unwrap());
    let keychain_path = data_dir.join("keychain.json");
    let keychain = KeychainStore::new(keychain_path.to_str().unwrap());
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let connections = crate::connection::ConnectionManager::new();

    let port_forwarding_path = data_dir.join("port_forwarding.json");
    let port_forwarding_store = crate::models::PortForwardingStore::new(port_forwarding_path.to_str().unwrap());
    let port_forwarding_manager = crate::port_forwarding::PortForwardingManager::new();

    let state = Arc::new(AppState { filter, hosts, keychain, mcp_tx, connections, port_forwarding_store, port_forwarding_manager });

    Router::new()
        .nest("/sftp", crate::sftp_api::sftp_router())
        .merge(crate::port_forwarding::port_forwarding_router())
        .route("/execute", post(execute_command))
        .route("/hosts", get(list_hosts).post(create_host))
        .route("/hosts/:id", put(update_host).delete(delete_host))
        .route("/keychain", get(list_keychain).post(create_keychain))
        .route("/keychain/:id", put(update_keychain).delete(delete_keychain))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .route("/ws/terminal/:id", get(ws_terminal_handler))
        .with_state(state)
}

async fn list_hosts(State(state): State<Arc<AppState>>) -> Json<Vec<Host>> {
    Json(state.hosts.get_all())
}

async fn create_host(State(state): State<Arc<AppState>>, Json(host): Json<Host>) -> Json<Host> {
    Json(state.hosts.add(host))
}

async fn update_host(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(host): Json<Host>,
) -> axum::response::Result<Json<Host>, axum::http::StatusCode> {
    if let Some(updated) = state.hosts.update(&id, host) {
        Ok(Json(updated))
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}

async fn delete_host(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> axum::response::Result<Json<bool>, axum::http::StatusCode> {
    if state.hosts.delete(&id) {
        Ok(Json(true))
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}

async fn execute_command(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SshExecuteRequest>,
) -> Json<SshExecuteResponse> {
    // CommandFilter is disabled for MCP / AI Agent unrestricted access
    
    let mut password = payload.password.clone();
    let mut key_pair = None;

    // MCP currently doesn't provide keychain_id directly in payload unless we updated it.
    // If we wanted to support it, we'd need to add it to SshExecuteRequest.
    // But let's check if the host in HostStore has it, or if payload has it.
    // Since MCP provides host ip/port/user/password directly, we might not have a host in HostStore.
    // However, if the payload matched a host in the store, we could look it up.
    // For now, let's just use what's in payload. If we want MCP to use keychain, we should update SshExecuteRequest to accept keychain_id.
    let mut client = match SshClient::connect(
        &payload.host,
        payload.port,
        &payload.user,
        password.as_deref(),
        key_pair,
    )
    .await
    {
        Ok(c) => c,
        Err(e) => {
            return Json(SshExecuteResponse {
                output: String::new(),
                error: Some(format!("Connection failed: {}", e)),
            });
        }
    };

    match client.execute(&payload.command).await {
        Ok(out) => Json(SshExecuteResponse {
            output: out,
            error: None,
        }),
        Err(e) => Json(SshExecuteResponse {
            output: String::new(),
            error: Some(format!("Execution failed: {}", e)),
        }),
    }
}

async fn ws_terminal_handler(
    ws: WebSocketUpgrade,
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let host_opt = state.hosts.get_all().into_iter().find(|h| h.id == id);
    
    ws.on_upgrade(move |socket| async move {
        if let Some(host) = host_opt {
            let mut password = host.password.clone();
            let mut key_pair = None;

            if let Some(kid) = &host.keychain_id {
                if let Some(kitem) = state.keychain.get(kid) {
                    match kitem.item_type {
                        crate::keychain::KeychainItemType::Password => {
                            password = Some(kitem.value);
                        }
                        crate::keychain::KeychainItemType::PrivateKey => {
                            // russh_keys::decode_secret_key requires the key and an optional password
                            if let Ok(kp) = russh_keys::decode_secret_key(kitem.value.as_str(), password.as_deref()) {
                                key_pair = Some(kp);
                            } else {
                                eprintln!("Failed to parse private key");
                            }
                        }
                    }
                }
            }

            match SshClient::connect(
                &host.host,
                host.port,
                &host.user,
                password.as_deref(),
                key_pair,
            ).await {
                Ok(mut client) => {
                    if let Err(e) = client.interactive_shell(socket).await {
                        eprintln!("Interactive shell error: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("SSH Connection failed: {}", e);
                    // Could optionally send an error message to the socket before closing
                    let mut s = socket;
                    let _ = s.send(axum::extract::ws::Message::Text(format!("\\r\\n\\x1b[31mSSH Connection Failed: {}\\x1b[0m\\r\\n", e))).await;
                }
            }
        } else {
            let mut s = socket;
            let _ = s.send(axum::extract::ws::Message::Text("\\r\\n\\x1b[31mHost not found.\\x1b[0m\\r\\n".to_string())).await;
        }
    })
}

async fn list_keychain(State(state): State<Arc<AppState>>) -> Json<Vec<KeychainItem>> {
    Json(state.keychain.get_all())
}

async fn create_keychain(State(state): State<Arc<AppState>>, Json(item): Json<KeychainItem>) -> Json<KeychainItem> {
    Json(state.keychain.add(item))
}

async fn update_keychain(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(item): Json<KeychainItem>,
) -> axum::response::Result<Json<KeychainItem>, axum::http::StatusCode> {
    if let Some(updated) = state.keychain.update(&id, item) {
        Ok(Json(updated))
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}

async fn delete_keychain(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> axum::response::Result<Json<bool>, axum::http::StatusCode> {
    if state.keychain.delete(&id) {
        Ok(Json(true))
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}
