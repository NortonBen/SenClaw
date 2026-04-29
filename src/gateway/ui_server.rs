//! UI HTTP server. Port target: src-old/gateway/UIServer.ts
//!
//! Listens on 127.0.0.1:18788 by default (overridable via `GATEWAY_UI_PORT`).
//! Serves the React web UI from `web/dist/` and exposes REST API endpoints for
//! the frontend: config, skills, subagents, wiki, admin permissions, quicknotes.
//!
//! LLM config endpoints (`/api/llm-config/*`) are stubbed — they require the
//! `sema-code-core` model manager which hasn't been ported yet.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use axum::{
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tower_http::{cors::CorsLayer, services::ServeDir};

use crate::clawhub::client::{
    download_skill_zip, get_skill_meta, search_skills, DEFAULT_REGISTRY,
};
use crate::clawhub::lockfile::{
    extract_zip_to_dir, read_lockfile, write_lockfile, write_skill_origin,
};
use crate::clawhub::signal::emit_skills_refresh;
use crate::config::Config;
use crate::gateway::group_manager::{
    get_admin_permissions_config, get_thinking_enabled, load_llm_configs, remove_llm_config,
    save_admin_permissions_config, save_llm_config, save_thinking_enabled,
    set_active_llm_config, set_active_quick_llm_config, AdminPermissions, LlmConfig,
};
use crate::skills::disabled::{
    disable_skill, enable_skill, is_skill_disabled, read_disabled_skills,
};
use crate::skills::scan::load_all_local_skills;
use crate::subagents::disabled::{
    disable_subagent, enable_subagent, is_subagent_disabled, read_disabled_subagents,
};
use crate::wiki::manager::WikiManager;

// ===== Trait for AgentPool-dependent operations =====

