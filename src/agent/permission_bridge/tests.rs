use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::api::PermissionBridgeApi;
use super::bridge::PermissionBridge;
use super::types::{RuleAction, RuleMatcher, RuleMatcherType, ToolAutoAcceptRule};
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
fn test_default_rules_do_not_auto_accept_skill_or_task() {
    let api = Arc::new(RecordingApi::default());
    let bridge = PermissionBridge::new(api.clone(), None);
    bridge.set_permission_request_callback(|_, _, _| {});
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
    assert!(responses.is_empty());
}

#[test]
fn test_skill_exact_rule_auto_accepts_only_selected_skill() {
    let api = Arc::new(RecordingApi::default());
    let bridge = PermissionBridge::new(api.clone(), None);
    bridge.set_permission_request_callback(|_, _, _| {});
    bridge.add_rule(ToolAutoAcceptRule {
        id: "skill-auto-access:agent-browser".into(),
        matcher: RuleMatcher {
            matcher_type: RuleMatcherType::SkillExact,
            pattern: None,
            tool_name: None,
            skill_name: Some("agent-browser".into()),
            server: None,
            tool: None,
            category: None,
        },
        action: RuleAction::AutoAccept,
        enabled: true,
        description: None,
    });
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
        "Skill",
        "Load skill?",
        &serde_json::json!({"skill": "web-research"}),
        &options,
        "group-1",
        "chat-1",
        None,
    );

    let responses = api.responses.lock().unwrap().clone();
    assert_eq!(
        responses,
        vec![("group-1".into(), "Skill".into(), "allow".into())]
    );
}

