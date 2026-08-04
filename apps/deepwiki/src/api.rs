use crate::db::{default_data_dir, Db};
use crate::query;
use axum::{
    extract::{Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::wiki;

pub struct AppState {
    pub db: Arc<Db>,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
    pub watch_tx: tokio::sync::mpsc::Sender<()>,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

pub struct ApiError(pub StatusCode, pub String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}
fn server(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub fn api_router() -> Router {
    let db_path = default_data_dir("deepwiki").join("index.db");
    let db = Arc::new(Db::open(&db_path).expect("open deepwiki index db"));
    wiki::migrate(&db).expect("migrate wiki tables");
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let (watch_tx, watch_rx) = tokio::sync::mpsc::channel::<()>(64);
    crate::watch::spawn_reindexer(db.clone(), watch_rx);

    let state = Arc::new(AppState {
        db,
        mcp_tx,
        watch_tx,
        watcher: Mutex::new(None),
    });

    Router::new()
        // index + wiki
        .route("/status", get(status))
        .route("/recents", get(recents))
        .route("/settings", get(get_settings).post(post_settings))
        .route("/llm-info", get(llm_info))
        .route("/index", post(index))
        .route("/outline", get(outline))
        .route("/context", get(context))
        .route("/ask", post(ask))
        .route("/ask-history", get(ask_history))
        .route(
            "/ask-history/:id",
            get(ask_history_get).delete(ask_history_delete),
        )
        .route("/pages", get(pages))
        .route("/page", get(get_page).post(save_page).delete(delete_page))
        .route("/generate-wiki", post(generate_wiki))
        // code graph
        .route("/search", get(search))
        .route("/symbol", get(symbol))
        .route("/explore", get(explore))
        .route("/investigate", get(investigate_route))
        .route("/file-graph", get(file_graph_route))
        .route("/symbol-graph", get(symbol_graph_route))
        .route("/file", get(file))
        .route("/files", get(files))
        .route("/snippet", get(snippet))
        // mcp
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let stats = query::stats(&s.db).map_err(server)?;
    let root = s.db.get_meta("root").map_err(server)?;
    let pages = wiki::page_count(&s.db).map_err(server)?;
    let exclude = crate::index::load_settings(&s.db).custom_excludes;
    Ok(Json(
        json!({ "root": root, "stats": stats, "pages": pages, "exclude": exclude }),
    ))
}

#[derive(Deserialize)]
struct IndexBody {
    path: String,
    /// Optional extra exclude globs (e.g. "release", "*.test.ts"). Persisted and
    /// reused on auto re-index. Defaults (node_modules, dist, minified, …) always apply.
    #[serde(default)]
    exclude: Option<Vec<String>>,
}

async fn index(
    State(s): State<Arc<AppState>>,
    Json(b): Json<IndexBody>,
) -> Result<Json<Value>, ApiError> {
    let root = PathBuf::from(expand(&b.path));
    if !root.is_dir() {
        return Err(bad(format!("not a directory: {}", root.display())));
    }
    if let Some(ex) = &b.exclude {
        let mut st = crate::index::load_settings(&s.db);
        st.custom_excludes = ex
            .iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let _ = crate::index::save_settings(&s.db, &st);
    }
    let db = s.db.clone();
    let root_clone = root.clone();
    let report = tokio::task::spawn_blocking(move || crate::index::index_repo(&db, &root_clone))
        .await
        .map_err(server)?
        .map_err(server)?;
    crate::watch::install_watcher(&s, &root);
    Ok(Json(serde_json::to_value(report).unwrap_or_default()))
}

async fn recents(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(query::recent_roots(&s.db).map_err(server)?)))
}

async fn get_settings(State(s): State<Arc<AppState>>) -> Json<Value> {
    let st = crate::index::load_settings(&s.db);
    Json(json!({
        "defaultExcludes": st.default_excludes,
        "customExcludes": st.custom_excludes,
        "minifiedMaxLine": st.minified_max_line,
        "factoryExcludes": crate::index::factory_excludes(),
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsBody {
    default_excludes: Option<Vec<String>>,
    custom_excludes: Option<Vec<String>>,
    minified_max_line: Option<usize>,
}

async fn post_settings(
    State(s): State<Arc<AppState>>,
    Json(b): Json<SettingsBody>,
) -> Result<Json<Value>, ApiError> {
    let clean = |v: Vec<String>| -> Vec<String> {
        v.into_iter()
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    };
    let mut st = crate::index::load_settings(&s.db);
    if let Some(d) = b.default_excludes {
        st.default_excludes = clean(d);
    }
    if let Some(c) = b.custom_excludes {
        st.custom_excludes = clean(c);
    }
    if let Some(m) = b.minified_max_line {
        st.minified_max_line = m.max(200);
    }
    crate::index::save_settings(&s.db, &st).map_err(server)?;
    Ok(Json(json!({ "success": true })))
}

/// Report which SenClaw LLM the bridge will use (the active Main model), by
/// querying the daemon's /api/llm-config. Confirms it's a real model, not a mock.
async fn llm_info() -> Json<Value> {
    let base =
        std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".into());
    let url = format!("{}/api/llm-config", base.trim_end_matches('/'));
    let fetch = reqwest::Client::new()
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await;
    match fetch {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                let active = v.get("activeId").and_then(|x| x.as_str()).unwrap_or("");
                let cfg = v.get("configs").and_then(|a| a.as_array()).and_then(|a| {
                    a.iter()
                        .find(|c| c.get("id").and_then(|x| x.as_str()) == Some(active))
                });
                let model = cfg
                    .and_then(|c| c.get("modelName"))
                    .and_then(|x| x.as_str());
                let provider = cfg
                    .and_then(|c| c.get("provider").and_then(|x| x.as_str()))
                    .or_else(|| cfg.and_then(|c| c.get("adapt").and_then(|x| x.as_str())));
                Json(json!({
                    "ok": model.is_some(), "daemon": base, "tier": "main",
                    "model": model, "provider": provider,
                }))
            }
            Err(e) => Json(json!({ "ok": false, "daemon": base, "error": format!("parse: {e}") })),
        },
        Err(e) => Json(
            json!({ "ok": false, "daemon": base, "error": format!("Không kết nối daemon: {e}") }),
        ),
    }
}

async fn outline(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(wiki::outline(&s.db).map_err(server)?))
}

#[derive(Deserialize)]
struct ContextQuery {
    q: String,
    #[serde(default = "default_depth")]
    depth: u32,
}
fn default_depth() -> u32 {
    3
}

async fn context(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ContextQuery>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(wiki::context(&s.db, &q.q, q.depth).map_err(server)?))
}

async fn pages(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(wiki::list_pages(&s.db).map_err(server)?)))
}

