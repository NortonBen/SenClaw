// ===== Individual message handlers =====

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::ws::Message;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::gateway::command_dispatcher::dispatch_command;
use crate::gateway::group_manager::{
    delete_feishu_app, delete_qq_app, delete_telegram_bot, get_feishu_apps, save_feishu_app,
    save_qq_app, save_telegram_bot, GroupBindingUpdate,
};
use crate::types::{GroupBinding, TaskStatus};
use crate::util::local_time::local_iso_string_now;

use super::helpers::{broadcast_to_all_inner, now_iso, require_admin, require_auth, send_json};
use super::state::{WsClient, WsState};
use super::wire::{to_group_info, GroupInfo};

pub(crate) async fn handle_connect(
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

pub(crate) async fn handle_subscribe(
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
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "groupJid required"}),
        );
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
        guard.get(client_idx).map(|c| c.is_admin).unwrap_or(false)
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
                let agent_name = entry
                    .get("agentName")
                    .and_then(|v| v.as_str())
                    .unwrap_or(agent_jid);
                let todos_arr = entry
                    .get("todos")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
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
                let agent_name = entry
                    .get("agentName")
                    .and_then(|v| v.as_str())
                    .unwrap_or(agent_jid);
                let tools_arr = entry
                    .get("tools")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
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
    if let Ok(messages) = state.db.get_group_messages(&jid, None) {
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

pub(crate) async fn handle_unsubscribe(
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

pub(crate) async fn handle_list_groups(
    clients: &Arc<Mutex<Vec<WsClient>>>,
    client_idx: usize,
    sender: &tokio::sync::mpsc::UnboundedSender<Message>,
    state: &Arc<WsState>,
) {
    if !require_auth(clients, client_idx, sender).await {
        return;
    }
    // Legacy groups table
    let mut groups: Vec<GroupInfo> = state
        .group_manager
        .list(&state.db)
        .unwrap_or_default()
        .iter()
        .map(to_group_info)
        .collect();

    // Entity-model bindings (Telegram, Feishu, etc. created via Settings UI)
    // Only include bindings that have a real JID (not pending).
    let bindings = state
        .binding_manager
        .list_with_relations(&state.db)
        .unwrap_or_default();
    for br in &bindings {
        if let Some(jid) = &br.binding.jid {
            // Skip if already present from legacy groups table.
            if groups.iter().any(|g| &g.jid == jid) {
                continue;
            }
            groups.push(GroupInfo {
                jid: jid.clone(),
                folder: br.agent.folder.clone(),
                name: format!("{} ({})", br.channel.name, br.agent.name),
                is_admin: br.binding.is_admin,
                channel: br.channel.platform_type.clone(),
                group_type: "chat".to_string(),
                requires_trigger: br.agent.requires_trigger,
                allowed_tools: br.agent.allowed_tools.clone(),
                allowed_paths: br.agent.allowed_paths.clone(),
                allowed_work_dirs: br.agent.allowed_work_dirs.clone(),
                max_messages: br.binding.max_messages,
                agent_id: Some(br.agent.id),
                channel_id: Some(br.channel.id),
            });
        }
    }

    send_json(
        sender,
        &serde_json::json!({"type": "groups", "groups": groups}),
    );
}

pub(crate) async fn handle_register_group(
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
                    Some((
                        c.get(1)?.as_str().to_string(),
                        c.get(2)?.as_str().to_string(),
                    ))
                })
            {
                resolved_jid = format!("tg:{bot_user_id}:{}:{}", caps.0, caps.1);
            }
            // Persist to config.json.
            if let Some(chat_caps) = regex::Regex::new(r"^tg:(?:\d+:)?user:(\d+)$")
                .ok()
                .and_then(|re| {
                    re.captures(&resolved_jid)
                        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
                })
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
            existing
                .as_ref()
                .map(|e| e.channel.clone())
                .unwrap_or_default()
        } else {
            resolved_channel.clone()
        },
        group_type: msg["groupType"].as_str().unwrap_or("chat").to_string(),
        is_admin: false,
        requires_trigger: msg["requiresTrigger"].as_bool().unwrap_or(
            existing
                .as_ref()
                .map(|e| e.requires_trigger)
                .unwrap_or(true),
        ),
        allowed_tools: if msg.get("allowedTools").is_some() {
            msg["allowedTools"].as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
        } else {
            existing.as_ref().and_then(|e| e.allowed_tools.clone())
        },
        allowed_paths: None,
        allowed_work_dirs: if msg.get("allowedWorkDirs").is_some() {
            msg["allowedWorkDirs"].as_array().map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
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
    broadcast_to_all_inner(
        clients,
        &serde_json::json!({"type": "group:registered", "group": info}),
    )
    .await;
}