/// Operations the UI server needs from AgentPool (stubbed until sema-core arrives).
#[async_trait]
pub trait UiApi: Send + Sync {
    /// Signal all agents to reload their skill registries.
    fn reload_all_skills(&self) {}
    /// Get current thinking-enabled state.
    fn get_thinking_enabled(&self) -> bool {
        false
    }
    /// Set thinking-enabled state.
    fn set_thinking_enabled(&self, _enabled: bool) {}
    /// Get current admin permissions config.
    fn get_permissions_config(&self) -> AdminPermissionsConfig {
        AdminPermissionsConfig::default()
    }
    /// Set admin permissions config.
    fn set_permissions_config(&self, _cfg: AdminPermissionsConfig) {}
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminPermissionsConfig {
    #[serde(rename = "skipMainAgentPermissions")]
    pub skip_main_agent_permissions: bool,
    #[serde(rename = "skipAllAgentsPermissions")]
    pub skip_all_agents_permissions: bool,
}

impl Default for AdminPermissionsConfig {
    fn default() -> Self {
        Self {
            skip_main_agent_permissions: false,
            skip_all_agents_permissions: false,
        }
    }
}

// ===== MIME types =====

const MIME: &[(&str, &str)] = &[
    (".html", "text/html; charset=utf-8"),
    (".js", "application/javascript"),
    (".mjs", "application/javascript"),
    (".css", "text/css"),
    (".svg", "image/svg+xml"),
    (".png", "image/png"),
    (".ico", "image/x-icon"),
    (".json", "application/json"),
    (".woff2", "font/woff2"),
    (".woff", "font/woff"),
    (".ttf", "font/ttf"),
];

// ===== Shared state =====

pub struct UiState {
    pub config: Arc<Config>,
    pub wiki_manager: Option<Arc<WikiManager>>,
    pub persona_registry: Option<Arc<Mutex<crate::agent::persona_registry::PersonaRegistry>>>,
    pub agent_api: Option<Arc<dyn UiApi>>,
    pub ws_port: u16,
    pub ws_token: String,
}

/// Return the web/dist directory, falling back to cwd-based path.
fn resolve_dist_dir() -> PathBuf {
    // Try relative to the binary first, then cwd
    let cwd_dist = PathBuf::from("web/dist");
    if cwd_dist.exists() {
        return cwd_dist;
    }
    // Try from workspace root (development)
    let workspace_dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web/dist");
    if workspace_dist.exists() {
        return workspace_dist;
    }
    cwd_dist
}

// ===== Router construction =====

pub fn build_router(state: Arc<UiState>) -> Router {
    let dist_dir = resolve_dist_dir();

    let serve_dir = ServeDir::new(&dist_dir)
        .precompressed_gzip()
        .precompressed_br();

    Router::new()
        // API endpoints
        .route("/api/config", get(config_handler))
        .route("/api/skills", get(skills_list))
        .route("/api/skills/remote-search", get(skills_remote_search))
        .route("/api/skills/install", post(skills_install))
        .route("/api/skills/:name/readme", get(skills_readme).put(skills_readme_save))
        .route("/api/skills/:name/:action", post(skills_toggle))
        .route("/api/subagents", get(subagents_list))
        .route("/api/subagents/create", post(subagents_create))
        .route("/api/subagents/:name/readme", get(subagents_readme).put(subagents_readme_save))
        .route("/api/subagents/:name/:action", post(subagents_toggle))
        .route("/api/thinking", post(thinking_handler))
        .route("/api/admin-permissions", get(admin_perms_get).post(admin_perms_set))
        .route("/api/quicknotes", post(quicknotes_save))
        // LLM config (specific routes before parameterized)
        .route("/api/llm-config", get(llm_config_list).post(llm_config_create))
        .route("/api/llm-config/active", post(llm_config_set_active))
        .route("/api/llm-config/test", post(llm_config_test))
        .route("/api/llm-config/models", post(llm_config_fetch_models))
        .route("/api/llm-config/:id", delete(llm_config_delete))
        // Wiki API
        .route("/api/wiki/tree", get(wiki_tree))
        .route("/api/wiki/file", get(wiki_read).put(wiki_write))
        .route("/api/wiki/search", get(wiki_search))
        .route("/api/wiki/stats", get(wiki_stats))
        .route("/api/wiki/history", get(wiki_history))
        .route("/api/wiki/tags", get(wiki_tags))
        .route("/api/wiki/mkdir", post(wiki_mkdir))
        .route("/api/wiki/dir", delete(wiki_dir_delete))
        // Static files
        .nest_service("/", serve_dir)
        // SPA fallback
        .fallback(get(move |headers: HeaderMap| spa_fallback(dist_dir.clone(), headers)))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ===== /api/config =====

async fn config_handler(State(s): State<Arc<UiState>>) -> Json<serde_json::Value> {
    let admin_perms = get_admin_permissions_config(&s.config.paths.global_config_path);
    Json(serde_json::json!({
        "wsPort": s.ws_port,
        "token": s.ws_token,
        "thinkingEnabled": get_thinking_enabled(&s.config.paths.global_config_path),
        "skipMainAgentPermissions": admin_perms.skip_main_agent_permissions,
        "skipAllAgentsPermissions": admin_perms.skip_all_agents_permissions,
    }))
}

// ===== /api/skills =====

async fn skills_list(State(s): State<Arc<UiState>>) -> Json<serde_json::Value> {
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
struct RemoteSearchQuery {
    q: Option<String>,
}

async fn skills_remote_search(
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
    let local_names: std::collections::HashSet<&str> =
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
struct SkillInstallBody {
    slug: String,
}

async fn skills_install(
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

async fn skills_readme(
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

async fn skills_readme_save(
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

async fn skills_toggle(
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

// ===== /api/subagents =====

async fn subagents_list(State(s): State<Arc<UiState>>) -> Json<serde_json::Value> {
    let personas: Vec<serde_json::Value> = if let Some(ref pr) = s.persona_registry {
        let mut reg = pr.lock().unwrap();
        reg.reload();
        let disabled = read_disabled_subagents();
        reg.list()
            .into_iter()
            .map(|p| {
                serde_json::json!({
                    "name": p.name,
                    "description": p.description,
                    "tools": p.tools,
                    "model": p.model,
                    "maxConcurrent": p.max_concurrent,
                    "filePath": p.file_path,
                    "disabled": disabled.contains(&p.name),
                })
            })
            .collect()
    } else {
        Vec::new()
    };
    Json(serde_json::json!({ "subagents": personas }))
}

// ===== /api/subagents/create =====

#[derive(Deserialize)]
struct SubagentCreateBody {
    name: String,
    content: String,
}

async fn subagents_create(
    State(s): State<Arc<UiState>>,
    Json(body): Json<SubagentCreateBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.name.is_empty() || body.content.is_empty() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "name and content are required".into(),
        ));
    }
    let filename = sanitize_persona_filename(&body.name);
    if filename.is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid name".into()));
    }

    let dir = &s.config.paths.virtual_agents_dir;
    let file_path = dir.join(format!("{filename}.md"));

    if file_path.exists() {
        return Err(AppError(
            StatusCode::CONFLICT,
            format!("A persona file \"{filename}.md\" already exists."),
        ));
    }

    // Check for duplicate names
    if let Some(ref pr) = s.persona_registry {
        let reg = pr.lock().unwrap();
        if reg.get(&body.name).is_some() {
            return Err(AppError(
                StatusCode::CONFLICT,
                format!("A persona named \"{}\" already exists.", body.name),
            ));
        }
    }

    fs::create_dir_all(dir)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    fs::write(&file_path, &body.content)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if let Some(ref pr) = s.persona_registry {
        pr.lock().unwrap().reload();
    }

    Ok(Json(serde_json::json!({ "ok": true, "filename": format!("{filename}.md") })))
}

fn sanitize_persona_filename(name: &str) -> String {
    name.trim()
        .replace(char::is_whitespace, "-")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || c.is_alphabetic())
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

// ===== /api/subagents/{name}/readme =====

async fn subagents_readme(
    State(s): State<Arc<UiState>>,
    AxumPath(name): AxumPath<String>,
) -> Result<Response, AppError> {
    let persona = get_persona_file(&s, &name).await?;
    let content = fs::read_to_string(&persona.file_path).unwrap_or_default();
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(content))
        .unwrap())
}

async fn subagents_readme_save(
    State(s): State<Arc<UiState>>,
    AxumPath(name): AxumPath<String>,
    body: String,
) -> Result<Json<serde_json::Value>, AppError> {
    let persona = get_persona_file(&s, &name).await?;
    fs::write(&persona.file_path, &body)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(ref pr) = s.persona_registry {
        pr.lock().unwrap().reload();
    }
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// Helper to find a persona by name, returning its PersonaConfig.
async fn get_persona_file(
    s: &UiState,
    name: &str,
) -> Result<crate::agent::persona_registry::PersonaConfig, AppError> {
    if let Some(ref pr) = s.persona_registry {
        let reg = pr.lock().unwrap();
        reg.get(name)
            .cloned()
            .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Not found".into()))
    } else {
        Err(AppError(StatusCode::NOT_FOUND, "Not found".into()))
    }
}

// ===== /api/subagents/{name}/{enable|disable} =====

async fn subagents_toggle(
    State(s): State<Arc<UiState>>,
    AxumPath((name, action)): AxumPath<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Verify persona exists
    if let Some(ref pr) = s.persona_registry {
        let reg = pr.lock().unwrap();
        if reg.get(&name).is_none() {
            return Err(AppError(StatusCode::NOT_FOUND, "Not found".into()));
        }
    }
    match action.as_str() {
        "enable" => enable_subagent(&name),
        "disable" => disable_subagent(&name),
        _ => return Err(AppError(StatusCode::BAD_REQUEST, "action must be enable or disable".into())),
    }
    let disabled = is_subagent_disabled(&name);
    Ok(Json(serde_json::json!({ "name": name, "disabled": disabled })))
}

// ===== /api/thinking =====

#[derive(Deserialize)]
struct ThinkingBody {
    enabled: bool,
}

async fn thinking_handler(
    State(s): State<Arc<UiState>>,
    Json(body): Json<ThinkingBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    save_thinking_enabled(&s.config.paths.global_config_path, body.enabled)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(ref api) = s.agent_api {
        api.set_thinking_enabled(body.enabled);
    }
    Ok(Json(serde_json::json!({ "thinkingEnabled": body.enabled })))
}

// ===== /api/admin-permissions =====

async fn admin_perms_get(State(s): State<Arc<UiState>>) -> Json<serde_json::Value> {
    let cfg = get_admin_permissions_config(&s.config.paths.global_config_path);
    Json(serde_json::json!({
        "skipMainAgentPermissions": cfg.skip_main_agent_permissions,
        "skipAllAgentsPermissions": cfg.skip_all_agents_permissions,
    }))
}

async fn admin_perms_set(
    State(s): State<Arc<UiState>>,
    Json(body): Json<AdminPermissionsConfig>,
) -> Result<Json<serde_json::Value>, AppError> {
    let perm = AdminPermissions {
        skip_main_agent_permissions: body.skip_main_agent_permissions,
        skip_all_agents_permissions: body.skip_all_agents_permissions,
    };
    save_admin_permissions_config(&s.config.paths.global_config_path, &perm)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if let Some(ref api) = s.agent_api {
        api.set_permissions_config(body.clone());
    }
    Ok(Json(serde_json::to_value(body).unwrap_or_default()))
}

// ===== /api/quicknotes =====

#[derive(Deserialize)]
struct QuicknoteBody {
    text: String,
}

async fn quicknotes_save(
    State(_s): State<Arc<UiState>>,
    Json(body): Json<QuicknoteBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Derive filename from H1 → H2 → timestamp
    let raw_title = body
        .text
        .lines()
        .find(|l| l.starts_with("# ") && !l.starts_with("## "))
        .or_else(|| body.text.lines().find(|l| l.starts_with("## ")))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_else(|| {
            let now = chrono::Local::now();
            now.format("%Y-%m-%d-%H-%M-%S").to_string()
        });

    let safe = raw_title
        .chars()
        .filter(|c| !matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' | '\x00'..='\x1f'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(60)
        .collect::<String>();

    let safe = if safe.is_empty() { "quicknote".to_string() } else { safe };

    let dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("senclaw")
        .join("quicknotes");
    fs::create_dir_all(&dir)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Resolve filename conflicts
    let mut filename = format!("{safe}.md");
    let mut filepath = dir.join(&filename);
    let mut counter = 1u32;
    while filepath.exists() {
        filename = format!("{safe}-{counter}.md");
        filepath = dir.join(&filename);
        counter += 1;
    }

    fs::write(&filepath, &body.text)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({ "filename": filename })))
}

// ===== /api/llm-config/* =====

/// Body for creating a new LLM config (no id — auto-generated).
#[derive(Deserialize)]
struct NewLlmConfigBody {
    label: String,
    provider: String,
    #[serde(rename = "baseURL")]
    base_url: String,
    #[serde(rename = "apiKey")]
    api_key: String,
    #[serde(rename = "modelName")]
    model_name: String,
    adapt: String,
    #[serde(rename = "maxTokens")]
    max_tokens: u32,
    #[serde(rename = "contextLength")]
    context_length: u32,
}

/// Body for setting active model.
#[derive(Deserialize)]
struct ActiveLlmBody {
    id: Option<String>,
    #[serde(rename = "type", default = "default_llm_type")]
    llm_type: String,
}

fn default_llm_type() -> String {
    "main".to_string()
}

/// Body for test/fetch-models.
#[derive(Deserialize)]
struct LlmProviderBody {
    #[serde(rename = "baseURL")]
    base_url: String,
    #[serde(rename = "apiKey")]
    api_key: String,
    adapt: String,
}

/// GET /api/llm-config — list all configs
async fn llm_config_list(State(s): State<Arc<UiState>>) -> Json<serde_json::Value> {
    let stored = load_llm_configs(&s.config.paths.global_config_path);
    Json(serde_json::json!({
        "configs": stored.configs,
        "activeId": stored.active_id,
        "activeQuickId": stored.active_quick_id,
        "thinkingEnabled": get_thinking_enabled(&s.config.paths.global_config_path),
    }))
}

/// POST /api/llm-config — create or update config
async fn llm_config_create(
    State(s): State<Arc<UiState>>,
    Json(body): Json<NewLlmConfigBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let id = format!(
        "llm_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        rand::thread_rng().gen_range(1000u32..9999u32)
    );
    let cfg = LlmConfig {
        id: id.clone(),
        label: body.label,
        provider: body.provider,
        base_url: body.base_url,
        api_key: body.api_key,
        model_name: body.model_name,
        adapt: body.adapt,
        max_tokens: body.max_tokens,
        context_length: body.context_length,
    };
    save_llm_config(&s.config.paths.global_config_path, &cfg)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Auto-activate the first configuration
    let stored = load_llm_configs(&s.config.paths.global_config_path);
    if stored.configs.len() == 1 {
        let _ = set_active_llm_config(&s.config.paths.global_config_path, Some(&id));
    }

    Ok(Json(serde_json::to_value(&cfg).unwrap_or_default()))
}

/// DELETE /api/llm-config/{id} — delete config
async fn llm_config_delete(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
) -> impl IntoResponse {
    let id = id.trim().to_string();
    if id.is_empty() {
        return StatusCode::BAD_REQUEST;
    }
    let _ = remove_llm_config(&s.config.paths.global_config_path, &id);
    StatusCode::NO_CONTENT
}

/// POST /api/llm-config/active — set active main or quick model
async fn llm_config_set_active(
    State(s): State<Arc<UiState>>,
    Json(body): Json<ActiveLlmBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.llm_type == "quick" {
        set_active_quick_llm_config(
            &s.config.paths.global_config_path,
            body.id.as_deref(),
        )
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({ "activeQuickId": body.id })))
    } else {
        set_active_llm_config(&s.config.paths.global_config_path, body.id.as_deref())
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({ "activeId": body.id })))
    }
}

