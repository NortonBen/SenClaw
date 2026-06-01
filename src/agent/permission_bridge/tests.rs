use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::api::PermissionBridgeApi;
use super::bridge::PermissionBridge;
use super::utils::{capitalize_first, format_content, short_id, truncate_content};

struct StubApi;
impl PermissionBridgeApi for StubApi {}

fn stub_api() -> Arc<dyn PermissionBridgeApi> {
    Arc::new(StubApi)
}

#[derive(Default)]
struct RecordingApi {
    responses: Mutex<Vec<(String, String, String)>>,
}

impl PermissionBridgeApi for RecordingApi {
    fn respond_to_tool_permission(&self, group_jid: &str, tool_name: &str, selected: &str) {
        self.responses.lock().unwrap().push((
            group_jid.to_string(),
            tool_name.to_string(),
            selected.to_string(),
        ));
    }
}

#[test]
fn test_short_id_is_8_hex_chars() {
    let id = short_id();
    assert_eq!(id.len(), 8);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_capitalize_first() {
    assert_eq!(capitalize_first("allow"), "Allow");
    assert_eq!(capitalize_first(""), "");
    assert_eq!(capitalize_first("a"), "A");
    assert_eq!(capitalize_first("ABC"), "ABC");
}

#[test]
fn test_format_content_string() {
    let v = serde_json::json!("hello world");
    assert_eq!(format_content(&v), "hello world");
}

#[test]
fn test_format_content_diff_patch() {
    let v = serde_json::json!({
        "patch": [
            {"lines": ["+added line", "-removed line"]},
            {"lines": [" context line"]}
        ]
    });
    assert_eq!(
        format_content(&v),
        "+added line\n-removed line\n context line"
    );
}

#[test]
fn test_format_content_fallback_json() {
    let v = serde_json::json!({"key": "value", "nested": {"a": 1}});
    let result = format_content(&v);
    assert!(result.contains("\"key\""));
    assert!(result.contains("\"value\""));
}

#[test]
fn test_truncate_content_no_truncation() {
    let s = "short message";
    assert_eq!(truncate_content(s, 200), s);
}

#[test]
fn test_truncate_content_utf8_no_panic_mid_char() {
    // 198 ASCII + "ị" (3 UTF-8 bytes) — raw byte 200 lies inside "ị" without boundary fix
    let s = format!("{}ị", "a".repeat(198));
    assert_eq!(s.len(), 201);
    let result = truncate_content(&s, 200);
    assert!(result.starts_with(&"a".repeat(198)));
    assert!(result.contains("chars omitted"));
}

#[test]
fn test_truncate_content_with_overflow() {
    let s = "x".repeat(250);
    let result = truncate_content(&s, 200);
    assert!(result.starts_with(&"x".repeat(200)));
    assert!(result.contains("50 chars omitted"));
}

#[test]
fn test_resolve_permission_not_found() {
    let bridge = PermissionBridge::new(stub_api(), None);
    assert!(!bridge.resolve_permission("nonexistent", "allow"));
}

#[test]
fn test_resolve_permission_first_responder_wins() {
    let bridge = PermissionBridge::new(stub_api(), None);

    // Set a permission-request callback to prevent auto-deny path
    let captured_id = Arc::new(Mutex::new(String::new()));
    {
        let captured_id = Arc::clone(&captured_id);
        bridge.set_permission_request_callback(move |_chat_jid, request_id, _payload| {
            *captured_id.lock().unwrap() = request_id.to_string();
        });
    }

    let options: HashMap<String, String> = [
        ("allow".into(), "Allow".into()),
        ("refuse".into(), "Refuse".into()),
    ]
    .into();
    bridge.handle_permission_request(
        "Bash",
        "Run command?",
        &serde_json::json!("rm -rf /"),
        &options,
        "group-1",
        "chat-1",
        None,
    );

    let request_id = captured_id.lock().unwrap().clone();
    assert!(!request_id.is_empty(), "request ID should be captured");

    // First resolution should succeed
    assert!(bridge.resolve_permission(&request_id, "allow"));

    // Second resolution on same ID should fail (already consumed)
    assert!(!bridge.resolve_permission(&request_id, "refuse"));
}

#[test]
fn test_default_rules_auto_accept_skill_and_task() {
    let api = Arc::new(RecordingApi::default());
    let bridge = PermissionBridge::new(api.clone(), None);
    let options: HashMap<String, String> = [
        ("allow".into(), "Allow".into()),
        ("refuse".into(), "Refuse".into()),
    ]
    .into();

    bridge.handle_permission_request(
        "Skill",
        "Load skill?",
        &serde_json::json!({"skill": "agent-browser"}),
        &options,
        "group-1",
        "chat-1",
        None,
    );
    bridge.handle_permission_request(
        "Task",
        "Launch agent?",
        &serde_json::json!({"subagent_type": "general-purpose"}),
        &options,
        "group-1",
        "chat-1",
        None,
    );

    let responses = api.responses.lock().unwrap().clone();
    assert_eq!(
        responses,
        vec![
            ("group-1".into(), "Skill".into(), "allow".into()),
            ("group-1".into(), "Task".into(), "allow".into()),
        ]
    );
}

#[test]
fn test_handle_callback_unknown_prefix() {
    let bridge = PermissionBridge::new(stub_api(), None);
    assert_eq!(bridge.handle_callback("X:123:allow", "chat-1"), None);
}

#[test]
fn test_resolve_ask_question_batch_not_found() {
    let bridge = PermissionBridge::new(stub_api(), None);
    assert!(!bridge.resolve_ask_question_batch("nonexistent", &serde_json::json!({"0": 0}), None));
}