pub(crate) async fn handle_unregister_group(
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
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "jid required"}),
        );
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
        let still_used = all.iter().any(|g| {
            g.jid != jid && g.channel == "feishu" && g.bot_token.as_deref() == Some(app_id)
        });
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

pub(crate) async fn handle_update_group(
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
        send_json(
            sender,
            &serde_json::json!({"type": "error", "message": "jid required"}),
        );
        return;
    }

    let mut updates = GroupBindingUpdate::default();
    if let Some(v) = msg["name"].as_str() {
        updates.name = Some(v.to_string());
    }
    if let Some(v) = msg["channel"].as_str() {
        updates.channel = Some(v.to_string());
    }
    if let Some(v) = msg["groupType"].as_str() {
        updates.group_type = Some(v.to_string());
    }
    if let Some(v) = msg["requiresTrigger"].as_bool() {
        updates.requires_trigger = Some(v);
    }
    if let Some(v) = msg["isAdmin"].as_bool() {
        updates.is_admin = Some(v);
    }
    if msg.get("allowedTools").is_some() {
        updates.allowed_tools = Some(msg["allowedTools"].as_array().map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        }));
    }
    if msg.get("allowedPaths").is_some() {
        updates.allowed_paths = Some(msg["allowedPaths"].as_array().map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        }));
    }
    if msg.get("allowedWorkDirs").is_some() {
        updates.allowed_work_dirs = Some(msg["allowedWorkDirs"].as_array().map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        }));
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

pub(crate) async fn handle_message_send(
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
        guard.get(client_idx).map(|c| c.is_admin).unwrap_or(false)
    };
    if is_admin {
        let is_reset = text.trim() == "/reset"
            || text.trim() == "reset"
            || text.trim().starts_with("/reset ")
            || text.trim().starts_with("reset ");
        if let Some(output) = dispatch_command(&state.db, &text, Some(&group_jid)) {
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
    let limit = state.config.agent.max_messages_per_group;
    if let Err(e) = state.db.insert_group_message(&stored, limit) {
        tracing::warn!("[WebSocketGateway] Failed to persist web message for {group_jid}: {e}");
    }

    state.api.enqueue_and_process(&group_jid, &group, &text);
}

pub(crate) async fn handle_permission_response(
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

pub(crate) async fn handle_question_response(
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

pub(crate) async fn handle_list_tasks(
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
            state
                .db
                .get_tasks_by_group(&group.folder)
                .unwrap_or_default()
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

pub(crate) async fn handle_task_logs(
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
    let logs = state
        .db
        .get_task_run_logs(&task_id, limit)
        .unwrap_or_default();
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

pub(crate) async fn handle_manage_task(
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

pub(crate) async fn handle_register_feishu_app(
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

pub(crate) async fn handle_unregister_feishu_app(
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

pub(crate) async fn handle_register_qq_app(
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

pub(crate) async fn handle_unregister_qq_app(
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

pub(crate) async fn handle_list_feishu_apps(
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
    send_json(
        sender,
        &serde_json::json!({"type": "feishu-apps", "apps": list}),
    );
}

pub(crate) async fn handle_list_dispatch(
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
    tracing::info!("[WsGateway] list:dispatch client #{client_idx}: {parent_count} parent(s)");
    send_json(
        sender,
        &serde_json::json!({"type": "dispatch:update", "parents": parents}),
    );
}

pub(crate) async fn handle_agent_control(
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
