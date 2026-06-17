//! Hook configuration loader. Mirrors `src-old/hooks/HookConfigLoader.ts`.
//!
//! Loads and validates hook configurations from multiple sources:
//! - Global hooks.json
//! - Workspace hooks.json
//! - Plugin marketplace hooks.json
//!
//! Uses zen_core hook types to avoid duplication.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::warn;

// Re-export zen_core hook types
use crate::zen_core::hooks::{
    HookConfig as ZenHookConfig, HookDefinition as ZenHookDefinition, HookEvent,
    HookEventConfig as ZenHookEventConfig, HookType as ZenHookType,
};

/// Hook configuration with string keys for JSON deserialization.
/// This is an intermediate format that gets converted to ZenHookConfig.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HookConfig {
    pub hooks: HashMap<String, Vec<HookEventConfig>>,
}

/// Hook event configuration with matcher and conditions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEventConfig {
    pub matcher: Option<String>,
    #[serde(rename = "if")]
    pub condition: Option<String>,
    pub hooks: Vec<HookDefinition>,
}

/// Hook definition (intermediate format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDefinition {
    #[serde(rename = "type")]
    pub hook_type: String,
    pub command: Option<String>,
    pub prompt: Option<String>,
    pub timeout: Option<u32>,
    pub blocking: Option<bool>,
    #[serde(rename = "async")]
    pub is_async: Option<bool>,
    #[serde(rename = "include_history")]
    pub include_history: Option<bool>,
    #[serde(rename = "history_limit")]
    pub history_limit: Option<u32>,
}

/// Valid hook event names (aligned with sema-core).
const VALID_HOOK_EVENTS: &[&str] = &[
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PermissionRequest",
    "Stop",
    "SessionStart",
    "PreCompact",
    "PostCompact",
];

/// Default timeout for blocking marketplace hooks without explicit timeout (seconds).
const MARKETPLACE_BLOCKING_DEFAULT_TIMEOUT: u32 = 30;

/// Convert string hook type to zen_core HookType.
fn parse_hook_type(s: &str) -> Option<ZenHookType> {
    match s.to_lowercase().as_str() {
        "command" => Some(ZenHookType::Command),
        "prompt" => Some(ZenHookType::Prompt),
        _ => None,
    }
}

/// Convert string event name to HookEvent.
fn parse_hook_event(s: &str) -> Option<HookEvent> {
    match s {
        "UserPromptSubmit" => Some(HookEvent::UserPromptSubmit),
        "PreToolUse" => Some(HookEvent::PreToolUse),
        "PostToolUse" => Some(HookEvent::PostToolUse),
        "PermissionRequest" => Some(HookEvent::PermissionRequest),
        "PrePermission" => Some(HookEvent::PrePermission),
        "OutputFilter" => Some(HookEvent::OutputFilter),
        "Stop" => Some(HookEvent::Stop),
        "SessionStart" => Some(HookEvent::SessionStart),
        "SessionEnd" => Some(HookEvent::SessionEnd),
        "PreCompact" => Some(HookEvent::PreCompact),
        "PostCompact" => Some(HookEvent::PostCompact),
        "Notification" => Some(HookEvent::Notification),
        "Error" => Some(HookEvent::Error),
        "SubagentStart" => Some(HookEvent::SubagentStart),
        "SubagentEnd" => Some(HookEvent::SubagentEnd),
        _ => None,
    }
}

/// Convert intermediate HookConfig to zen_core HookConfig.
pub fn convert_to_zen_hook_config(config: HookConfig) -> Result<ZenHookConfig, String> {
    let mut zen_config = ZenHookConfig::default();

    for (event_name, event_configs) in config.hooks {
        let hook_event = parse_hook_event(&event_name)
            .ok_or_else(|| format!("Unknown hook event: {}", event_name))?;

        let mut zen_event_configs = Vec::new();

        for event_config in event_configs {
            let mut zen_hooks = Vec::new();

            for hook in event_config.hooks {
                let hook_type = parse_hook_type(&hook.hook_type)
                    .ok_or_else(|| format!("Invalid hook type: {}", hook.hook_type))?;

                let zen_hook = ZenHookDefinition {
                    hook_type,
                    command: hook.command,
                    prompt: hook.prompt,
                    timeout: hook.timeout.map(|t| t as u64),
                    blocking: hook.blocking,
                    is_async: hook.is_async,
                    include_history: hook.include_history,
                    history_limit: hook.history_limit.map(|h| h as usize),
                };

                zen_hooks.push(zen_hook);
            }

            zen_event_configs.push(ZenHookEventConfig {
                matcher: event_config.matcher,
                if_condition: event_config.condition,
                hooks: zen_hooks,
            });
        }

        zen_config.hooks.insert(hook_event, zen_event_configs);
    }

    Ok(zen_config)
}

