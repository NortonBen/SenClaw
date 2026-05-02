//! Tests for the MCP manager module.

use super::*;
use crate::mcp::{ExternalMcpServerConfig, McpScopeType, McpServerStatus, McpTransportType};
use std::collections::HashMap;

#[test]
fn parse_valid_mcp_tool_name() {
    let result = parse_mcp_tool_name("mcp__my-server__my-tool");
    assert_eq!(
        result,
        Some(("my-server".to_string(), "my-tool".to_string()))
    );
}

#[test]
fn parse_mcp_tool_name_with_underscores() {
    let result = parse_mcp_tool_name("mcp__my_server__my_tool_v2");
    assert_eq!(
        result,
        Some(("my_server".to_string(), "my_tool_v2".to_string()))
    );
}

#[test]
fn parse_mcp_tool_name_with_double_underscore_in_tool() {
    // mcp__server__tool has trailing __ in tool name
    let result = parse_mcp_tool_name("mcp__server__list__tasks");
    // splitn(3, "__"): ["mcp", "server", "list__tasks"]
    assert_eq!(
        result,
        Some(("server".to_string(), "list__tasks".to_string()))
    );
}

#[test]
fn parse_invalid_mcp_tool_name() {
    assert_eq!(parse_mcp_tool_name("Bash"), None);
    assert_eq!(parse_mcp_tool_name("mcp__server"), None);
    assert_eq!(parse_mcp_tool_name(""), None);
}

#[test]
fn is_mcp_tool_positive() {
    assert!(is_mcp_tool("mcp__server__tool"));
    assert!(is_mcp_tool("mcp__x__y"));
}

#[test]
fn is_mcp_tool_negative() {
    assert!(!is_mcp_tool("Bash"));
    assert!(!is_mcp_tool("mcp_server_tool"));
}

#[tokio::test]
async fn manager_init_empty() {
    let dir = tempfile::TempDir::new().unwrap();
    let work = dir.path().join("project");
    std::fs::create_dir_all(&work).unwrap();
    let cfg_dir = dir.path().to_path_buf();

    let mgr = McpManager::new(work, cfg_dir);
    mgr.init().await.unwrap();

    let servers = mgr.get_all_servers().await;
    assert!(servers.is_empty());
}

#[tokio::test]
async fn manager_add_and_get_server() {
    let dir = tempfile::TempDir::new().unwrap();
    let work = dir.path().join("project");
    std::fs::create_dir_all(&work).unwrap();
    let cfg_dir = dir.path().to_path_buf();

    let mgr = McpManager::new(work.clone(), cfg_dir);

    let server = ExternalMcpServerConfig {
        name: "test-server".into(),
        transport: McpTransportType::Http,
        description: Some("A test server".into()),
        enabled: false, // don't auto-connect
        use_tools: None,
        command: None,
        args: vec![],
        env: HashMap::new(),
        url: Some("http://localhost:9999".into()),
        headers: HashMap::new(),
    };

    let info = mgr
        .add_or_update(server.clone(), McpScopeType::Project)
        .await
        .unwrap();
    assert_eq!(info.config.name, "test-server");
    assert_eq!(info.status, McpServerStatus::Disconnected);
    assert_eq!(info.scope, McpScopeType::Project);

    // Should be listed
    let servers = mgr.get_all_servers().await;
    assert_eq!(servers.len(), 1);
}

#[tokio::test]
async fn manager_remove_server() {
    let dir = tempfile::TempDir::new().unwrap();
    let work = dir.path().join("project");
    std::fs::create_dir_all(&work).unwrap();
    let cfg_dir = dir.path().to_path_buf();

    let mgr = McpManager::new(work.clone(), cfg_dir);

    let server = ExternalMcpServerConfig {
        name: "to-remove".into(),
        transport: McpTransportType::Http,
        description: None,
        enabled: false,
        use_tools: None,
        command: None,
        args: vec![],
        env: HashMap::new(),
        url: Some("http://localhost:9999".into()),
        headers: HashMap::new(),
    };

    mgr.add_or_update(server, McpScopeType::Project)
        .await
        .unwrap();
    assert_eq!(mgr.get_all_servers().await.len(), 1);

    let removed = mgr.remove("to-remove", McpScopeType::Project).await.unwrap();
    assert!(removed);
    assert_eq!(mgr.get_all_servers().await.len(), 0);
}

#[tokio::test]
async fn manager_builtin_servers_listed() {
    let dir = tempfile::TempDir::new().unwrap();
    let work = dir.path().join("project");
    std::fs::create_dir_all(&work).unwrap();
    let cfg_dir = dir.path().to_path_buf();

    let mgr = McpManager::new(work, cfg_dir);
    let builtins = mgr.get_builtin_servers();
    assert_eq!(builtins.len(), 7);
    // Each has a name starting with senclaw-
    for s in &builtins {
        assert!(s.name.starts_with("senclaw-"), "unexpected name: {}", s.name);
        assert!(!s.tools.is_empty(), "no tools for {}", s.name);
    }
}