/// Generate a small wiki (Overview + Architecture) from the structural outline
/// using SenClaw's LLM, and save the pages. In-app alternative to the
/// `deepwiki-generate` skill.
async fn generate_wiki(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let outline = wiki::outline(&s.db).map_err(server)?;
    if outline["stats"]["symbols"].as_i64().unwrap_or(0) == 0 {
        return Err(bad("Chưa index repo (chưa có symbol nào)."));
    }
    let mut ctx = serde_json::to_string(&outline).unwrap_or_default();
    if ctx.len() > 7000 {
        ctx.truncate(7000);
    }

    let sys = "You are DeepWiki, generating a concise, source-grounded wiki page in Markdown for \
        a codebase. Use ONLY the provided structural outline (directories, largest files, \
        architectural types, hot symbols). Cite file paths. Be skimmable: a short intro, then \
        structured sections. Reply in the same language as the codebase's identifiers/paths \
        (Vietnamese if appropriate). Output ONLY the Markdown body, no preamble.";

    let pages: [(&str, &str, &str, i64); 2] = [
        ("overview", "Tổng quan", "Write the OVERVIEW page: what the project is, its top-level layout, and the key entry points (most-called/hot symbols). Start with an H1 title.", 0),
        ("architecture", "Kiến trúc", "Write the ARCHITECTURE page: the major components/modules (from directories + architectural types) and how they connect. Start with an H1 title.", 1),
    ];

    let mut created: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for (slug, title, task, ord) in pages {
        let user = format!("Repo structural outline (JSON):\n{ctx}\n\nTask: {task}");
        match bridge_llm(sys, &user).await {
            Ok((md, _)) if !md.trim().is_empty() => {
                let page = wiki::PageInput {
                    slug: slug.to_string(),
                    title: title.to_string(),
                    parent: None,
                    content: md,
                    ord,
                };
                if wiki::save_page(&s.db, &page).is_ok() {
                    created.push(slug.to_string());
                }
            }
            Ok(_) => errors.push(format!("{slug}: empty")),
            Err(e) => errors.push(format!("{slug}: {e}")),
        }
    }
    if created.is_empty() {
        return Err(ApiError(
            StatusCode::BAD_GATEWAY,
            format!("Không sinh được trang nào. {}", errors.join("; ")),
        ));
    }
    Ok(Json(json!({ "created": created, "errors": errors })))
}

