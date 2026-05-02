//! Feishu and QQ multi-app config functions.

use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use super::config::{load_global_config, save_global_config};
use super::types::{FeishuAppConfig, QqAppConfig};

// ===== Feishu app config =====

pub fn save_feishu_app(
    config_path: &Path,
    app_id: &str,
    app_secret: &str,
    domain: Option<&str>,
) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    let apps = cfg.feishu_apps.get_or_insert_with(HashMap::new);
    apps.insert(
        app_id.to_string(),
        FeishuAppConfig {
            app_secret: app_secret.to_string(),
            domain: domain.map(|s| s.to_string()),
        },
    );
    save_global_config(config_path, &cfg)
}

pub fn delete_feishu_app(config_path: &Path, app_id: &str) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    if let Some(ref mut apps) = cfg.feishu_apps {
        apps.remove(app_id);
    }
    save_global_config(config_path, &cfg)
}

pub fn get_feishu_apps(config_path: &Path) -> HashMap<String, FeishuAppConfig> {
    load_global_config(config_path)
        .feishu_apps
        .unwrap_or_default()
}

// ===== QQ multi-app config =====

pub fn save_qq_app(
    config_path: &Path,
    app_id: &str,
    app_secret: &str,
    sandbox: Option<bool>,
) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    let apps = cfg.qq_apps.get_or_insert_with(HashMap::new);
    apps.insert(
        app_id.to_string(),
        QqAppConfig {
            app_secret: app_secret.to_string(),
            sandbox,
        },
    );
    save_global_config(config_path, &cfg)
}

pub fn delete_qq_app(config_path: &Path, app_id: &str) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    if let Some(ref mut apps) = cfg.qq_apps {
        apps.remove(app_id);
    }
    save_global_config(config_path, &cfg)
}

pub fn get_qq_apps(config_path: &Path) -> HashMap<String, QqAppConfig> {
    load_global_config(config_path)
        .qq_apps
        .unwrap_or_default()
}