/// Load and validate marketplace hook configuration.
///
/// Validates and filters invalid entries, patching potential issues.
/// Strategy:
/// - Unknown event names → skip entire group (warn)
/// - Missing required fields → skip that hook entry (warn)
/// - blocking=true without timeout → add timeout=30 and warn (don't skip)
fn validate_and_filter_marketplace_hook_config(config: HookConfig, file_path: &Path) -> HookConfig {
    let mut result = HookConfig::default();

    for (event, event_configs) in config.hooks {
        if !VALID_HOOK_EVENTS.contains(&event.as_str()) {
            warn!(
                "[hooks:marketplace] {} Unknown hook event {}, skipping entire group",
                file_path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown"),
                event
            );
            continue;
        }

        let mut valid_configs = Vec::new();

        for event_config in event_configs {
            let mut valid_hooks = Vec::new();

            for hook in event_config.hooks {
                // Validate hook type
                if parse_hook_type(&hook.hook_type).is_none() {
                    warn!(
                        "[hooks:marketplace] {} [{}] Invalid type {}, skipping",
                        file_path
                            .parent()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown"),
                        event,
                        hook.hook_type
                    );
                    continue;
                }

                // Validate required fields based on type
                if hook.hook_type.to_lowercase() == "command" && hook.command.is_none() {
                    warn!(
                        "[hooks:marketplace] {} [{}] type=command missing command field, skipping",
                        file_path
                            .parent()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown"),
                        event
                    );
                    continue;
                }

                if hook.hook_type.to_lowercase() == "prompt" && hook.prompt.is_none() {
                    warn!(
                        "[hooks:marketplace] {} [{}] type=prompt missing prompt field, skipping",
                        file_path
                            .parent()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown"),
                        event
                    );
                    continue;
                }

                // Add default timeout for blocking hooks without explicit timeout
                let hook = if hook.blocking == Some(true) && hook.timeout.is_none() {
                    warn!(
                        "[hooks:marketplace] {} [{}] blocking=true without timeout, applying default {}s",
                        file_path
                            .parent()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown"),
                        event,
                        MARKETPLACE_BLOCKING_DEFAULT_TIMEOUT
                    );
                    let mut hook = hook;
                    hook.timeout = Some(MARKETPLACE_BLOCKING_DEFAULT_TIMEOUT);
                    hook
                } else {
                    hook
                };

                valid_hooks.push(hook);
            }

            if !valid_hooks.is_empty() {
                valid_configs.push(HookEventConfig {
                    matcher: event_config.matcher,
                    condition: event_config.condition,
                    hooks: valid_hooks,
                });
            }
        }

        if !valid_configs.is_empty() {
            result.hooks.insert(event, valid_configs);
        }
    }

    result
}

/// Load hook configuration from JSON file.
///
/// Returns None if file doesn't exist or parsing fails.
fn load_hook_json(file_path: &Path) -> Option<HookConfig> {
    if !file_path.exists() {
        return None;
    }

    let raw = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read hook config from {}", file_path.display()))
        .ok()?;

    let parsed: HookConfig = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse hook config from {}", file_path.display()))
        .ok()?;

    Some(parsed)
}

/// Merge two hook configurations (global + workspace).
///
/// Merges EventConfig arrays for same events (additive, not overriding).
fn merge_hook_configs(global: Option<HookConfig>, workspace: Option<HookConfig>) -> HookConfig {
    match (global, workspace) {
        (None, None) => HookConfig::default(),
        (None, Some(w)) => w,
        (Some(g), None) => g,
        (Some(g), Some(w)) => {
            let mut merged = HookConfig {
                hooks: g.hooks.clone(),
            };

            for (event, configs) in w.hooks {
                let existing = merged.hooks.entry(event.clone()).or_insert_with(Vec::new);
                existing.extend(configs);
            }

            merged
        }
    }
}

