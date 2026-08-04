//! Code executor REPL API.
//!
//! Endpoint:
//!   POST /api/code/run — { code, timeout_ms?, memory_mb?, language? }
//!                        → an outcome with the common shape
//!                          { ok, result, result_type, logs, error, timed_out, duration_ms }
//!
//! Languages:
//!   - `javascript`/`js` (default) and `typescript`/`ts` run in the **sandboxed**
//!     QuickJS engine (see [`crate::mcp::js_server`]) — no host access.
//!   - `bash`/`sh` runs in the **brush** sandbox (pure-Rust shell) — see
//!     [`super::bash_sandbox`]: no env inheritance, empty PATH (external
//!     programs by name blocked), temp cwd, timeout + output cap.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
};
use serde::Deserialize;

use super::core::{AppError, UiState};

/// Sandbox defaults match the `senclaw-js` MCP server. The sandbox clamps to
/// its own hard ceilings (60 s / 1 GiB) regardless of what's requested here.
const DEFAULT_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_MEMORY_MB: u64 = 128;

#[derive(Deserialize)]
pub(crate) struct CodeRunBody {
    code: String,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    memory_mb: Option<u64>,
    /// Language: `javascript`/`js` (default), `typescript`/`ts`, or `bash`/`sh`.
    #[serde(default)]
    language: Option<String>,
}

enum Lang {
    Js,
    Ts,
    Bash,
}

fn parse_lang(language: Option<&str>) -> Result<Lang, String> {
    match language.map(|l| l.trim().to_lowercase()) {
        None => Ok(Lang::Js),
        Some(l) if l.is_empty() || l == "javascript" || l == "js" => Ok(Lang::Js),
        Some(l) if l == "typescript" || l == "ts" => Ok(Lang::Ts),
        Some(l) if l == "bash" || l == "sh" || l == "shell" => Ok(Lang::Bash),
        Some(l) => Err(format!(
            "language `{l}` is not supported yet — JavaScript, TypeScript, and Bash are live"
        )),
    }
}

/// Run `code` in the appropriate sandbox and return the common outcome JSON.
/// Shared by the REPL endpoint and the artifact-run endpoint.
pub(super) async fn run_code(
    language: Option<&str>,
    code: String,
    timeout_ms: u64,
    memory_mb: u64,
) -> Result<serde_json::Value, String> {
    let lang = parse_lang(language)?;
    Ok(match lang {
        Lang::Bash => super::bash_sandbox::run(code, timeout_ms).await,
        Lang::Js | Lang::Ts => {
            let json = match lang {
                Lang::Ts => {
                    crate::mcp::js_server::eval_ts_to_json(code, timeout_ms, memory_mb).await
                }
                _ => crate::mcp::js_server::eval_to_json(code, timeout_ms, memory_mb).await,
            };
            // eval_*_to_json returns a JSON-encoded outcome; forward it verbatim.
            serde_json::from_str(&json).unwrap_or_else(|_| {
                serde_json::json!({ "ok": false, "error": "internal: malformed sandbox output" })
            })
        }
    })
}

pub(crate) async fn code_run(
    State(_state): State<Arc<UiState>>,
    Json(body): Json<CodeRunBody>,
) -> Result<impl IntoResponse, AppError> {
    if body.code.trim().is_empty() {
        return Err(AppError(StatusCode::BAD_REQUEST, "code is empty".into()));
    }
    let timeout = body.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    let memory = body.memory_mb.unwrap_or(DEFAULT_MEMORY_MB);
    let value = run_code(body.language.as_deref(), body.code, timeout, memory)
        .await
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(value))
}