/// POST /api/llm-config/test — test provider connection
async fn llm_config_test(
    Json(body): Json<LlmProviderBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    match fetch_models(&body.base_url, &body.api_key, &body.adapt).await {
        Ok(_) => Ok(Json(
            serde_json::json!({ "success": true, "message": "Connected successfully" }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "message": e.to_string() }),
        )),
    }
}

/// POST /api/llm-config/models — fetch available models from provider
async fn llm_config_fetch_models(
    Json(body): Json<LlmProviderBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    match fetch_models(&body.base_url, &body.api_key, &body.adapt).await {
        Ok(models) => Ok(Json(
            serde_json::json!({ "success": true, "models": models }),
        )),
        Err(e) => Ok(Json(
            serde_json::json!({ "success": false, "message": e.to_string() }),
        )),
    }
}

/// Fetch model list from a provider's /models endpoint.
async fn fetch_models(base_url: &str, api_key: &str, adapt: &str) -> Result<Vec<String>, String> {
    let client = reqwest::Client::new();
    let is_anthropic = adapt == "anthropic"
        && base_url.contains("anthropic.com");

    let models_url = if is_anthropic {
        let base = base_url.trim_end_matches("/v1");
        format!("{base}/v1/models")
    } else {
        let base = base_url
            .trim_end_matches('/')
            .trim_end_matches("/chat/completions");
        format!("{base}/models")
    };

    let req = if is_anthropic {
        client
            .get(&models_url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
    } else {
        client
            .get(&models_url)
            .header("Authorization", format!("Bearer {api_key}"))
    };

    let resp = req
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("Connection failed: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("Read error: {e}"))?;

    if !status.is_success() {
        let preview: String = text.chars().take(200).collect();
        return Err(format!("HTTP {status}: {preview}"));
    }

    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("Invalid JSON: {e}"))?;
    let list: Vec<String> = json["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    Ok(list)
}

// ===== Wiki API handlers =====

fn wiki_manager(
    s: &UiState,
) -> Result<&WikiManager, AppError> {
    s.wiki_manager
        .as_ref()
        .map(|w| w.as_ref())
        .ok_or_else(|| AppError(StatusCode::SERVICE_UNAVAILABLE, "Wiki not initialized".into()))
}

async fn wiki_tree(State(s): State<Arc<UiState>>) -> Result<Json<serde_json::Value>, AppError> {
    let wm = wiki_manager(&s)?;
    let tree = wm.get_tree().map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "tree": tree })))
}