#[derive(Deserialize)]
struct AskBody {
    q: String,
}

/// "Ask" mode (Devin-style): deeply investigate the question through the call
/// graph (multi-hop, both directions), have SenClaw's LLM synthesize a cited
/// answer, persist it to history, and return the answer + the overview graph.
async fn ask(
    State(s): State<Arc<AppState>>,
    Json(b): Json<AskBody>,
) -> Result<Json<Value>, ApiError> {
    let q = b.q.trim().to_string();
    if q.is_empty() {
        return Err(bad("câu hỏi trống"));
    }
    let inv = query::investigate(&s.db, &q, 2).map_err(server)?;
    if inv.matches.is_empty() {
        return Ok(Json(json!({
            "question": q,
            "answer": "Không tìm thấy symbol nào khớp trong codebase đã index. Hãy thử từ khoá khác hoặc index repo trước.",
            "model": null, "focus": null, "matches": [], "graph": { "nodes": [], "edges": [] },
        })));
    }
    let focus = inv.focus.clone();
    let (system, user) = build_prompt(&s.db, &q, &inv);

    match bridge_llm(&system, &user).await {
        Ok((answer, model)) => {
            let graph = json!({ "nodes": inv.nodes, "edges": inv.edges });
            let matches = serde_json::to_value(&inv.matches).unwrap_or_default();
            let data = json!({ "matches": matches, "graph": graph });
            let id = wiki::save_ask(&s.db, &q, &answer, Some(&model), focus.as_deref(), &data).ok();
            Ok(Json(json!({
                "id": id, "question": q, "answer": answer, "model": model,
                "focus": focus, "matches": matches, "graph": graph,
            })))
        }
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    }
}

async fn ask_history(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(wiki::list_ask(&s.db, 50).map_err(server)?))
}

