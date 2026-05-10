// ============================================================
// Cowork WS handlers
// ============================================================

use std::sync::Arc;

use axum::extract::ws::Message;
use tokio::sync::Mutex;

use super::helpers::send_json;
use super::state::{WsClient, WsState};

pub(crate) async fn handle_cowork_ws_list(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
) {
    match state.cowork_manager.list_workspaces(&state.db) {
        Ok(wss) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:workspaces","workspaces":wss}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_ws_create(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let name = msg["name"].as_str().unwrap_or("");
    if name.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"name required"}),
        );
        return;
    }
    let desc = msg["description"].as_str();
    let now = chrono::Utc::now().to_rfc3339();
    match state.cowork_manager.create_workspace(
        &state.db,
        &state.config,
        name,
        desc,
        msg["workingDir"].as_str(),
        &now,
    ) {
        Ok(ws) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:workspace:created","workspace":ws}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_ws_update(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let id = msg["id"].as_str().unwrap_or("");
    if id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"id required"}),
        );
        return;
    }
    let now = chrono::Utc::now().to_rfc3339();
    match state.cowork_manager.update_workspace(
        &state.db,
        id,
        msg["name"].as_str(),
        msg["description"].as_str(),
        msg["status"].as_str(),
        msg["workingDir"].as_str(),
        &now,
    ) {
        Ok(()) => {
            if let Ok(Some(ws)) = state.cowork_manager.get_workspace(&state.db, id) {
                send_json(
                    sender,
                    &serde_json::json!({"type":"cowork:workspace:updated","workspace":ws}),
                );
            }
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_ws_delete(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let id = msg["id"].as_str().unwrap_or("");
    if id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"id required"}),
        );
        return;
    }
    match state.cowork_manager.delete_workspace(&state.db, id) {
        Ok(()) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:workspace:deleted","id":id}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_members_list(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    if ws_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId required"}),
        );
        return;
    }
    match state.cowork_manager.list_members(&state.db, ws_id) {
        Ok(members) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:members","workspaceId":ws_id,"members":members}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_member_add(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    let member_id = msg["memberId"].as_str().unwrap_or("");
    if ws_id.is_empty() || member_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId and memberId required"}),
        );
        return;
    }
    let role = msg["role"].as_str().unwrap_or("worker");
    let now = chrono::Utc::now().to_rfc3339();
    match state.cowork_manager.add_member(
        &state.db,
        &state.config,
        ws_id,
        member_id,
        role,
        msg["jid"].as_str(),
        msg["subdir"].as_str(),
        &now,
    ) {
        Ok(member) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:member:added","workspaceId":ws_id,"member":member}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_member_update(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    let member_id = msg["memberId"].as_str().unwrap_or("");
    if ws_id.is_empty() || member_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId and memberId required"}),
        );
        return;
    }
    let now = chrono::Utc::now().to_rfc3339();
    match state.cowork_manager.update_member_spec(
        &state.db,
        ws_id,
        member_id,
        msg["role"].as_str(),
        msg["persona"].as_str(),
        msg["responsibilities"].as_str(),
        msg["triggers"].as_str(),
        msg["handoffRules"].as_str(),
        msg["acceptanceCriteria"].as_str(),
        msg["outputFormat"].as_str(),
        msg["sla"].as_str(),
        msg["limits"].as_str(),
        &now,
    ) {
        Ok(()) => {
            if let Ok(Some(member)) = state.cowork_manager.get_member(&state.db, ws_id, member_id) {
                send_json(
                    sender,
                    &serde_json::json!({"type":"cowork:member:updated","workspaceId":ws_id,"member":member}),
                );
            }
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_member_remove(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    let member_id = msg["memberId"].as_str().unwrap_or("");
    if ws_id.is_empty() || member_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId and memberId required"}),
        );
        return;
    }
    match state
        .cowork_manager
        .remove_member(&state.db, ws_id, member_id)
    {
        Ok(()) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:member:removed","workspaceId":ws_id,"memberId":member_id}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_board_list(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    if ws_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId required"}),
        );
        return;
    }
    match state
        .cowork_manager
        .get_board(&state.db, ws_id, msg["section"].as_str())
    {
        Ok(entries) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:board","workspaceId":ws_id,"entries":entries}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_board_update(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    let section = msg["section"].as_str().unwrap_or("");
    let content = msg["content"].as_str().unwrap_or("");
    if ws_id.is_empty() || section.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId and section required"}),
        );
        return;
    }
    let author = msg["author"].as_str().unwrap_or("system");
    let now = chrono::Utc::now().to_rfc3339();
    match state.cowork_manager.upsert_board_entry(
        &state.db,
        ws_id,
        section,
        msg["title"].as_str(),
        content,
        author,
        &now,
    ) {
        Ok(entry) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:board:updated","workspaceId":ws_id,"entry":entry}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_tasks_list(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    if ws_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId required"}),
        );
        return;
    }
    match state
        .cowork_manager
        .list_tasks(&state.db, ws_id, msg["status"].as_str())
    {
        Ok(tasks) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:tasks","workspaceId":ws_id,"tasks":tasks}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_task_create(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    let title = msg["title"].as_str().unwrap_or("");
    if ws_id.is_empty() || title.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId and title required"}),
        );
        return;
    }
    let created_by = msg["createdBy"].as_str().unwrap_or("user");
    let now = chrono::Utc::now().to_rfc3339();
    match state.cowork_manager.create_task(
        &state.db,
        ws_id,
        title,
        msg["description"].as_str(),
        msg["assignee"].as_str(),
        msg["reviewer"].as_str(),
        msg["priority"].as_str(),
        msg["dependsOn"].as_str(),
        created_by,
        msg["attachments"].as_str(),
        &now,
    ) {
        Ok(task) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:task:created","workspaceId":ws_id,"task":task}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_task_update(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    let task_id = msg["taskId"].as_str().unwrap_or("");
    if ws_id.is_empty() || task_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId and taskId required"}),
        );
        return;
    }
    let now = chrono::Utc::now().to_rfc3339();
    let agent_api = state.agent_api.clone();
    let mgr_arc = Arc::clone(&state.cowork_manager);

    let result = if let Some(api) = agent_api {
        state.cowork_manager.update_task_with_triggers(
            &Arc::clone(&state.db),
            task_id,
            msg["title"].as_str(),
            msg["description"].as_str(),
            msg["status"].as_str(),
            msg["assignee"].as_str(),
            msg["reviewer"].as_str(),
            msg["priority"].as_str(),
            msg["dependsOn"].as_str(),
            msg["attachments"].as_str(),
            &now,
            Some(api),
            mgr_arc,
        )
    } else {
        state.cowork_manager.update_task(
            &state.db,
            task_id,
            msg["title"].as_str(),
            msg["description"].as_str(),
            msg["status"].as_str(),
            msg["assignee"].as_str(),
            msg["reviewer"].as_str(),
            msg["priority"].as_str(),
            msg["dependsOn"].as_str(),
            msg["attachments"].as_str(),
            &now,
        )
    };

    match result {
        Ok(()) => {
            if let Ok(Some(task)) = state.cowork_manager.get_task(&state.db, task_id) {
                send_json(
                    sender,
                    &serde_json::json!({"type":"cowork:task:updated","workspaceId":ws_id,"task":task}),
                );
            }
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_task_delete(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    let task_id = msg["taskId"].as_str().unwrap_or("");
    if task_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"taskId required"}),
        );
        return;
    }
    match state.cowork_manager.delete_task(&state.db, task_id) {
        Ok(()) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:task:deleted","workspaceId":ws_id,"taskId":task_id}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}

pub(crate) async fn handle_cowork_message_send(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    let from = msg["fromMember"].as_str().unwrap_or("");
    let message_type = msg["messageType"].as_str();
    if ws_id.is_empty() || from.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId and fromMember required"}),
        );
        return;
    }
    let content = msg["content"].as_str().unwrap_or("");
    let now = chrono::Utc::now().to_rfc3339();

    // If the workspace has members, use process_user_message to decompose and dispatch tasks.
    // Otherwise fall back to simple send_message.
    let has_members = state
        .cowork_manager
        .list_members(&state.db, ws_id)
        .map(|m| !m.is_empty())
        .unwrap_or(false);

    if has_members {
        let agent_api = state
            .agent_api
            .as_ref()
            .map(|api| (Arc::clone(api), Arc::clone(&state.db)));
        match state.cowork_manager.process_user_message(
            &state.db,
            ws_id,
            from,
            content,
            message_type,
            &now,
            agent_api,
            Arc::clone(&state.cowork_manager),
        ) {
            Ok((cmsg, tasks)) => {
                send_json(
                    sender,
                    &serde_json::json!({
                        "type": "cowork:message:sent",
                        "workspaceId": ws_id,
                        "message": cmsg,
                        "tasks": tasks,
                    }),
                );
            }
            Err(e) => {
                send_json(
                    sender,
                    &serde_json::json!({"type":"error","message":e.to_string()}),
                );
            }
        }
    } else {
        match state.cowork_manager.send_message(
            &state.db,
            ws_id,
            from,
            msg["toMember"].as_str(),
            msg["messageType"].as_str().unwrap_or("status"),
            content,
            msg["taskId"].as_str(),
            msg["attachments"].as_str(),
            &now,
        ) {
            Ok(cmsg) => {
                send_json(
                    sender,
                    &serde_json::json!({"type":"cowork:message:sent","workspaceId":ws_id,"message":cmsg}),
                );
            }
            Err(e) => {
                send_json(
                    sender,
                    &serde_json::json!({"type":"error","message":e.to_string()}),
                );
            }
        }
    }
}

pub(crate) async fn handle_cowork_messages_list(
    _clients: &Arc<Mutex<Vec<WsClient>>>,
    _client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
    msg: &serde_json::Value,
) {
    let ws_id = msg["workspaceId"].as_str().unwrap_or("");
    if ws_id.is_empty() {
        send_json(
            sender,
            &serde_json::json!({"type":"error","message":"workspaceId required"}),
        );
        return;
    }
    let limit = msg["limit"].as_u64().unwrap_or(50) as u32;
    match state
        .db
        .list_cowork_messages(ws_id, limit, msg["since"].as_str())
    {
        Ok(msgs) => {
            send_json(
                sender,
                &serde_json::json!({"type":"cowork:messages","workspaceId":ws_id,"messages":msgs}),
            );
        }
        Err(e) => {
            send_json(
                sender,
                &serde_json::json!({"type":"error","message":e.to_string()}),
            );
        }
    }
}