#[derive(Deserialize)]
struct WikiFileQuery {
    path: Option<String>,
}

async fn wiki_read(
    State(s): State<Arc<UiState>>,
    Query(q): Query<WikiFileQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = q.path.as_deref().ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Missing path".into()))?;
    let wm = wiki_manager(&s)?;
    let doc = wm
        .read_file(path)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let json = serde_json::json!({
        "path": doc.path,
        "content": doc.content,
        "frontmatter": doc.frontmatter,
        "gitLog": doc.git_log,
    });
    Ok(Json(json))
}

#[derive(Deserialize)]
struct WikiWriteBody {
    path: String,
    content: String,
    #[serde(rename = "commitMsg")]
    commit_msg: Option<String>,
    source: Option<String>,
    tags: Option<Vec<String>>,
}

async fn wiki_write(
    State(s): State<Arc<UiState>>,
    Json(body): Json<WikiWriteBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.path.is_empty() || body.content.is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "Missing path or content".into()));
    }
    let wm = wiki_manager(&s)?;
    wm.write_file(
        &body.path,
        &body.content,
        body.source.as_deref(),
        body.tags.as_deref(),
        body.commit_msg.as_deref(),
    )
    .await
    .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({
        "path": body.path,
        "updated": chrono::Utc::now().to_rfc3339(),
    })))
}