async fn ask_history_get(
    State(s): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Value>, ApiError> {
    match wiki::get_ask(&s.db, id).map_err(server)? {
        Some(v) => Ok(Json(v)),
        None => Err(ApiError(StatusCode::NOT_FOUND, format!("no ask #{id}"))),
    }
}

async fn ask_history_delete(
    State(s): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Value>, ApiError> {
    wiki::delete_ask(&s.db, id).map_err(server)?;
    Ok(Json(json!({ "success": true })))
}

/// Build (system, user) prompt from the multi-hop investigation.
fn build_prompt(db: &Db, q: &str, inv: &query::Investigation) -> (String, String) {
    use std::collections::BTreeMap;
    let system = "You are DeepWiki, an expert assistant that deeply investigates a codebase \
        (like an autonomous code agent) to answer questions. Use ONLY the evidence below — the \
        call-flow graph and source excerpts. Explain HOW the relevant pieces connect (the flow), \
        cite concrete claims as `path:line`, and give a clear overview. Use Markdown. If evidence \
        is insufficient, say so. Reply in the same language as the question."
        .to_string();

    let mut u = format!("Question: {q}\n");
    if let Some(f) = &inv.focus {
        u.push_str(&format!("Primary symbol: {f}\n"));
    }

    // Investigation graph, grouped by relative depth.
    let mut by_depth: BTreeMap<i64, Vec<&query::GraphNode>> = BTreeMap::new();
    for n in &inv.nodes {
        by_depth.entry(n.depth).or_default().push(n);
    }
    u.push_str("\nCall-flow investigation (multi-hop):\n");
    for (d, ns) in &by_depth {
        let label = match d.cmp(&0) {
            std::cmp::Ordering::Less => format!("callers L{}", -d),
            std::cmp::Ordering::Equal => "focus".to_string(),
            std::cmp::Ordering::Greater => format!("callees L{d}"),
        };
        let names = ns
            .iter()
            .map(|n| format!("{}{}", n.id, if n.external { " (ext)" } else { "" }))
            .collect::<Vec<_>>()
            .join(", ");
        u.push_str(&format!("  [{label}] {names}\n"));
    }
    if !inv.edges.is_empty() {
        u.push_str("\nEdges (caller -> callee):\n");
        for e in inv.edges.iter().take(40) {
            u.push_str(&format!("  {} -> {}\n", e.from, e.to));
        }
    }

    u.push_str("\nKey symbols:\n");
    for m in inv.matches.iter().take(8) {
        u.push_str(&format!(
            "- {} ({}) — {}:{}\n",
            m.name, m.kind, m.path, m.start_line
        ));
        if !m.signature.is_empty() {
            u.push_str(&format!("    sig: {}\n", m.signature));
        }
        if let Some(d) = &m.doc {
            u.push_str(&format!("    doc: {}\n", d));
        }
    }

    u.push_str("\nSource excerpts:\n");
    for m in inv.matches.iter().take(3) {
        if let Ok(src) = query::symbol_source(db, &m.name, 0) {
            if let Some(code) = src.get("code").and_then(|c| c.as_str()) {
                let excerpt = code.lines().take(40).collect::<Vec<_>>().join("\n");
                u.push_str(&format!("\n// {}:{}\n{}\n", m.path, m.start_line, excerpt));
            }
        }
    }
    (system, u)
}

/// Call SenClaw's LLM through the Space-App bridge (`llm.request`).
async fn bridge_llm(system: &str, user: &str) -> Result<(String, String), String> {
    let base =
        std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".into());
    let app_id = std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "deepwiki".into());
    let url = format!(
        "{}/api/space/apps/{}/bridge",
        base.trim_end_matches('/'),
        app_id
    );
    let body = json!({
        "action": "llm.request",
        "payload": { "system": system, "prompt": user, "maxTokens": 1200 },
    });
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .timeout(Duration::from_secs(125))
        .send()
        .await
        .map_err(|e| format!("Không gọi được SenClaw LLM ({url}): {e}"))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("Phản hồi LLM không hợp lệ: {e}"))?;
    match v.get("status").and_then(|x| x.as_str()) {
        Some("ok") => Ok((
            v.get("text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            v.get("model")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        )),
        Some("pending") => Err("SenClaw bridge LLM chưa được bật trong daemon này.".into()),
        _ => Err(v
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("LLM lỗi không rõ")
            .to_string()),
    }
}

#[derive(Deserialize)]
struct SlugQuery {
    slug: String,
}

async fn get_page(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SlugQuery>,
) -> Result<Json<Value>, ApiError> {
    match wiki::get_page(&s.db, &q.slug).map_err(server)? {
        Some(p) => Ok(Json(json!(p))),
        None => Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("no page: {}", q.slug),
        )),
    }
}

async fn save_page(
    State(s): State<Arc<AppState>>,
    Json(p): Json<wiki::PageInput>,
) -> Result<Json<Value>, ApiError> {
    wiki::save_page(&s.db, &p).map_err(server)?;
    Ok(Json(json!({ "success": true, "slug": p.slug })))
}

async fn delete_page(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SlugQuery>,
) -> Result<Json<Value>, ApiError> {
    wiki::delete_page(&s.db, &q.slug).map_err(server)?;
    Ok(Json(json!({ "success": true })))
}

// ===== Code-graph endpoints =====

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    #[serde(default = "default_limit")]
    limit: u32,
    /// Optional repo-relative folder prefix to scope the search to.
    #[serde(default)]
    path: Option<String>,
}
fn default_limit() -> u32 {
    30
}