/// Resolve variables in hook configuration.
///
/// Supports: ${SENCLAW_ROOT}, ${AGENT_WORKSPACE}
fn resolve_variables_in_config(config: HookConfig, env: &HashMap<String, String>) -> HookConfig {
    let mut resolved = HookConfig::default();

    for (event, configs) in config.hooks {
        let resolved_configs: Vec<HookEventConfig> = configs
            .iter()
            .map(|event_config| HookEventConfig {
                matcher: event_config.matcher.clone(),
                condition: event_config.condition.clone(),
                hooks: event_config
                    .hooks
                    .iter()
                    .map(|hook| HookDefinition {
                        hook_type: hook.hook_type.clone(),
                        command: hook
                            .command
                            .as_ref()
                            .map(|c| resolve_variables(c, env))
                            .or_else(|| hook.command.clone()),
                        prompt: hook
                            .prompt
                            .as_ref()
                            .map(|p| resolve_variables(p, env))
                            .or_else(|| hook.prompt.clone()),
                        timeout: hook.timeout,
                        blocking: hook.blocking,
                        is_async: hook.is_async,
                        include_history: hook.include_history,
                        history_limit: hook.history_limit,
                    })
                    .collect(),
            })
            .collect();

        resolved.hooks.insert(event, resolved_configs);
    }

    resolved
}

/// Resolve variable placeholders in a string.
fn resolve_variables(str: &str, env: &HashMap<String, String>) -> String {
    let mut result = str.to_string();

    for (key, value) in env {
        let placeholder = format!("${{{}}}", key);
        result = result.replace(&placeholder, value);
    }

    result
}

/// Resolve plugin root variables in hook configuration.
///
/// Replaces ${SENCLAW_PLUGIN_ROOT} / ${CLAUDE_PLUGIN_ROOT} with plugin directory,
/// allowing plugins to reference scripts bundled within themselves.
fn resolve_plugin_root_in_config(config: HookConfig, plugin_dir: &str) -> HookConfig {
    let mut resolved = HookConfig::default();

    for (event, configs) in config.hooks {
        let resolved_configs: Vec<HookEventConfig> = configs
            .iter()
            .map(|event_config| HookEventConfig {
                matcher: event_config.matcher.clone(),
                condition: event_config.condition.clone(),
                hooks: event_config
                    .hooks
                    .iter()
                    .map(|hook| HookDefinition {
                        hook_type: hook.hook_type.clone(),
                        command: hook
                            .command
                            .as_ref()
                            .map(|c| {
                                c.replace("${SENCLAW_PLUGIN_ROOT}", plugin_dir)
                                    .replace("${CLAUDE_PLUGIN_ROOT}", plugin_dir)
                            })
                            .or_else(|| hook.command.clone()),
                        prompt: hook
                            .prompt
                            .as_ref()
                            .map(|p| {
                                p.replace("${SENCLAW_PLUGIN_ROOT}", plugin_dir)
                                    .replace("${CLAUDE_PLUGIN_ROOT}", plugin_dir)
                            })
                            .or_else(|| hook.prompt.clone()),
                        timeout: hook.timeout,
                        blocking: hook.blocking,
                        is_async: hook.is_async,
                        include_history: hook.include_history,
                        history_limit: hook.history_limit,
                    })
                    .collect(),
            })
            .collect();

        resolved.hooks.insert(event, resolved_configs);
    }

    resolved
}

/// Load and merge hook configurations (global + workspace + plugin marketplace sources).
///
/// Search paths:
///   Global: ~/.senclaw/hooks.json
///   Workspace: <workspaceDir>/.senclaw/hooks.json
///   extraFiles: Plugin marketplace hooks.json files (already sorted by priority)
pub fn load_merged_hook_config(
    global_config_dir: &Path,
    workspace_dir: Option<&Path>,
    extra_files: Option<&[PathBuf]>,
) -> HookConfig {
    let global_hooks = load_hook_json(&global_config_dir.join("hooks.json"));
    let workspace_hooks =
        workspace_dir.and_then(|dir| load_hook_json(&dir.join(".senclaw").join("hooks.json")));

    let mut merged = merge_hook_configs(global_hooks, workspace_hooks);

    // Merge marketplace hook files: load → validate/filter → resolve plugin-root vars → merge (additive)
    for file_path in extra_files.unwrap_or(&[]) {
        let raw = load_hook_json(file_path);
        if raw.is_none() {
            continue;
        }

        let validated = validate_and_filter_marketplace_hook_config(raw.unwrap(), file_path);
        if validated.hooks.is_empty() {
            continue;
        }

        // Plugin layout: <pluginDir>/hooks/hooks.json — resolve ${SENCLAW_PLUGIN_ROOT}/${CLAUDE_PLUGIN_ROOT}
        let plugin_dir = file_path.parent().and_then(|p| p.to_str()).unwrap_or("");

        let resolved = resolve_plugin_root_in_config(validated, plugin_dir);
        merged = merge_hook_configs(Some(merged), Some(resolved));
    }

    merged
}

