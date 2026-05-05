use std::collections::HashSet;
use std::sync::Arc;

use super::engine::ZenCoreApi;
use super::pool::AgentPool;
use super::traits::CoreApi;
use super::types::PermissionsConfig;
use super::workspace::WorkspaceStateFile;
use crate::types::GroupBinding;

fn fake_binding(jid: &str, is_admin: bool) -> GroupBinding {
    GroupBinding {
        jid: jid.into(),
        folder: "test".into(),
        name: "Test".into(),
        channel: "web".into(),
        group_type: "chat".into(),
        is_admin,
        requires_trigger: false,
        allowed_tools: None,
        allowed_paths: None,
        allowed_work_dirs: None,
        bot_token: None,
        max_messages: None,
        last_active: None,
        added_at: "2026-01-01T00:00:00Z".into(),
    }
}

#[tokio::test]
async fn zen_core_api_process_message_dispatches() {
    let api = ZenCoreApi::new(None);
    let result = api.process_message("test:1", "hello", &fake_binding("test:1", false));
    assert!(result.is_ok());
}

#[test]
fn agent_pool_send_reply_no_callback_does_not_panic() {
    let pool = AgentPool::new(Arc::new(ZenCoreApi::new(None)));
    // Default permissions config is all-false.
    let cfg = pool.get_permissions_config();
    assert!(!cfg.skip_main_agent_permissions);
    assert!(!cfg.skip_all_agents_permissions);
    // notify_activity on unknown JID is a no-op.
    pool.notify_activity("nobody:0");
}

#[test]
fn permissions_config_round_trips() {
    let pool = AgentPool::new(Arc::new(ZenCoreApi::new(None)));
    pool.set_permissions_config(PermissionsConfig {
        skip_main_agent_permissions: true,
        skip_all_agents_permissions: false,
    });
    let cfg = pool.get_permissions_config();
    assert!(cfg.skip_main_agent_permissions);
    assert!(!cfg.skip_all_agents_permissions);
    assert!(pool.get_skip_perms_for_virtual());
}

#[test]
fn thinking_default_on() {
    let pool = AgentPool::new(Arc::new(ZenCoreApi::new(None)));
    assert!(pool.get_thinking_enabled());
    pool.set_thinking_enabled(false);
    assert!(!pool.get_thinking_enabled());
}

#[test]
fn skip_perms_admin_with_main_flag() {
    let opts = PermissionsConfig {
        skip_main_agent_permissions: true,
        skip_all_agents_permissions: false,
    };
    let admin = fake_binding("admin:1", true);
    let regular = fake_binding("group:1", false);
    let dispatch_set = HashSet::new();
    assert!(AgentPool::compute_skip_perms(&opts, &admin, &dispatch_set));
    assert!(!AgentPool::compute_skip_perms(
        &opts,
        &regular,
        &dispatch_set
    ));
}

#[test]
fn skip_perms_dispatch_subagent_inherits_main() {
    let opts = PermissionsConfig {
        skip_main_agent_permissions: true,
        skip_all_agents_permissions: false,
    };
    let sub = fake_binding("sub:1", false);
    let mut dispatch_set = HashSet::new();
    dispatch_set.insert("sub:1".to_string());
    assert!(AgentPool::compute_skip_perms(&opts, &sub, &dispatch_set));
}

#[test]
fn skip_perms_skip_all_overrides_everything() {
    let opts = PermissionsConfig {
        skip_main_agent_permissions: false,
        skip_all_agents_permissions: true,
    };
    let regular = fake_binding("g:1", false);
    let dispatch_set = HashSet::new();
    assert!(AgentPool::compute_skip_perms(
        &opts,
        &regular,
        &dispatch_set
    ));
}

#[test]
fn dispatch_executing_mark_clear() {
    let pool = AgentPool::new(Arc::new(ZenCoreApi::new(None)));
    pool.mark_dispatch_executing("g:1");
    assert!(pool
        .state
        .lock()
        .unwrap()
        .dispatch_executing
        .contains("g:1"));
    pool.clear_dispatch_executing("g:1");
    assert!(!pool
        .state
        .lock()
        .unwrap()
        .dispatch_executing
        .contains("g:1"));
}

#[test]
fn dispatch_task_map_round_trip() {
    let pool = AgentPool::new(Arc::new(ZenCoreApi::new(None)));
    pool.set_current_dispatch_task_id("g:1", "task-42");
    let s = pool.state.lock().unwrap();
    assert_eq!(
        s.dispatch_task_map.get("g:1").map(String::as_str),
        Some("task-42")
    );
}

#[test]
fn notify_dispatch_skips_when_no_pending_reply() {
    let pool = AgentPool::new(Arc::new(ZenCoreApi::new(None)));
    // No content recorded → silent no-op (no panic).
    pool.notify_dispatch_if_pending("g:1", Some("task-1"));
}

#[test]
fn workspace_state_file_path_format() {
    let pool = AgentPool::new(Arc::new(ZenCoreApi::new(None)));
    let tmp = std::env::temp_dir().join(format!("senclaw-test-{}", std::process::id()));
    pool.set_senclaw_home(tmp.clone());
    let p = pool.workspace_state_file("main");
    assert_eq!(p, tmp.join("workspace-state-main.json"));
}

#[test]
fn init_workspace_state_writes_default() {
    let tmp = tempfile::tempdir().unwrap();
    let state_file = tmp.path().join("workspace-state-foo.json");
    let default_dir = tmp.path().join("foo-workspace");
    AgentPool::init_workspace_state(&state_file, &default_dir);
    let raw = std::fs::read_to_string(&state_file).unwrap();
    let parsed: WorkspaceStateFile = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed.current_dir, default_dir.to_string_lossy());
    assert!(!parsed.updated_at.is_empty());
}

#[test]
fn init_workspace_state_skips_when_file_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let state_file = tmp.path().join("ws.json");
    std::fs::write(&state_file, r#"{"currentDir":"/custom","updatedAt":""}"#).unwrap();
    AgentPool::init_workspace_state(&state_file, &tmp.path().join("default"));
    let raw = std::fs::read_to_string(&state_file).unwrap();
    assert!(raw.contains("/custom"));
}

#[test]
fn cached_todos_empty_by_default() {
    let pool = AgentPool::new(Arc::new(ZenCoreApi::new(None)));
    assert!(pool.get_all_cached_todos().is_empty());
}
