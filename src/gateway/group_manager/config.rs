//! Global config load/save and group config persistence.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::config::Config;
use crate::db::Db;
use crate::types::GroupBinding;

use super::dirs::ensure_agent_dirs;
use super::manager::GroupManager;
use super::types::{GlobalConfig, GroupConfigEntry};

pub(super) fn load_global_config(path: &Path) -> GlobalConfig {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => GlobalConfig::default(),
    }
}

pub(super) fn save_global_config(path: &Path, cfg: &GlobalConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create dir {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(cfg)?;
    fs::write(path, json)?;
    Ok(())
}

pub(super) fn save_group_to_config(config_path: &Path, binding: &GroupBinding) {
    if binding.is_admin {
        return;
    }
    let mut cfg = load_global_config(config_path);
    let groups = cfg.groups.get_or_insert_with(Vec::new);
    let entry = GroupConfigEntry {
        jid: binding.jid.clone(),
        folder: binding.folder.clone(),
        name: binding.name.clone(),
        channel: Some(binding.channel.clone()).filter(|c| !c.is_empty()),
        group_type: Some(binding.group_type.clone()).filter(|t| t != "chat"),
        requires_trigger: Some(binding.requires_trigger),
        allowed_tools: binding.allowed_tools.clone(),
        allowed_paths: binding.allowed_paths.clone(),
        allowed_work_dirs: binding.allowed_work_dirs.clone(),
        bot_token: binding.bot_token.clone(),
        max_messages: binding.max_messages,
    };
    if let Some(existing) = groups.iter_mut().find(|g| g.jid == entry.jid) {
        *existing = entry;
    } else {
        groups.push(entry);
    }
    let _ = save_global_config(config_path, &cfg);
}

pub(super) fn remove_group_from_config(config_path: &Path, jid: &str) {
    let mut cfg = load_global_config(config_path);
    if let Some(ref mut groups) = cfg.groups {
        groups.retain(|g| g.jid != jid);
    }
    let _ = save_global_config(config_path, &cfg);
}

pub fn sync_groups_from_config(
    db: &Db,
    gm: &GroupManager,
    config: &Config,
) -> (usize, usize, usize) {
    let cfg = load_global_config(&config.paths.global_config_path);
    let config_groups = cfg.groups.unwrap_or_default();
    let config_jids: HashSet<&str> = config_groups.iter().map(|g| g.jid.as_str()).collect();
    let now = super::manager::chrono_now();
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut removed = 0usize;

    for entry in &config_groups {
        // Prevent folder UNIQUE conflicts
        if let Ok(all) = db.list_groups() {
            if let Some(_conflict) = all
                .iter()
                .find(|g| g.folder == entry.folder && g.jid != entry.jid)
            {
                let _ = db.delete_group_by_folder(&entry.folder);
            }
        }

        let existing = gm.get(db, &entry.jid);
        let binding = GroupBinding {
            jid: entry.jid.clone(),
            folder: entry.folder.clone(),
            name: entry.name.clone(),
            channel: entry.channel.clone().unwrap_or_default(),
            group_type: entry
                .group_type
                .clone()
                .unwrap_or_else(|| "chat".to_string()),
            is_admin: false,
            requires_trigger: entry.requires_trigger.unwrap_or(true),
            allowed_tools: entry.allowed_tools.clone(),
            allowed_paths: None,
            allowed_work_dirs: entry.allowed_work_dirs.clone(),
            bot_token: entry.bot_token.clone(),
            max_messages: entry.max_messages,
            last_active: existing.as_ref().and_then(|e| e.last_active.clone()),
            added_at: existing
                .as_ref()
                .map(|e| e.added_at.clone())
                .unwrap_or_else(|| now.clone()),
        };

        ensure_agent_dirs(config, &binding.folder, &binding.name);
        let _ = db.upsert_group(&binding);
        if existing.is_some() {
            updated += 1;
        } else {
            added += 1;
        }
    }

    // Delete non-admin DB groups not in config
    if let Ok(all) = gm.list(db) {
        for db_group in &all {
            if db_group.is_admin {
                continue;
            }
            if !config_jids.contains(db_group.jid.as_str()) {
                let _ = db.delete_group(&db_group.jid);
                removed += 1;
            }
        }
    }

    (added, updated, removed)
}

pub fn get_agent_allowed_work_dirs(
    config_path: &Path,
    folder: &str,
) -> Option<Option<Vec<String>>> {
    let cfg = load_global_config(config_path);
    match cfg.agents.and_then(|a| a.get(folder).cloned()) {
        None => None,                                 // not present in config
        Some(entry) => Some(entry.allowed_work_dirs), // null = switching disallowed
    }
}
