//! Event injection methods (called externally by daemon wiring).

use super::gateway::WebSocketGateway;
use super::wire::to_group_info;
use crate::types::GroupBinding;

impl WebSocketGateway {
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

    pub async fn notify_agent_usage(
        &self,
        agent_jid: &str,
        usage: &crate::zen_core::ConversationUsageData,
    ) {
        let payload = serde_json::json!({
            "type": "agent:usage",
            "agentJid": agent_jid,
            "usage": {
                "useTokens": usage.usage.use_tokens,
                "maxTokens": usage.usage.max_tokens,
                "promptTokens": usage.usage.prompt_tokens,
            },
        });
        self.broadcast_to_all(&payload).await;
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
        sender: &tokio::sync::mpsc::UnboundedSender<axum::extract::ws::Message>,
        jid: &str,
    ) {
        let states = self.last_known_states.lock().await;
        if let Some(state) = states.get(jid) {
            let msg = serde_json::json!({
                "type": "agent:state",
                "groupJid": jid,
                "state": state,
            });
            let _ = sender.send(axum::extract::ws::Message::Text(msg.to_string().into()));
        }
    }
}
