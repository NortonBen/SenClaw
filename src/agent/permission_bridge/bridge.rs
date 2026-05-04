//! PermissionBridge — relay sema-core permission requests to inline keyboards / Web UI.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::types::InlineButton;

use super::api::{PermissionBridgeApi, PREFIX_ASK, PREFIX_PERM};
use super::types::{
    AskQuestionData, AskQuestionPayload, PendingAskQuestion, PendingPermission, PermissionOption,
    PermissionPayload,
};
use super::utils::{capitalize_first, format_content, short_id, truncate_content};

pub struct PermissionBridge {
    pub(crate) pending_permissions: Mutex<HashMap<String, PendingPermission>>,
    pub(crate) pending_ask_questions: Mutex<HashMap<String, PendingAskQuestion>>,
    max_content_length: usize,
    api: Arc<dyn PermissionBridgeApi>,

    // Callback setters (set once during daemon wiring)
    pub(crate) on_activity: Mutex<Option<Box<dyn Fn(&str) + Send + Sync>>>,
    pub(crate) on_permission_request:
        Mutex<Option<Box<dyn Fn(&str, &str, PermissionPayload) + Send + Sync>>>,
    pub(crate) on_ask_question_request:
        Mutex<Option<Box<dyn Fn(&str, &str, AskQuestionPayload) + Send + Sync>>>,
    pub(crate) on_permission_resolved:
        Mutex<Option<Box<dyn Fn(&str, &str, &str, &str) + Send + Sync>>>,
    pub(crate) on_ask_question_resolved:
        Mutex<Option<Box<dyn Fn(&str, &str, HashMap<String, String>) + Send + Sync>>>,
}

impl PermissionBridge {
    pub fn new(api: Arc<dyn PermissionBridgeApi>, max_content_length: Option<usize>) -> Self {
        Self {
            pending_permissions: Mutex::new(HashMap::new()),
            pending_ask_questions: Mutex::new(HashMap::new()),
            max_content_length: max_content_length.unwrap_or(200),
            api,
            on_activity: Mutex::new(None),
            on_permission_request: Mutex::new(None),
            on_ask_question_request: Mutex::new(None),
            on_permission_resolved: Mutex::new(None),
            on_ask_question_resolved: Mutex::new(None),
        }
    }

    // ===== Callback setters =====

