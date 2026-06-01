//! Install / remove skills bundled with a Space App.
//!
//! A manifest may declare a `skills` array, each `{ name, path }` pointing at a
//! skill folder (containing `SKILL.md`) inside the app. On install we copy each
//! into the managed skills dir as `<app_id>__<skill>` and drop a
//! `.senclaw-app.json` marker so the scanner labels it `app:<app_id>` — which
//! makes it read-only in the UI/API. On app uninstall we remove every skill dir
//! carrying that app's marker.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::Config;

const MARKER: &str = ".senclaw-app.json";

fn managed_dir(config: &Config) -> &Path {
    &config.paths.managed_skills_dir
}

fn app_skill_dest(config: &Config, _app_id: &str, skill_name: &str) -> PathBuf {
    // The scanner uses the directory name as the skill's display name, so keep
    // it clean (just the skill name). Ownership is tracked via the marker file,
    // not the directory name.
    managed_dir(config).join(skill_name)
}

fn has_skill_md(dir: &Path) -> bool {
    ["SKILL.md", "skill.md", "Skill.md"]
        .iter()
        .any(|n| dir.join(n).is_file())
}

/// Install (or refresh) all skills declared in the manifest. Idempotent:
/// re-installing overwrites the existing copy.
pub fn install_app_skills(config: &Config, app_id: &str, app_dir: &Path, manifest: &Value) {
    let Some(skills) = manifest.get("skills").and_then(Value::as_array) else {
        return;
    };
    let _ = fs::create_dir_all(managed_dir(config));

    for sk in skills {
        let Some(name) = sk.get("name").and_then(Value::as_str) else {
            continue;
        };
        let rel = sk.get("path").and_then(Value::as_str).unwrap_or(name);
        let src = app_dir.join(rel);
        if !has_skill_md(&src) {
            tracing::warn!("[space-skills] app '{app_id}' skill '{name}': no SKILL.md at {src:?}");
            continue;
        }
        let dest = app_skill_dest(config, app_id, name);
        let _ = fs::remove_dir_all(&dest);
        if let Err(e) = copy_dir_all(&src, &dest) {
            tracing::warn!("[space-skills] copy '{name}' for '{app_id}' failed: {e}");
            continue;
        }
        let marker = serde_json::json!({ "app_id": app_id, "skill": name });
        let _ = fs::write(dest.join(MARKER), marker.to_string());
        tracing::info!("[space-skills] installed skill '{name}' for app '{app_id}'");
    }
}

/// Remove every managed skill dir tagged with this app's marker.
pub fn remove_app_skills(config: &Config, app_id: &str) {
    let Ok(items) = fs::read_dir(managed_dir(config)) else {
        return;
    };
    for item in items.flatten() {
        let dir = item.path();
        if !dir.is_dir() {
            continue;
        }
        let Ok(raw) = fs::read_to_string(dir.join(MARKER)) else {
            continue;
        };
        let owned = serde_json::from_str::<Value>(&raw)
            .ok()
            .and_then(|v| v.get("app_id").and_then(|x| x.as_str()).map(str::to_string));
        if owned.as_deref() == Some(app_id) {
            let _ = fs::remove_dir_all(&dir);
            tracing::info!("[space-skills] removed skill dir {dir:?} for app '{app_id}'");
        }
    }
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to)?;
        } else if ty.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