async fn search(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let prefix = q
        .path
        .as_deref()
        .map(|p| p.trim_end_matches('/').to_string());
    let want = if prefix.is_some() {
        q.limit * 6
    } else {
        q.limit
    };
    let mut rows = query::search(&s.db, &q.q, want).map_err(server)?;
    if let Some(p) = prefix.filter(|p| !p.is_empty()) {
        rows.retain(|sym| sym.path == p || sym.path.starts_with(&format!("{p}/")));
        rows.truncate(q.limit as usize);
    }
    Ok(Json(json!(rows)))
}

#[derive(Deserialize)]
struct NameQuery {
    name: String,
}

async fn symbol(
    State(s): State<Arc<AppState>>,
    Query(q): Query<NameQuery>,
) -> Result<Json<Value>, ApiError> {
    let defs = query::symbols_by_name(&s.db, &q.name).map_err(server)?;
    let callers = query::callers(&s.db, &q.name, 100).map_err(server)?;
    let callees = query::callees(&s.db, &q.name, 100).map_err(server)?;
    Ok(Json(
        json!({ "name": q.name, "definitions": defs, "callers": callers, "callees": callees }),
    ))
}

async fn explore(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ContextQuery>,
) -> Result<Json<Value>, ApiError> {
    let ex = query::explore(&s.db, &q.q, q.depth).map_err(server)?;
    Ok(Json(serde_json::to_value(ex).unwrap_or_default()))
}

/// Multi-hop investigation subgraph (Devin-style) for the Graph tab.
async fn investigate_route(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ContextQuery>,
) -> Result<Json<Value>, ApiError> {
    let inv = query::investigate(&s.db, &q.q, q.depth).map_err(server)?;
    Ok(Json(serde_json::to_value(inv).unwrap_or_default()))
}

/// Whole-codebase file dependency graph (files = nodes, cross-file calls = edges).
async fn file_graph_route(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let g = query::file_graph(&s.db).map_err(server)?;
    Ok(Json(serde_json::to_value(g).unwrap_or_default()))
}

/// Whole-codebase function call graph (functions = nodes, calls = edges).
async fn symbol_graph_route(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let g = query::symbol_graph(&s.db).map_err(server)?;
    Ok(Json(serde_json::to_value(g).unwrap_or_default()))
}

#[derive(Deserialize)]
struct PathQuery {
    path: String,
}

async fn file(
    State(s): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, ApiError> {
    let outline = query::file_outline(&s.db, &q.path).map_err(server)?;
    let imports = query::imports_of_file(&s.db, &q.path).map_err(server)?;
    Ok(Json(
        json!({ "path": q.path, "outline": outline, "imports": imports }),
    ))
}

async fn files(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(query::list_files(&s.db).map_err(server)?)))
}

#[derive(Deserialize)]
struct SnippetQuery {
    name: Option<String>,
    path: Option<String>,
    start: Option<i64>,
    end: Option<i64>,
    #[serde(default = "default_ctx")]
    context: i64,
}
fn default_ctx() -> i64 {
    2
}

async fn snippet(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SnippetQuery>,
) -> Result<Json<Value>, ApiError> {
    let v = if let Some(name) = q.name.filter(|s| !s.is_empty()) {
        query::symbol_source(&s.db, &name, q.context).map_err(server)?
    } else if let Some(path) = q.path.filter(|s| !s.is_empty()) {
        let start = q.start.unwrap_or(1);
        let end = q.end.unwrap_or(start + 40);
        query::snippet(&s.db, &path, start, end, q.context).map_err(server)?
    } else {
        return Err(bad("provide either name or path+start/end"));
    };
    Ok(Json(v))
}

pub fn expand(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}/{}", home.to_string_lossy(), rest);
        }
    }
    path.to_string()
}
