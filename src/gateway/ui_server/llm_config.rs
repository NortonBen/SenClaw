use std::sync::Arc;

use axum::{
    extract::{Path as AxumPath, State},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use rand::Rng;
use serde::Deserialize;

use crate::gateway::group_manager::{
    get_thinking_enabled, load_llm_configs, remove_llm_config, save_llm_config,
    set_active_llm_config, set_active_quick_llm_config, LlmConfig,
};

use super::core::{AppError, UiState};

// ===== /api/llm-config/* =====

/// Body for creating a new LLM config (no id — auto-generated).
#[derive(Deserialize)]
pub(crate) struct NewLlmConfigBody {
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
    /// Explicitly declare whether vision input is supported; undefined = auto-infer from modelName
    #[serde(default)]
    vision: Option<bool>,
}

/// Body for setting active model.
#[derive(Deserialize)]
pub(crate) struct ActiveLlmBody {
    id: Option<String>,
    #[serde(rename = "type", default = "default_llm_type")]
    llm_type: String,
}

fn default_llm_type() -> String {
    "main".to_string()
}

/// Body for test/fetch-models.
#[derive(Deserialize)]
pub(crate) struct LlmProviderBody {
    #[serde(rename = "baseURL")]
    base_url: String,
    #[serde(rename = "apiKey")]
    api_key: String,
    adapt: String,
}

/// Body for updating LLM config fields (partial update).
#[derive(Deserialize)]
pub(crate) struct UpdateLlmConfigBody {
    /// Explicitly declare whether vision input is supported; null = reset to auto-infer
    #[serde(default)]
    vision: Option<bool>,
}

/// GET /api/llm-config — list all configs
pub(crate) async fn llm_config_list(State(s): State<Arc<UiState>>) -> Json<serde_json::Value> {
    let stored = load_llm_configs(&s.config.paths.global_config_path);
    Json(serde_json::json!({
        "configs": stored.configs,
        "activeId": stored.active_id,
        "activeQuickId": stored.active_quick_id,
        "thinkingEnabled": get_thinking_enabled(&s.config.paths.global_config_path),
    }))
}

/// POST /api/llm-config — create or update config
pub(crate) async fn llm_config_create(
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
        vision: body.vision,
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
pub(crate) async fn llm_config_delete(
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

/// PATCH /api/llm-config/{id} — update config fields
pub(crate) async fn llm_config_update(
    State(s): State<Arc<UiState>>,
    AxumPath(id): AxumPath<String>,
    Json(body): Json<UpdateLlmConfigBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    let id = id.trim().to_string();
    if id.is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "Invalid ID".to_string()));
    }

    // Load existing configs
    let stored = load_llm_configs(&s.config.paths.global_config_path);
    
    // Find the config to update
    let mut cfg = stored
        .configs
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Config not found".to_string()))?;

    // Update vision field if provided
    if body.vision.is_some() {
        cfg.vision = body.vision;
    }

    // Save the updated config
    save_llm_config(&s.config.paths.global_config_path, &cfg)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::to_value(cfg).unwrap_or_default()))
}

/// POST /api/llm-config/active — set active main or quick model
pub(crate) async fn llm_config_set_active(
    State(s): State<Arc<UiState>>,
    Json(body): Json<ActiveLlmBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if body.llm_type == "quick" {
        set_active_quick_llm_config(&s.config.paths.global_config_path, body.id.as_deref())
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({ "activeQuickId": body.id })))
    } else {
        set_active_llm_config(&s.config.paths.global_config_path, body.id.as_deref())
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(Json(serde_json::json!({ "activeId": body.id })))
    }
}

/// POST /api/llm-config/test — test provider connection
pub(crate) async fn llm_config_test(
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
pub(crate) async fn llm_config_fetch_models(
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
    let is_anthropic = adapt == "anthropic" && base_url.contains("anthropic.com");

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