    /// Inject activity callback (used by AgentPool to reset timeout timer).
    pub fn set_activity_callback<F: Fn(&str) + Send + Sync + 'static>(&self, cb: F) {
        *self.on_activity.lock().unwrap() = Some(Box::new(cb));
    }

    /// Inject permission-request notifier (WS notification to Web UI).
    pub fn set_permission_request_callback<
        F: Fn(&str, &str, PermissionPayload) + Send + Sync + 'static,
    >(
        &self,
        cb: F,
    ) {
        *self.on_permission_request.lock().unwrap() = Some(Box::new(cb));
    }

    /// Inject ask-question-request notifier (WS notification to Web UI).
    pub fn set_ask_question_request_callback<
        F: Fn(&str, &str, AskQuestionPayload) + Send + Sync + 'static,
    >(
        &self,
        cb: F,
    ) {
        *self.on_ask_question_request.lock().unwrap() = Some(Box::new(cb));
    }

    /// Inject permission-resolution notifier (broadcast to other endpoints).
    pub fn set_permission_resolved_callback<
        F: Fn(&str, &str, &str, &str) + Send + Sync + 'static,
    >(
        &self,
        cb: F,
    ) {
        *self.on_permission_resolved.lock().unwrap() = Some(Box::new(cb));
    }

    /// Inject ask-question-resolution notifier (broadcast to other endpoints).
    pub fn set_ask_question_resolved_callback<
        F: Fn(&str, &str, HashMap<String, String>) + Send + Sync + 'static,
    >(
        &self,
        cb: F,
    ) {
        *self.on_ask_question_resolved.lock().unwrap() = Some(Box::new(cb));
    }

    // ===== Public resolution API (called from Web UI via WebSocket gateway) =====

    /// Resolve a pending permission request. First responder wins.
    /// Returns `false` if the request was already consumed.
    pub fn resolve_permission(&self, request_id: &str, option_key: &str) -> bool {
        let pending = {
            let mut map = self.pending_permissions.lock().unwrap();
            map.remove(request_id)
        };
        let Some(pending) = pending else {
            return false;
        };

        self.fire_activity(&pending.chat_jid);
        self.api
            .respond_to_tool_permission(&pending.group_jid, &pending.tool_name, option_key);

        let label = capitalize_first(option_key);
        if let Some(cb) = self.on_permission_resolved.lock().unwrap().as_ref() {
            cb(&pending.chat_jid, request_id, option_key, &label);
        }
        true
    }

    /// Batch-answer a pending ask-question request (Web UI path).
    ///
    /// `answers`: `{ qi: oi }` single-select or `{ qi: [oi, ...] }` multi-select.
    /// `-1` = "Other" option.
    /// `other_texts`: `{ qi: "custom text" }` for the Other option.
    ///
    /// Returns `false` if the request was already consumed.
    pub fn resolve_ask_question_batch(
        &self,
        request_id: &str,
        answers: &serde_json::Value,
        other_texts: Option<&serde_json::Value>,
    ) -> bool {
        const OTHER_INDEX: i64 = -1;

        let pending = {
            let mut map = self.pending_ask_questions.lock().unwrap();
            map.remove(request_id)
        };
        let Some(pending) = pending else {
            return false;
        };

        let mut resolved: HashMap<String, String> = HashMap::new();

        if let Some(obj) = answers.as_object() {
            for (qi_str, selection) in obj {
                let qi: usize = match qi_str.parse() {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let question = match pending.questions.get(qi) {
                    Some(q) => q,
                    None => continue,
                };

                let resolve_option = |oi: i64| -> String {
                    if oi == OTHER_INDEX {
                        return other_texts
                            .and_then(|ot| ot.get(qi_str))
                            .and_then(|v| v.as_str())
                            .unwrap_or("Other")
                            .to_string();
                    }
                    question
                        .options
                        .get(oi as usize)
                        .map(|o| o.label.clone())
                        .unwrap_or_default()
                };

                let label = if let Some(arr) = selection.as_array() {
                    let labels: Vec<String> = arr
                        .iter()
                        .filter_map(|v| v.as_i64().map(resolve_option))
                        .filter(|s| !s.is_empty())
                        .collect();
                    labels.join(",")
                } else if let Some(n) = selection.as_i64() {
                    resolve_option(n)
                } else {
                    continue;
                };

                resolved.insert(question.question.clone(), label);
            }
        }

        self.fire_activity(&pending.chat_jid);
        self.api
            .respond_to_ask_question(&pending.group_jid, &pending.agent_id, resolved.clone());

        if let Some(cb) = self.on_ask_question_resolved.lock().unwrap().as_ref() {
            cb(&pending.chat_jid, request_id, resolved);
        }
        true
    }

    // ===== Incoming request handlers (called when sema-core fires events) =====

    /// Handle a `tool:permission:request` event from sema-core.
    /// Sends inline keyboard to channel (if supported) and notifies Web UI via callback.
    pub fn handle_permission_request(
        &self,
        tool_name: &str,
        title: &str,
        content: &serde_json::Value,
        options: &HashMap<String, String>,
        group_jid: &str,
        chat_jid: &str,
        bot_token: Option<&str>,
    ) {
        let request_id = short_id();
        {
            let mut map = self.pending_permissions.lock().unwrap();
            map.insert(
                request_id.clone(),
                PendingPermission {
                    tool_name: tool_name.to_string(),
                    chat_jid: chat_jid.to_string(),
                    group_jid: group_jid.to_string(),
                },
            );
        }

        let raw_content = format_content(content);
        let content_str = truncate_content(&raw_content, self.max_content_length);

        let text =
            format!("🔐 *Permission Request*\n\nTool: {tool_name}\n{title}\n\n{content_str}");

        let buttons: Vec<InlineButton> = options
            .iter()
            .map(|(key, label)| InlineButton {
                label: label.clone(),
                callback_data: format!("{PREFIX_PERM}:{request_id}:{key}"),
            })
            .collect();

        let is_web = self.api.is_web_jid(chat_jid);

        // Try channel send with buttons
        if !is_web {
            if self.api.supports_buttons(chat_jid) {
                if let Err(e) = self
                    .api
                    .send_with_buttons(chat_jid, &text, &buttons, bot_token)
                {
                    tracing::warn!(
                        "[PermissionBridge] send_with_buttons failed for {chat_jid}: {e}"
                    );
                }
            } else if self.on_permission_request.lock().unwrap().is_none() {
                // No WS sink → downgrade to plain text + auto-deny
                let option_lines: String = buttons
                    .iter()
                    .map(|b| format!("• {}", b.label))
                    .collect::<Vec<_>>()
                    .join("\n");
                let _ = self.api.send_message(
                    chat_jid,
                    &format!(
                        "{text}\n\nOptions:\n{option_lines}\n\n\
                         (This channel does not support interactive buttons. \
                         Please contact the administrator for configuration)"
                    ),
                    bot_token,
                );
                self.api
                    .respond_to_tool_permission(group_jid, tool_name, "refuse");
                self.pending_permissions.lock().unwrap().remove(&request_id);
                return;
            }
        }

        // Notify Web UI
        if let Some(cb) = self.on_permission_request.lock().unwrap().as_ref() {
            cb(
                chat_jid,
                &request_id,
                PermissionPayload {
                    tool_name: tool_name.to_string(),
                    title: title.to_string(),
                    content: raw_content,
                    options: options
                        .iter()
                        .map(|(key, label)| PermissionOption {
                            key: key.clone(),
                            label: label.clone(),
                        })
                        .collect(),
                },
            );
        }

        self.fire_activity(chat_jid);
    }

    /// Handle an `ask:question:request` event from sema-core.
    /// Sends each question as an inline-keyboard message and notifies Web UI.
    pub fn handle_ask_question_request(
        &self,
        agent_id: &str,
        questions: Vec<AskQuestionData>,
        group_jid: &str,
        chat_jid: &str,
        bot_token: Option<&str>,
    ) {
        let request_id = short_id();
        let pending_count = questions.len();
        {
            let mut map = self.pending_ask_questions.lock().unwrap();
            map.insert(
                request_id.clone(),
                PendingAskQuestion {
                    agent_id: agent_id.to_string(),
                    chat_jid: chat_jid.to_string(),
                    group_jid: group_jid.to_string(),
                    questions: questions.clone(),
                    answers: HashMap::new(),
                    pending_count,
                },
            );
        }

        let is_web = self.api.is_web_jid(chat_jid);

        // Send each question to the channel if buttons supported
        if !is_web && self.api.supports_buttons(chat_jid) {
            for (qi, q) in questions.iter().enumerate() {
                let text = format!("❓ *{}*\n\n{}", q.header, q.question);
                let buttons: Vec<InlineButton> = q
                    .options
                    .iter()
                    .enumerate()
                    .map(|(oi, opt)| InlineButton {
                        label: opt.label.clone(),
                        callback_data: format!("{PREFIX_ASK}:{request_id}:{qi}:{oi}"),
                    })
                    .collect();
                if let Err(e) = self
                    .api
                    .send_with_buttons(chat_jid, &text, &buttons, bot_token)
                {
                    tracing::warn!(
                        "[PermissionBridge] ask-question send_with_buttons failed for {chat_jid}: {e}"
                    );
                }
            }
        }

        // Notify Web UI
        if let Some(cb) = self.on_ask_question_request.lock().unwrap().as_ref() {
            cb(
                chat_jid,
                &request_id,
                AskQuestionPayload {
                    agent_id: agent_id.to_string(),
                    questions,
                },
            );
        }

        self.fire_activity(chat_jid);
    }

    // ===== Callback routing (called when a channel receives an inline-button press) =====

    /// Route an inline-keyboard callback press. Returns confirmation text to show
    /// the user (e.g. via Telegram `answerCallbackQuery`), or `None` if unknown.
    pub fn handle_callback(&self, callback_data: &str, _chat_jid: &str) -> Option<String> {
        if let Some(rest) = callback_data.strip_prefix(&format!("{PREFIX_PERM}:")) {
            return self.handle_permission_callback(rest);
        }
        if let Some(rest) = callback_data.strip_prefix(&format!("{PREFIX_ASK}:")) {
            return self.handle_ask_question_callback(rest);
        }
        None
    }

    // ===== Internal =====

    fn fire_activity(&self, chat_jid: &str) {
        if let Some(cb) = self.on_activity.lock().unwrap().as_ref() {
            cb(chat_jid);
        }
    }

    /// Parse `P:{requestId}:{optionKey}` (prefix already stripped → `{requestId}:{optionKey}`).
    fn handle_permission_callback(&self, rest: &str) -> Option<String> {
        let colon = rest.find(':')?;
        let request_id = &rest[..colon];
        let option_key = &rest[colon + 1..];

        let pending = self
            .pending_permissions
            .lock()
            .unwrap()
            .remove(request_id)?;

        self.fire_activity(&pending.chat_jid);

        self.api
            .respond_to_tool_permission(&pending.group_jid, &pending.tool_name, option_key);

        let label = capitalize_first(option_key);
        if let Some(cb) = self.on_permission_resolved.lock().unwrap().as_ref() {
            cb(&pending.chat_jid, request_id, option_key, &label);
        }

        Some(format!("✅ Selected: {label}"))
    }

    /// Parse `Q:{requestId}:{qi}:{oi}` (prefix already stripped → `{requestId}:{qi}:{oi}`).
    fn handle_ask_question_callback(&self, rest: &str) -> Option<String> {
        let mut parts = rest.splitn(3, ':');
        let request_id = parts.next()?;
        let qi: usize = parts.next()?.parse().ok()?;
        let oi: usize = parts.next()?.parse().ok()?;

        let question_label = {
            let map = self.pending_ask_questions.lock().unwrap();
            let pending = map.get(request_id)?;
            let question = pending.questions.get(qi)?;
            let option = question.options.get(oi)?;
            option.label.clone()
        };

        // Re-acquire for mutation
        let (should_resolve, resolved_answers, chat_jid, group_jid, agent_id) = {
            let mut map = self.pending_ask_questions.lock().unwrap();
            let pending = match map.get_mut(request_id) {
                Some(p) => p,
                None => return Some(format!("✅ Selected: {question_label}")),
            };

            let question = match pending.questions.get(qi) {
                Some(q) => q,
                None => return Some(format!("✅ Selected: {question_label}")),
            };

            pending
                .answers
                .insert(question.question.clone(), question_label.clone());
            pending.pending_count = pending.pending_count.saturating_sub(1);

            self.fire_activity(&pending.chat_jid);

            if pending.pending_count == 0 {
                let answers = pending.answers.clone();
                let chat_jid = pending.chat_jid.clone();
                let group_jid = pending.group_jid.clone();
                let agent_id = pending.agent_id.clone();
                map.remove(request_id);
                (true, answers, chat_jid, group_jid, agent_id)
            } else {
                return Some(format!("✅ Selected: {question_label}"));
            }
        };

        if should_resolve {
            self.api
                .respond_to_ask_question(&group_jid, &agent_id, resolved_answers.clone());
            if let Some(cb) = self.on_ask_question_resolved.lock().unwrap().as_ref() {
                cb(&chat_jid, request_id, resolved_answers);
            }
        }

        Some(format!("✅ Selected: {question_label}"))
    }
}
