use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::{fs, io::{AsyncReadExt, AsyncWriteExt}};
use crate::api::AppState;

#[derive(Serialize)]
pub struct FileNode {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_time: i64,
}

#[derive(Deserialize)]
pub struct PathQuery {
    pub path: String,
}

#[derive(Deserialize)]
pub struct TransferRequest {
    pub source: String,
    pub target: String,
}

#[derive(Deserialize)]
pub struct SftpConnectRequest {
    pub host_id: String,
}

#[derive(Serialize)]
pub struct SftpConnectResponse {
    pub conn_id: String,
}

pub fn sftp_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/local/ls", get(local_ls))
        .route("/remote/:conn_id/ls", get(remote_ls))
        .route("/remote/:conn_id/download", post(download_file))
        .route("/remote/:conn_id/upload", post(upload_file))
        .route("/connect", post(connect_sftp))
}

async fn local_ls(Query(query): Query<PathQuery>) -> axum::response::Result<Json<Vec<FileNode>>, axum::http::StatusCode> {
    let mut entries = match fs::read_dir(&query.path).await {
        Ok(e) => e,
        Err(_) => return Err(axum::http::StatusCode::NOT_FOUND),
    };

    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(metadata) = entry.metadata().await {
            let modified = metadata.modified()
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;

            files.push(FileNode {
                name: entry.file_name().to_string_lossy().to_string(),
                path: entry.path().to_string_lossy().to_string(),
                is_dir: metadata.is_dir(),
                size: metadata.len(),
                modified_time: modified,
            });
        }
    }
    
    // Sort directories first, then alphabetical
    files.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));

    Ok(Json(files))
}

async fn remote_ls(
    Path(conn_id): Path<String>,
    Query(query): Query<PathQuery>,
    State(state): State<Arc<AppState>>,
) -> axum::response::Result<Json<Vec<FileNode>>, axum::http::StatusCode> {
    if let Some(client_arc) = state.connections.get(&conn_id).await {
        let mut client = client_arc.lock().await;
        
        let path = if query.path.is_empty() || query.path == "." {
            String::from(".") // Get default remote dir if empty
        } else {
            query.path.clone()
        };

        if let Ok(mut sftp) = client.get_sftp().await {
            let mut files = Vec::new();
            if let Ok(entries) = sftp.read_dir(path.clone()).await {
                for entry in entries {
                    let name_str = entry.file_name();
                    if name_str == "." || name_str == ".." {
                        continue;
                    }
                    
                    let is_dir = entry.metadata().is_dir();
                    let size = entry.metadata().len();
                    let modified_time = entry.metadata().modified()
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                        
                    let full_path = entry.path();

                    files.push(FileNode {
                        name: name_str,
                        path: full_path,
                        is_dir,
                        size,
                        modified_time,
                    });
                }
            }
            files.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
            Ok(Json(files))
        } else {
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}

async fn download_file(
    Path(conn_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<TransferRequest>,
) -> axum::response::Result<Json<bool>, axum::http::StatusCode> {
    if let Some(client_arc) = state.connections.get(&conn_id).await {
        let mut client = client_arc.lock().await;
        if let Ok(mut sftp) = client.get_sftp().await {
            let mut file = match sftp.open(payload.source.clone()).await {
                Ok(f) => f,
                Err(_) => return Err(axum::http::StatusCode::NOT_FOUND),
            };
            
            let mut contents = Vec::new();
            if file.read_to_end(&mut contents).await.is_err() {
                return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
            }
            
            if fs::write(&payload.target, contents).await.is_err() {
                return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
            }
            
            Ok(Json(true))
        } else {
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}

async fn upload_file(
    Path(conn_id): Path<String>,
    State(state): State<Arc<AppState>>,
    Json(payload): Json<TransferRequest>,
) -> axum::response::Result<Json<bool>, axum::http::StatusCode> {
    if let Some(client_arc) = state.connections.get(&conn_id).await {
        let contents = match fs::read(&payload.source).await {
            Ok(c) => c,
            Err(_) => return Err(axum::http::StatusCode::NOT_FOUND),
        };

        let mut client = client_arc.lock().await;
        if let Ok(mut sftp) = client.get_sftp().await {
            let mut file = match sftp.create(payload.target.clone()).await {
                Ok(f) => f,
                Err(_) => return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
            };
            
            if file.write_all(&contents).await.is_err() {
                return Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
            }
            
            Ok(Json(true))
        } else {
            Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
        }
    } else {
        Err(axum::http::StatusCode::NOT_FOUND)
    }
}

async fn connect_sftp(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SftpConnectRequest>,
) -> axum::response::Result<Json<SftpConnectResponse>, axum::http::StatusCode> {
    let host = match state.hosts.get_all().into_iter().find(|h| h.id == payload.host_id) {
        Some(h) => h,
        None => return Err(axum::http::StatusCode::NOT_FOUND),
    };

    let mut password = host.password.clone();
    let mut key_pair = None;

    if let Some(kid) = &host.keychain_id {
        if let Some(kitem) = state.keychain.get(kid) {
            match kitem.item_type {
                crate::keychain::KeychainItemType::Password => {
                    password = Some(kitem.value);
                }
                crate::keychain::KeychainItemType::PrivateKey => {
                    if let Ok(kp) = russh_keys::decode_secret_key(kitem.value.as_str(), password.as_deref()) {
                        key_pair = Some(kp);
                    }
                }
            }
        }
    }

    match crate::client::SshClient::connect(
        &host.host,
        host.port,
        &host.user,
        password.as_deref(),
        key_pair,
    ).await {
        Ok(client) => {
            let conn_id = state.connections.add(client).await;
            Ok(Json(SftpConnectResponse { conn_id }))
        }
        Err(_) => Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
    }
}
