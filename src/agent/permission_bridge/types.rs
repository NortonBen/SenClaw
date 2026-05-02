//! Public payload types and internal pending state for permission bridge.

use std::collections::HashMap;

use serde::Serialize;

// ===== Public payload types (mirrors TS PermissionPayload / AskQuestionPayload) =====

#[derive(Debug, Clone, Serialize)]
pub struct PermissionOption {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionPayload {
    #[serde(rename = "toolName")]
    pub tool_name: String,
    pub title: String,
    /// Full untruncated content; frontend decides whether to collapse.
    pub content: String,
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AskQuestionOption {
    pub label: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AskQuestionData {
    pub header: String,
    pub question: String,
    pub options: Vec<AskQuestionOption>,
    #[serde(rename = "multiSelect")]
    pub multi_select: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AskQuestionPayload {
    #[serde(rename = "agentId")]
    pub agent_id: String,
    pub questions: Vec<AskQuestionData>,
}

// ===== Internal pending state =====

pub(crate) struct PendingPermission {
    pub tool_name: String,
    pub chat_jid: String,
    /// Identifies which group/core to respond to (typically the group JID).
    pub group_jid: String,
}

pub(crate) struct PendingAskQuestion {
    pub agent_id: String,
    pub chat_jid: String,
    pub group_jid: String,
    pub questions: Vec<AskQuestionData>,
    /// Accumulated answers keyed by question text (Telegram step-by-step path).
    pub answers: HashMap<String, String>,
    /// Remaining unanswered count; triggers respond when zero.
    pub pending_count: usize,
}