#[derive(Deserialize)]
struct WikiSearchQuery {
    q: Option<String>,
    tags: Option<String>,
    limit: Option<usize>,
}

async fn wiki_search(
    State(s): State<Arc<UiState>>,
    Query(q): Query<WikiSearchQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let query = q.q.unwrap_or_default();
    let tags: Option<Vec<String>> = q.tags.map(|t| {
        t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    });
    let limit = q.limit.unwrap_or(20);
    let wm = wiki_manager(&s)?;
    let results = wm
        .search(&query, tags.as_deref(), Some(limit))
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "results": results })))
}

async fn wiki_stats(State(s): State<Arc<UiState>>) -> Result<Json<serde_json::Value>, AppError> {
    let wm = wiki_manager(&s)?;
    let stats = wm
        .get_stats()
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::to_value(stats).unwrap_or_default()))
}

#[derive(Deserialize)]
struct WikiHistoryQuery {
    path: Option<String>,
    limit: Option<usize>,
}

async fn wiki_history(
    State(s): State<Arc<UiState>>,
    Query(q): Query<WikiHistoryQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = q.path.as_deref().ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Missing path".into()))?;
    let wm = wiki_manager(&s)?;
    let commits = wm
        .get_history(path, q.limit)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "commits": commits })))
}

