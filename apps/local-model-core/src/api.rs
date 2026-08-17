//! The model-management REST surface, shared by both engine apps.
//!
//! This is the daemon's old `/api/local-models/*` moved out. It is engine
//! agnostic — listing, downloading and deleting a checkpoint is the same work
//! whether MLX or Candle will run it — so both apps mount the same router and
//! add only their own `/api/engine/*` on top.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use serde_json::json;

use crate::{download, settings, store};

/// What an app must tell this module about its own engine.
pub trait EngineHost: Send + Sync + 'static {
    /// Engine name, for the UI (`"mlx"`, `"candle"`).
    fn engine(&self) -> &'static str;
    /// Can this engine run the checkpoint in `dir`? Used to grey out a model
    /// the other app owns rather than letting the user select it and fail.
    fn supports(&self, dir: &std::path::Path) -> bool;
    /// Model ids currently resident in memory.
    fn loaded(&self) -> Vec<String>;
    /// Drop a model's weights. `None` unloads everything.
    fn unload(&self, model_id: Option<&str>);
    /// Start loading a model's weights in the background. Fire-and-forget: the
    /// UI polls the list and watches `loaded` flip, exactly as the old daemon
    /// screen did — a synchronous load here would hold an HTTP worker for the
    /// seconds-to-minutes a big checkpoint takes.
    fn load(&self, model_id: &str);
}

pub fn router<H: EngineHost>(host: Arc<H>) -> Router {
    Router::new()
        .route("/api/local-models", get(list::<H>))
        .route("/api/local-models/settings", get(settings_get).post(settings_put))
        .route("/api/local-models/downloads", get(downloads))
        .route("/api/local-models/:id/download", post(start_download))
        .route("/api/local-models/:id/status", get(download_status))
        .route("/api/local-models/:id/cancel", post(cancel_download))
        .route("/api/local-models/:id", axum::routing::delete(delete_model::<H>))
        .route("/api/local-models/:id/load", post(load::<H>))
        .route("/api/local-models/:id/unload", post(unload::<H>))
        .route("/api/local-models/unload-all", post(unload_all::<H>))
        .with_state(host)
}

/// Model ids travel in the path, where `/` cannot: the UI sends `org__repo` and
/// this turns it back. Accepts the raw form too, for `curl`.
fn id_from_path(raw: &str) -> Result<String, String> {
    let candidate = if raw.contains("__") && !raw.contains('/') {
        store::dirname_to_id(raw).unwrap_or_else(|| raw.to_string())
    } else {
        raw.to_string()
    };
    store::normalize_hf_id(&candidate)
}

fn bad_request(msg: impl Into<String>) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": msg.into() })),
    )
}

async fn list<H: EngineHost>(State(h): State<Arc<H>>) -> Json<serde_json::Value> {
    let loaded = h.loaded();
    let models: Vec<serde_json::Value> = store::list_installed()
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id,
                "sizeBytes": m.size_bytes,
                "architecture": m.architecture,
                "contextLength": m.context_length,
                "supported": h.supports(&m.dir),
                "loaded": loaded.contains(&m.id),
                "dir": m.dir,
            })
        })
        .collect();
    Json(json!({
        "engine": h.engine(),
        "root": store::models_root(),
        "models": models,
    }))
}

async fn settings_get() -> Json<settings::Settings> {
    Json(settings::load(&store::models_root()))
}

async fn settings_put(
    Json(body): Json<settings::Settings>,
) -> Result<Json<settings::Settings>, (StatusCode, Json<serde_json::Value>)> {
    let root = store::models_root();
    settings::save(&root, &body).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
    })?;
    // Read back rather than echo: the reply is then what the next inference call
    // will actually see, including anything the serde defaults filled in.
    Ok(Json(settings::load(&root)))
}

async fn downloads() -> Json<serde_json::Value> {
    Json(json!({ "downloads": download::all() }))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct DownloadBody {
    revision: Option<String>,
}

async fn start_download(
    Path(raw): Path<String>,
    body: Option<Json<DownloadBody>>,
) -> impl IntoResponse {
    let id = match id_from_path(&raw) {
        Ok(id) => id,
        Err(e) => return bad_request(e).into_response(),
    };
    let revision = body
        .and_then(|Json(b)| b.revision)
        .map(|r| r.trim().to_string())
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| "main".to_string());
    let dir = store::model_dir(&id);
    Json(download::start(&id, &revision, dir)).into_response()
}

async fn download_status(Path(raw): Path<String>) -> impl IntoResponse {
    let id = match id_from_path(&raw) {
        Ok(id) => id,
        Err(e) => return bad_request(e).into_response(),
    };
    match download::status(&id) {
        Some(s) => Json(s).into_response(),
        // Never downloaded *this process* — which is not the same as not
        // installed, so report the installed state rather than 404.
        None => Json(json!({
            "modelId": id,
            "status": if store::is_installed(&store::model_dir(&id)) { "done" } else { "idle" },
        }))
        .into_response(),
    }
}

async fn cancel_download(Path(raw): Path<String>) -> impl IntoResponse {
    let id = match id_from_path(&raw) {
        Ok(id) => id,
        Err(e) => return bad_request(e).into_response(),
    };
    Json(json!({ "cancelled": download::cancel(&id) })).into_response()
}

async fn delete_model<H: EngineHost>(
    State(h): State<Arc<H>>,
    Path(raw): Path<String>,
) -> impl IntoResponse {
    let id = match id_from_path(&raw) {
        Ok(id) => id,
        Err(e) => return bad_request(e).into_response(),
    };
    // Stop the download first, then drop the weights: deleting the directory
    // under a running download leaves it recreating the files it is writing.
    download::cancel(&id);
    h.unload(Some(&id));
    match store::remove(&id) {
        Ok(()) => (StatusCode::NO_CONTENT, ()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn unload<H: EngineHost>(State(h): State<Arc<H>>, Path(raw): Path<String>) -> impl IntoResponse {
    match id_from_path(&raw) {
        Ok(id) => {
            h.unload(Some(&id));
            (StatusCode::NO_CONTENT, ()).into_response()
        }
        Err(e) => bad_request(e).into_response(),
    }
}

async fn load<H: EngineHost>(State(h): State<Arc<H>>, Path(raw): Path<String>) -> impl IntoResponse {
    match id_from_path(&raw) {
        Ok(id) => {
            h.load(&id);
            // 202: the load has started, not finished — the caller polls the
            // model list for `loaded`.
            (StatusCode::ACCEPTED, ()).into_response()
        }
        Err(e) => bad_request(e).into_response(),
    }
}

async fn unload_all<H: EngineHost>(State(h): State<Arc<H>>) -> impl IntoResponse {
    h.unload(None);
    (StatusCode::NO_CONTENT, ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ui_form_and_the_curl_form_both_resolve() {
        assert_eq!(
            id_from_path("mlx-community__gemma-4-e2b-it-4bit").unwrap(),
            "mlx-community/gemma-4-e2b-it-4bit"
        );
        assert_eq!(
            id_from_path("mlx-community/gemma-4-e2b-it-4bit").unwrap(),
            "mlx-community/gemma-4-e2b-it-4bit"
        );
    }

    /// The path segment is attacker-controlled in the sense that anything on
    /// loopback can send it, and it ends up as a directory to delete.
    #[test]
    fn a_traversal_in_the_path_is_refused_before_it_becomes_a_directory() {
        for raw in ["..__..", "../..", "..", "x", ""] {
            assert!(id_from_path(raw).is_err(), "`{raw}` must be refused");
        }
    }
}
