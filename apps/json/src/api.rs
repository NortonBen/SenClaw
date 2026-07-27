//! REST API — the single engine behind both the Ant Design UI and the MCP
//! server. The browser posts here instead of converting in JS, so a person and
//! an agent always see the same bytes.
//!
//! Every endpoint answers HTTP 200 with either `{ ok: true, … }` or
//! `{ ok: false, error }`; parse failures additionally carry `line`/`column`.

use axum::{
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::{analyze, codec, convert, fmt};

/// Shared state: only the MCP SSE broadcast channel — every operation is a
/// pure function of its request, so the app keeps no data.
pub struct AppState {
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> Arc<AppState> {
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState { mcp_tx })
}

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    app: &'static str,
    formats: [&'static str; 5],
    codecs: [&'static str; 6],
}

async fn status() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok",
        app: "json",
        formats: convert::FORMATS,
        codecs: codec::CODECS,
    })
}

fn ok(v: Value) -> Json<Value> {
    let mut obj = json!({ "ok": true });
    if let (Some(o), Some(extra)) = (obj.as_object_mut(), v.as_object()) {
        for (k, val) in extra {
            o.insert(k.clone(), val.clone());
        }
    }
    Json(obj)
}

fn err(msg: impl Into<String>) -> Json<Value> {
    Json(json!({ "ok": false, "error": msg.into() }))
}

/// Wrap a `Result<String, String>` as `{ ok, output }` / `{ ok: false, error }`.
fn out(result: Result<String, String>) -> Json<Value> {
    match result {
        Ok(output) => ok(json!({ "output": output })),
        Err(e) => err(e),
    }
}

fn default_indent() -> usize {
    2
}
fn default_root() -> String {
    "root".to_string()
}

#[derive(Deserialize)]
struct FormatRequest {
    input: String,
    /// "pretty" (default) | "minify" | "sort"
    #[serde(default)]
    mode: Option<String>,
    #[serde(default = "default_indent")]
    indent: usize,
}

async fn format(Json(req): Json<FormatRequest>) -> Json<Value> {
    let result = match req.mode.as_deref().unwrap_or("pretty") {
        "minify" => fmt::minify(&req.input),
        "sort" => fmt::sorted(&req.input, req.indent),
        _ => fmt::pretty(&req.input, req.indent),
    };
    match result {
        Ok(output) => ok(json!({ "output": output })),
        Err(e) => Json(json!({
            "ok": false,
            "error": e.to_string(),
            "line": e.line,
            "column": e.column,
        })),
    }
}

#[derive(Deserialize)]
struct InputRequest {
    input: String,
}

async fn validate(Json(req): Json<InputRequest>) -> Json<Value> {
    match fmt::validate(&req.input) {
        Ok(v) => ok(json!({
            "valid": true,
            "type": type_name(&v),
            "bytes": req.input.len(),
        })),
        Err(e) => Json(json!({
            "ok": true,
            "valid": false,
            "error": e.message,
            "line": e.line,
            "column": e.column,
        })),
    }
}

async fn stats(Json(req): Json<InputRequest>) -> Json<Value> {
    match analyze::stats(&req.input) {
        Ok(v) => ok(json!({ "stats": v })),
        Err(e) => err(e),
    }
}

async fn schema(Json(req): Json<InputRequest>) -> Json<Value> {
    match analyze::infer_schema(&req.input) {
        Ok(v) => ok(json!({
            "schema": v,
            "output": serde_json::to_string_pretty(&v).unwrap_or_default(),
        })),
        Err(e) => err(e),
    }
}

pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

#[derive(Deserialize)]
struct ConvertRequest {
    from: String,
    to: String,
    input: String,
    #[serde(default = "default_root")]
    root: String,
    #[serde(default)]
    columns: Vec<String>,
    #[serde(default = "default_indent")]
    indent: usize,
}

async fn convert_route(Json(req): Json<ConvertRequest>) -> Json<Value> {
    match convert::convert(
        &req.from,
        &req.to,
        &req.input,
        &req.root,
        &req.columns,
        req.indent,
    ) {
        Ok(output) => ok(json!({ "output": output, "from": req.from, "to": req.to })),
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
struct DiffRequest {
    left: String,
    right: String,
}

async fn diff_route(Json(req): Json<DiffRequest>) -> Json<Value> {
    match convert::diff(&req.left, &req.right) {
        Ok(v) => ok(v),
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
struct QueryRequest {
    input: String,
    path: String,
}

async fn query_route(Json(req): Json<QueryRequest>) -> Json<Value> {
    match convert::query(&req.input, &req.path) {
        Ok(v) => ok(json!({
            "pointer": convert::to_pointer(&req.path),
            "type": type_name(&v),
            "value": v,
            "output": serde_json::to_string_pretty(&v).unwrap_or_default(),
        })),
        Err(e) => err(e),
    }
}

#[derive(Deserialize)]
struct CodecRequest {
    input: String,
    /// base64 | base64url | hex | url | escape | msgpack (+ jwt on decode)
    format: String,
}

async fn encode_route(Json(req): Json<CodecRequest>) -> Json<Value> {
    out(codec::encode(&req.format, &req.input))
}

async fn decode_route(Json(req): Json<CodecRequest>) -> Json<Value> {
    out(codec::decode(&req.format, &req.input))
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/format", post(format))
        .route("/validate", post(validate))
        .route("/stats", post(stats))
        .route("/schema", post(schema))
        .route("/convert", post(convert_route))
        .route("/diff", post(diff_route))
        .route("/query", post(query_route))
        .route("/encode", post(encode_route))
        .route("/decode", post(decode_route))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}