#[test]
fn test_mcp_server_rule_auto_accepts_hyphenated_server() {
    // Regression: app-space MCP servers keep hyphens in the tool name
    // (e.g. "mcp__ssh-manager-mcp__ssh_list_hosts"). An "Auto Access ALL"
    // rule stores server "ssh-manager-mcp"; the matcher must accept the
    // hyphenated tool name, not only the underscore-normalized form.
    let api = Arc::new(RecordingApi::default());
    let bridge = PermissionBridge::new(api.clone(), None);
    bridge.set_permission_request_callback(|_, _, _| {});
    bridge.add_rule(ToolAutoAcceptRule {
        id: "mcp:ssh-manager-mcp:*".into(),
        matcher: RuleMatcher {
            matcher_type: RuleMatcherType::McpServer,
            pattern: None,
            tool_name: None,
            skill_name: None,
            server: Some("ssh-manager-mcp".into()),
            tool: None,
            category: None,
        },
        action: RuleAction::AutoAccept,
        enabled: true,
        description: None,
    });
    let options: HashMap<String, String> = [
        ("allow".into(), "Allow".into()),
        ("refuse".into(), "Refuse".into()),
    ]
    .into();

    bridge.handle_permission_request(
        "mcp__ssh-manager-mcp__ssh_list_hosts",
        "Run tool?",
        &serde_json::json!(null),
        &options,
        "group-1",
        "chat-1",
        None,
    );
    // A tool from a different server must NOT auto-accept.
    bridge.handle_permission_request(
        "mcp__other-server__do_thing",
        "Run tool?",
        &serde_json::json!(null),
        &options,
        "group-1",
        "chat-1",
        None,
    );

    let responses = api.responses.lock().unwrap().clone();
    assert_eq!(
        responses,
        vec![(
            "group-1".into(),
            "mcp__ssh-manager-mcp__ssh_list_hosts".into(),
            "allow".into()
        )]
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

// ===== FormUI bridge tests =====

fn sample_form_request(agent_id: &str) -> crate::zen_core::FormRequestData {
    let fields: Vec<crate::zen_core::FormField> = serde_json::from_value(serde_json::json!([
        {"type": "static_text", "text": "Header", "variant": "heading"},
        {"type": "text", "key": "env", "label": "Environment", "required": true, "default": "staging"},
        {"type": "checkbox", "key": "dry_run", "label": "Dry run", "default": true}
    ]))
    .unwrap();
    crate::zen_core::FormRequestData {
        agent_id: agent_id.to_string(),
        title: "Deploy".to_string(),
        surface: "inline".to_string(),
        submit_label: "Submit".to_string(),
        fields,
    }
}

/// Records `respond_to_form` deliveries plus plain messages (snapshot path).
#[derive(Default)]
struct FormRecordingApi {
    forms: Mutex<Vec<(String, String, HashMap<String, serde_json::Value>, bool)>>,
    sent: Mutex<Vec<String>>,
    web: bool,
}

impl PermissionBridgeApi for FormRecordingApi {
    fn is_web_jid(&self, _chat_jid: &str) -> bool {
        self.web
    }
    fn send_message(
        &self,
        _chat_jid: &str,
        text: &str,
        _bot_token: Option<&str>,
    ) -> anyhow::Result<()> {
        self.sent.lock().unwrap().push(text.to_string());
        Ok(())
    }
    fn respond_to_form(
        &self,
        group_jid: &str,
        agent_id: &str,
        values: HashMap<String, serde_json::Value>,
        submitted: bool,
    ) {
        self.forms.lock().unwrap().push((
            group_jid.to_string(),
            agent_id.to_string(),
            values,
            submitted,
        ));
    }
}

#[test]
fn test_resolve_form_not_found() {
    let bridge = PermissionBridge::new(stub_api(), None);
    assert!(!bridge.resolve_form("nonexistent", HashMap::new(), true));
}

#[test]
fn test_form_request_round_trip_via_web() {
    let api = Arc::new(FormRecordingApi {
        web: true,
        ..Default::default()
    });
    let bridge = PermissionBridge::new(api.clone(), None);

    // Capture the WS notification (requestId + payload).
    let captured = Arc::new(Mutex::new(None::<(String, String)>));
    {
        let captured = Arc::clone(&captured);
        bridge.set_form_request_callback(move |chat_jid, request_id, payload| {
            *captured.lock().unwrap() = Some((request_id.to_string(), payload.title.clone()));
            assert_eq!(chat_jid, "web:chat-1");
            assert_eq!(payload.fields.len(), 3);
        });
    }
    let resolved_cb = Arc::new(Mutex::new(None::<String>));
    {
        let resolved_cb = Arc::clone(&resolved_cb);
        bridge.set_form_resolved_callback(move |_chat_jid, request_id, values| {
            assert_eq!(values["env"], serde_json::json!("prod"));
            *resolved_cb.lock().unwrap() = Some(request_id.to_string());
        });
    }

    bridge.handle_form_request(&sample_form_request("main"), "group-1", "web:chat-1", None);
    let (request_id, title) = captured.lock().unwrap().clone().expect("WS notified");
    assert_eq!(title, "Deploy");

    // First responder wins; second submit is a no-op.
    let mut values = HashMap::new();
    values.insert("env".to_string(), serde_json::json!("prod"));
    assert!(bridge.resolve_form(&request_id, values.clone(), true));
    assert!(!bridge.resolve_form(&request_id, values, true));

    let forms = api.forms.lock().unwrap().clone();
    assert_eq!(forms.len(), 1);
    let (group_jid, agent_id, delivered, submitted) = &forms[0];
    assert_eq!(group_jid, "group-1");
    assert_eq!(agent_id, "main");
    assert!(submitted);
    assert_eq!(delivered["env"], serde_json::json!("prod"));
    assert_eq!(resolved_cb.lock().unwrap().as_deref(), Some(request_id.as_str()));
}

#[test]
fn test_form_request_degraded_channel_auto_submits_defaults() {
    // Non-web jid + no WS sink → snapshot text + auto-submit defaults with
    // submitted=false so the agent is never blocked forever.
    let api = Arc::new(FormRecordingApi::default());
    let bridge = PermissionBridge::new(api.clone(), None);

    bridge.handle_form_request(&sample_form_request("main"), "group-1", "tg:123", None);

    let sent = api.sent.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains("Deploy"));
    assert!(sent[0].contains("Environment"));

    let forms = api.forms.lock().unwrap().clone();
    assert_eq!(forms.len(), 1);
    let (_, _, values, submitted) = &forms[0];
    assert!(!submitted);
    assert_eq!(values["env"], serde_json::json!("staging"));
    assert_eq!(values["dry_run"], serde_json::json!(true));
    // static_text contributes no value
    assert_eq!(values.len(), 2);

    // Pending entry must be consumed — resolving later returns false.
    assert!(!bridge.resolve_form("anything", HashMap::new(), true));
}