/// Build hook environment variables.
pub fn resolve_hook_env(
    global_config_dir: &Path,
    workspace_dir: Option<&Path>,
) -> HashMap<String, String> {
    let mut env = HashMap::new();

    env.insert(
        "SENCLAW_ROOT".to_string(),
        global_config_dir.display().to_string(),
    );

    if let Some(workspace) = workspace_dir {
        env.insert(
            "AGENT_WORKSPACE".to_string(),
            workspace.display().to_string(),
        );
    } else {
        env.insert(
            "AGENT_WORKSPACE".to_string(),
            global_config_dir.display().to_string(),
        );
    }

    env
}

/// Complete hook configuration loading flow: load → merge → variable resolution.
///
/// Returns None if hooks are empty (sema-core won't create HookManager).
pub fn load_and_resolve_hook_config(
    global_config_dir: &Path,
    workspace_dir: Option<&Path>,
    extra_files: Option<&[PathBuf]>,
) -> Option<HookConfig> {
    let hook_config = load_merged_hook_config(global_config_dir, workspace_dir, extra_files);
    let env = resolve_hook_env(global_config_dir, workspace_dir);

    let resolved = resolve_variables_in_config(hook_config, &env);

    if resolved.hooks.is_empty() {
        None
    } else {
        Some(resolved)
    }
}