async fn wiki_tags(State(s): State<Arc<UiState>>) -> Result<Json<serde_json::Value>, AppError> {
    let wm = wiki_manager(&s)?;
    let tags = wm.get_tags();
    Ok(Json(serde_json::json!({ "tags": tags })))
}

#[derive(Deserialize)]
struct WikiMkdirBody {
    path: Option<String>,
}

async fn wiki_mkdir(
    State(s): State<Arc<UiState>>,
    Json(body): Json<WikiMkdirBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let path = body.path.as_deref().ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Missing path".into()))?;
    let wm = wiki_manager(&s)?;
    wm.mkdir(path).await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "path": path })))
}

#[derive(Deserialize)]
struct WikiDirDeleteQuery {
    path: Option<String>,
}

async fn wiki_dir_delete(
    State(s): State<Arc<UiState>>,
    Query(q): Query<WikiDirDeleteQuery>,
) -> Result<impl IntoResponse, AppError> {
    let path = q.path.as_deref().ok_or_else(|| AppError(StatusCode::BAD_REQUEST, "Missing path".into()))?;
    let wm = wiki_manager(&s)?;
    wm.delete_empty_dir(path).await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn spa_fallback(dist_dir: PathBuf, headers: HeaderMap) -> Response {
    let path = headers
        .get("x-original-uri")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/");

    let is_wiki = path == "/wiki" || path.starts_with("/wiki/");
    let is_plugins = path == "/plugins" || path.starts_with("/plugins/");

    let fallback = if is_wiki {
        "wiki.html"
    } else if is_plugins {
        "plugins.html"
    } else {
        "index.html"
    };

    let file = dist_dir.join(fallback);
    match fs::read(&file) {
        Ok(contents) => {
            let mime = path_to_mime(fallback);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .body(Body::from(contents))
                .unwrap()
        }
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("Web UI not built. Run: npm run build:web"))
            .unwrap(),
    }
}

fn path_to_mime(path: &str) -> &'static str {
    if let Some(pos) = path.rfind('.') {
        let ext = &path[pos..];
        for (e, m) in MIME {
            if *e == ext {
                return m;
            }
        }
    }
    "application/octet-stream"
}

// ===== App error type =====

struct AppError(StatusCode, String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = serde_json::json!({ "error": self.1 });
        (self.0, Json(body)).into_response()
    }
}

// ===== Server launcher =====

/// Start the UI HTTP server on the configured port. Binds to 127.0.0.1.
pub async fn start_ui_server(state: Arc<UiState>, port: u16) -> Result<()> {
    let router = build_router(state);
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("bind {addr}"))?;
    tracing::info!("[UIServer] Web UI at http://{addr}");
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mime_lookup() {
        assert_eq!(path_to_mime("index.html"), "text/html; charset=utf-8");
        assert_eq!(path_to_mime("app.js"), "application/javascript");
        assert_eq!(path_to_mime("style.css"), "text/css");
        assert_eq!(path_to_mime("unknown.xyz"), "application/octet-stream");
    }

    #[test]
    fn test_sanitize_persona_filename() {
        assert_eq!(sanitize_persona_filename("My Agent"), "My-Agent");
        assert_eq!(sanitize_persona_filename("hello world"), "hello-world");
        assert_eq!(sanitize_persona_filename("  spaces  "), "spaces");
        assert_eq!(sanitize_persona_filename("safe_name"), "safe_name");
        assert_eq!(sanitize_persona_filename("with-dash"), "with-dash");
    }

    #[test]
    fn test_admin_perms_default() {
        let cfg = AdminPermissionsConfig::default();
        assert!(!cfg.skip_main_agent_permissions);
        assert!(!cfg.skip_all_agents_permissions);
    }
}
