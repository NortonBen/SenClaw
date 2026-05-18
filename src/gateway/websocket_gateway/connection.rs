// ===== Connection handler =====

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{SinkExt, StreamExt};
use tokio::sync::Mutex;

use super::helpers::{send_error, send_json};
use super::state::{WsClient, WsState};

pub(crate) async fn handle_connection(
    ws: WebSocket,
    clients: Arc<Mutex<Vec<WsClient>>>,
    last_known_states: Arc<Mutex<HashMap<String, String>>>,
    pending_interactions: Arc<Mutex<HashMap<String, serde_json::Value>>>,
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
                    &pending_interactions,
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
    pending_interactions: &Arc<Mutex<HashMap<String, serde_json::Value>>>,
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
        "connect" => {
            super::handlers::handle_connect(clients, client_idx, &sender, token, msg).await
        }
        "subscribe" => {
            super::handlers::handle_subscribe(
                clients,
                client_idx,
                &sender,
                last_known_states,
                pending_interactions,
                state,
                msg,
            )
            .await
        }
        "unsubscribe" => {
            super::handlers::handle_unsubscribe(clients, client_idx, &sender, msg).await
        }
        "list:groups" => {
            super::handlers::handle_list_groups(clients, client_idx, &sender, state).await
        }
        "register:group" => {
            super::handlers::handle_register_group(clients, client_idx, &sender, state, msg).await
        }
        "unregister:group" => {
            super::handlers::handle_unregister_group(clients, client_idx, &sender, state, msg).await
        }
        "update:group" => {
            super::handlers::handle_update_group(clients, client_idx, &sender, state, msg).await
        }
        "message" => {
            super::handlers::handle_message_send(clients, client_idx, &sender, state, msg).await
        }
        "permission:response" => {
            super::handlers::handle_permission_response(clients, client_idx, &sender, state, msg)
                .await
        }
        "permission:rule:add" => {
            super::handlers::handle_tool_rule_add(clients, client_idx, &sender, state, msg).await
        }
        "permission:rule:remove" => {
            super::handlers::handle_tool_rule_remove(clients, client_idx, &sender, state, msg).await
        }
        "permission:rule:update" => {
            super::handlers::handle_tool_rule_update(clients, client_idx, &sender, state, msg).await
        }
        "permission:accept-all" => {
            super::handlers::handle_tool_accept_all(clients, client_idx, &sender, state, msg).await
        }
        "question:response" => {
            super::handlers::handle_question_response(clients, client_idx, &sender, state, msg)
                .await
        }
        "list:tasks" => {
            super::handlers::handle_list_tasks(clients, client_idx, &sender, state, msg).await
        }
        "list:task-logs" => {
            super::handlers::handle_task_logs(clients, client_idx, &sender, state, msg).await
        }
        "manage:task" => {
            super::handlers::handle_manage_task(clients, client_idx, &sender, state, msg).await
        }
        "register:feishu-app" => {
            super::handlers::handle_register_feishu_app(clients, client_idx, &sender, state, msg)
                .await
        }
        "unregister:feishu-app" => {
            super::handlers::handle_unregister_feishu_app(clients, client_idx, &sender, state, msg)
                .await
        }
        "register:qq-app" => {
            super::handlers::handle_register_qq_app(clients, client_idx, &sender, state, msg).await
        }
        "unregister:qq-app" => {
            super::handlers::handle_unregister_qq_app(clients, client_idx, &sender, state, msg)
                .await
        }
        "list:feishu-apps" => {
            super::handlers::handle_list_feishu_apps(clients, client_idx, &sender, state).await
        }
        "list:dispatch" => {
            super::handlers::handle_list_dispatch(clients, client_idx, &sender, state).await
        }
        "agent:control" => {
            super::handlers::handle_agent_control(clients, client_idx, &sender, state, msg).await
        }
        "agent:mode" => {
            super::handlers::handle_agent_mode(clients, client_idx, &sender, state, msg).await
        }
        "list:channels" => {
            super::entity_handlers::handle_list_channels(clients, client_idx, &sender, state).await
        }
        "list:agents" => {
            super::entity_handlers::handle_list_agents(clients, client_idx, &sender, state).await
        }
        "list:bindings" => {
            super::entity_handlers::handle_list_bindings(clients, client_idx, &sender, state).await
        }
        "register:channel" => {
            super::entity_handlers::handle_register_channel(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "register:agent" => {
            super::entity_handlers::handle_register_agent(clients, client_idx, &sender, state, msg)
                .await
        }
        "register:binding" => {
            super::entity_handlers::handle_register_binding(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "unregister:channel" => {
            super::entity_handlers::handle_unregister_channel(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "unregister:agent" => {
            super::entity_handlers::handle_unregister_agent(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "unregister:binding" => {
            super::entity_handlers::handle_unregister_binding(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "update:channel" => {
            super::entity_handlers::handle_update_channel(clients, client_idx, &sender, state, msg)
                .await
        }
        "update:agent" => {
            super::entity_handlers::handle_update_agent(clients, client_idx, &sender, state, msg)
                .await
        }
        "update:binding" => {
            super::entity_handlers::handle_update_binding(clients, client_idx, &sender, state, msg)
                .await
        }
        // Cowork
        "list:cowork:workspaces" => {
            super::cowork_handlers::handle_cowork_ws_list(clients, client_idx, &sender, state).await
        }
        "create:cowork:workspace" => {
            super::cowork_handlers::handle_cowork_ws_create(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "update:cowork:workspace" => {
            super::cowork_handlers::handle_cowork_ws_update(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "delete:cowork:workspace" => {
            super::cowork_handlers::handle_cowork_ws_delete(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "list:cowork:members" => {
            super::cowork_handlers::handle_cowork_members_list(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "add:cowork:member" => {
            super::cowork_handlers::handle_cowork_member_add(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "update:cowork:member" => {
            super::cowork_handlers::handle_cowork_member_update(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "remove:cowork:member" => {
            super::cowork_handlers::handle_cowork_member_remove(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "list:cowork:board" => {
            super::cowork_handlers::handle_cowork_board_list(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "update:cowork:board" => {
            super::cowork_handlers::handle_cowork_board_update(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "list:cowork:tasks" => {
            super::cowork_handlers::handle_cowork_tasks_list(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "create:cowork:task" => {
            super::cowork_handlers::handle_cowork_task_create(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "update:cowork:task" => {
            super::cowork_handlers::handle_cowork_task_update(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "delete:cowork:task" => {
            super::cowork_handlers::handle_cowork_task_delete(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "send:cowork:message" => {
            super::cowork_handlers::handle_cowork_message_send(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        "list:cowork:messages" => {
            super::cowork_handlers::handle_cowork_messages_list(
                clients, client_idx, &sender, state, msg,
            )
            .await
        }
        _ => {
            send_json(
                &sender,
                &serde_json::json!({"type": "error", "message": format!("Unknown message type: {msg_type}")}),
            );
        }
    }
}