/// Load hook configuration and convert to zen_core format.
///
/// This is the main entry point for integration with zen_core hooks system.
pub fn load_zen_hook_config(
    global_config_dir: &Path,
    workspace_dir: Option<&Path>,
    extra_files: Option<&[PathBuf]>,
) -> Option<ZenHookConfig> {
    let hook_config = load_and_resolve_hook_config(global_config_dir, workspace_dir, extra_files)?;
    match convert_to_zen_hook_config(hook_config) {
        Ok(zen_config) => Some(zen_config),
        Err(e) => {
            warn!("Failed to convert hook config to zen_core format: {}", e);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_variables() {
        let mut env = HashMap::new();
        env.insert("ROOT".to_string(), "/path/to/root".to_string());
        env.insert("WORKSPACE".to_string(), "/path/to/workspace".to_string());

        let input = "Path: ${ROOT}/${WORKSPACE}";
        let resolved = resolve_variables(input, &env);
        assert_eq!(resolved, "Path: /path/to/root//path/to/workspace");
    }

    #[test]
    fn test_resolve_variables_unknown() {
        let mut env = HashMap::new();
        env.insert("ROOT".to_string(), "/path".to_string());

        let input = "Path: ${ROOT}/${UNKNOWN}";
        let resolved = resolve_variables(input, &env);
        assert_eq!(resolved, "Path: /path/${UNKNOWN}");
    }

    #[test]
    fn test_resolve_hook_env() {
        let global_dir = PathBuf::from("/global");
        let workspace_dir = Some(PathBuf::from("/workspace"));

        let env = resolve_hook_env(&global_dir, workspace_dir.as_deref());
        assert_eq!(env.get("SENCLAW_ROOT"), Some(&"/global".to_string()));
        assert_eq!(env.get("AGENT_WORKSPACE"), Some(&"/workspace".to_string()));
    }

    #[test]
    fn test_resolve_hook_env_no_workspace() {
        let global_dir = PathBuf::from("/global");

        let env = resolve_hook_env(&global_dir, None);
        assert_eq!(env.get("SENCLAW_ROOT"), Some(&"/global".to_string()));
        assert_eq!(env.get("AGENT_WORKSPACE"), Some(&"/global".to_string()));
    }

    #[test]
    fn test_merge_hook_configs() {
        let mut global = HookConfig::default();
        global.hooks.insert(
            "UserPromptSubmit".to_string(),
            vec![HookEventConfig {
                matcher: Some("test".to_string()),
                condition: None,
                hooks: vec![],
            }],
        );

        let mut workspace = HookConfig::default();
        workspace.hooks.insert(
            "UserPromptSubmit".to_string(),
            vec![HookEventConfig {
                matcher: Some("workspace".to_string()),
                condition: None,
                hooks: vec![],
            }],
        );

        let merged = merge_hook_configs(Some(global), Some(workspace));
        assert_eq!(merged.hooks["UserPromptSubmit"].len(), 2);
    }

    #[test]
    fn test_merge_hook_configs_global_only() {
        let mut global = HookConfig::default();
        global.hooks.insert(
            "TestEvent".to_string(),
            vec![HookEventConfig {
                matcher: None,
                condition: None,
                hooks: vec![],
            }],
        );

        let merged = merge_hook_configs(Some(global), None);
        assert_eq!(merged.hooks["TestEvent"].len(), 1);
    }

    #[test]
    fn test_resolve_plugin_root_in_config() {
        let mut config = HookConfig::default();
        config.hooks.insert(
            "TestEvent".to_string(),
            vec![HookEventConfig {
                matcher: None,
                condition: None,
                hooks: vec![HookDefinition {
                    hook_type: "command".to_string(),
                    command: Some("${SENCLAW_PLUGIN_ROOT}/script.sh".to_string()),
                    prompt: None,
                    timeout: None,
                    blocking: None,
                    is_async: None,
                    include_history: None,
                    history_limit: None,
                }],
            }],
        );

        let resolved = resolve_plugin_root_in_config(config, "/plugin/path");
        assert_eq!(
            resolved.hooks["TestEvent"][0].hooks[0].command,
            Some("/plugin/path/script.sh".to_string())
        );
    }

    #[test]
    fn test_parse_hook_type() {
        assert_eq!(parse_hook_type("command"), Some(ZenHookType::Command));
        assert_eq!(parse_hook_type("COMMAND"), Some(ZenHookType::Command));
        assert_eq!(parse_hook_type("prompt"), Some(ZenHookType::Prompt));
        assert_eq!(parse_hook_type("PROMPT"), Some(ZenHookType::Prompt));
        assert_eq!(parse_hook_type("invalid"), None);
    }

    #[test]
    fn test_parse_hook_event() {
        assert_eq!(
            parse_hook_event("UserPromptSubmit"),
            Some(HookEvent::UserPromptSubmit)
        );
        assert_eq!(parse_hook_event("PreToolUse"), Some(HookEvent::PreToolUse));
        assert_eq!(
            parse_hook_event("PostToolUse"),
            Some(HookEvent::PostToolUse)
        );
        assert_eq!(
            parse_hook_event("PermissionRequest"),
            Some(HookEvent::PermissionRequest)
        );
        assert_eq!(parse_hook_event("Stop"), Some(HookEvent::Stop));
        assert_eq!(
            parse_hook_event("SessionStart"),
            Some(HookEvent::SessionStart)
        );
        assert_eq!(parse_hook_event("SessionEnd"), Some(HookEvent::SessionEnd));
        assert_eq!(parse_hook_event("PreCompact"), Some(HookEvent::PreCompact));
        assert_eq!(
            parse_hook_event("PostCompact"),
            Some(HookEvent::PostCompact)
        );
        assert_eq!(parse_hook_event("InvalidEvent"), None);
    }

    #[test]
    fn test_convert_to_zen_hook_config() {
        let mut config = HookConfig::default();
        config.hooks.insert(
            "UserPromptSubmit".to_string(),
            vec![HookEventConfig {
                matcher: Some("*".to_string()),
                condition: None,
                hooks: vec![HookDefinition {
                    hook_type: "command".to_string(),
                    command: Some("echo hello".to_string()),
                    prompt: None,
                    timeout: Some(10),
                    blocking: Some(true),
                    is_async: None,
                    include_history: None,
                    history_limit: None,
                }],
            }],
        );

        let zen_config = convert_to_zen_hook_config(config).unwrap();
        assert!(zen_config.hooks.contains_key(&HookEvent::UserPromptSubmit));
        let event_configs = zen_config.hooks.get(&HookEvent::UserPromptSubmit).unwrap();
        assert_eq!(event_configs.len(), 1);
        assert_eq!(event_configs[0].matcher, Some("*".to_string()));
        assert_eq!(event_configs[0].hooks.len(), 1);
        assert_eq!(event_configs[0].hooks[0].hook_type, ZenHookType::Command);
        assert_eq!(
            event_configs[0].hooks[0].command,
            Some("echo hello".to_string())
        );
    }

    #[test]
    fn test_convert_to_zen_hook_config_invalid_event() {
        let mut config = HookConfig::default();
        config.hooks.insert(
            "InvalidEvent".to_string(),
            vec![HookEventConfig {
                matcher: None,
                condition: None,
                hooks: vec![],
            }],
        );

        let result = convert_to_zen_hook_config(config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown hook event"));
    }

    #[test]
    fn test_convert_to_zen_hook_config_invalid_type() {
        let mut config = HookConfig::default();
        config.hooks.insert(
            "UserPromptSubmit".to_string(),
            vec![HookEventConfig {
                matcher: None,
                condition: None,
                hooks: vec![HookDefinition {
                    hook_type: "invalid_type".to_string(),
                    command: None,
                    prompt: None,
                    timeout: None,
                    blocking: None,
                    is_async: None,
                    include_history: None,
                    history_limit: None,
                }],
            }],
        );

        let result = convert_to_zen_hook_config(config);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid hook type"));
    }
}
