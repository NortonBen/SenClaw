use std::fs;
use std::collections::HashSet;
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, StatusCode},
    response::{Json, Response},
};
use serde::Deserialize;

use crate::clawhub::client::{
    download_skill_zip, get_skill_meta, search_skills, DEFAULT_REGISTRY,
};
use crate::clawhub::lockfile::{
    extract_zip_to_dir, read_lockfile, write_lockfile, write_skill_origin,
};
use crate::clawhub::signal::emit_skills_refresh;
use crate::skills::disabled::{
    disable_skill, enable_skill, is_skill_disabled, read_disabled_skills,
};
use crate::skills::scan::load_all_local_skills;

use super::core::{AppError, UiState};

// ===== /api/skills =====

pub(crate) async fn skills_list(State(s): State<Arc<UiState>>) -> Json<serde_json::Value> {
    let skills = load_all_local_skills(&s.config);
    let disabled = read_disabled_skills();
    let result: Vec<serde_json::Value> = skills
        .iter()
        .map(|sk| {
            serde_json::json!({
                "name": sk.name,
                "description": sk.description,
                "version": sk.version,
                "source": sk.source,
                "dir": sk.dir,
                "disabled": disabled.contains(&sk.name),
            })
        })
        .collect();
    Json(serde_json::json!({ "skills": result }))
}

// ===== /api/skills/remote-search =====

#[derive(Deserialize)]
pub(crate) struct RemoteSearchQuery {
    q: Option<String>,
}

pub(crate) async fn skills_remote_search(
    State(s): State<Arc<UiState>>,
    Query(q): Query<RemoteSearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let query = q.q.unwrap_or_default();
    if query.trim().is_empty() {
        return Ok(Json(serde_json::json!({ "results": [] })));
    }
    let registry = std::env::var("CLAWHUB_REGISTRY")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY.to_string());
    let raw = search_skills(&query, Some(&registry), Some(20), None)
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, e.to_string()))?;
    let local_skills = load_all_local_skills(&s.config);
    let local_names: HashSet<&str> =
        local_skills.iter().map(|sk| sk.name.as_str()).collect();
    let results: Vec<serde_json::Value> = raw
        .into_iter()
        .map(|r| {
            let mut v = serde_json::to_value(&r).unwrap_or_default();
            v["installed"] = serde_json::Value::Bool(local_names.contains(r.slug.as_str()));
            v
        })
        .collect();
    Ok(Json(serde_json::json!({ "results": results })))
}

// ===== /api/skills/install =====

#[derive(Deserialize)]
pub(crate) struct SkillInstallBody {
    slug: String,
}

pub(crate) async fn skills_install(
    State(s): State<Arc<UiState>>,
    Json(body): Json<SkillInstallBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let slug = body.slug.trim().to_string();
    if slug.is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "slug required".into()));
    }
    if !slug.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(AppError(StatusCode::BAD_REQUEST, "invalid slug".into()));
    }
    let managed_dir = &s.config.paths.managed_skills_dir;
    let target = managed_dir.join(&slug);
    // Path traversal guard
    let canonical_managed = managed_dir
        .canonicalize()
        .unwrap_or_else(|_| managed_dir.clone());
    if !target
        .canonicalize()
        .unwrap_or_else(|_| target.clone())
        .starts_with(&canonical_managed)
        && target.exists()
    {
        return Err(AppError(StatusCode::BAD_REQUEST, "invalid slug".into()));
    }

    let registry = std::env::var("CLAWHUB_REGISTRY")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_REGISTRY.to_string());

    let meta = get_skill_meta(&slug, Some(&registry), None)
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, e.to_string()))?;

    if meta.moderation.as_ref().map_or(false, |m| m.is_malware_blocked) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            format!("{slug} is flagged as malicious"),
        ));
    }

    let version = meta
        .latest_version
        .as_ref()
        .map(|v| v.version.clone())
        .ok_or_else(|| AppError(StatusCode::UNPROCESSABLE_ENTITY, "no version available".into()))?;

    let zip_buf = download_skill_zip(&slug, &version, Some(&registry), None)
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, e.to_string()))?;

    if target.exists() {
        let _ = tokio::fs::remove_dir_all(&target).await;
    }
    extract_zip_to_dir(&zip_buf, &target)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let _ = write_skill_origin(
        &target,
        &crate::clawhub::lockfile::SkillOrigin {
            version: 1,
            registry,
            slug: slug.clone(),
            installed_version: version.clone(),
            installed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        },
    );

    let mut lock = read_lockfile(managed_dir);
    lock.skills.insert(
        slug.clone(),
        crate::clawhub::lockfile::LockfileEntry {
            version: Some(version.clone()),
            installed_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        },
    );
    let _ = write_lockfile(managed_dir, &lock);

    if let Some(ref api) = s.agent_api {
        api.reload_all_skills();
    }
    let _ = emit_skills_refresh(&s.config);

    Ok(Json(serde_json::json!({ "ok": true, "slug": slug, "version": version })))
}

// ===== /api/skills/{name}/readme =====

pub(crate) async fn skills_readme(
    State(s): State<Arc<UiState>>,
    AxumPath(name): AxumPath<String>,
) -> Result<Response, AppError> {
    let skills = load_all_local_skills(&s.config);
    let skill = skills
        .iter()
        .find(|sk| sk.name == name)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Not found".into()))?;
    let content = fs::read_to_string(&skill.file_path).unwrap_or_default();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(content))
        .unwrap())
}

pub(crate) async fn skills_readme_save(
    State(s): State<Arc<UiState>>,
    AxumPath(name): AxumPath<String>,
    body: String,
) -> Result<Json<serde_json::Value>, AppError> {
    let skills = load_all_local_skills(&s.config);
    let skill = skills
        .iter()
        .find(|sk| sk.name == name)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Not found".into()))?;
    fs::write(&skill.file_path, &body)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ===== /api/skills/{name}/{enable|disable} =====

pub(crate) async fn skills_toggle(
    State(s): State<Arc<UiState>>,
    AxumPath((name, action)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let skills = load_all_local_skills(&s.config);
    if !skills.iter().any(|sk| sk.name == name) {
        return Err(AppError(StatusCode::NOT_FOUND, "Skill not found".into()));
    }
    match action.as_str() {
        "enable" => enable_skill(&name),
        "disable" => disable_skill(&name),
        _ => return Err(AppError(StatusCode::BAD_REQUEST, "action must be enable or disable".into())),
    }
    if let Some(ref api) = s.agent_api {
        api.reload_all_skills();
    }
    let _ = emit_skills_refresh(&s.config);
    let disabled = is_skill_disabled(&name);
    Ok(Json(serde_json::json!({ "name": name, "disabled": disabled })))
}
