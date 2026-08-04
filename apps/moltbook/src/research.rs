//! Research workflows: before the molty comments or posts, it can run one or
//! more user-defined **workflows** — sequences of MCP-tool steps — to gather
//! context, then an LLM pass synthesises the raw outputs into findings +
//! key facts + **open questions**. When the synthesis is not confident enough,
//! the draft is parked as `needs_input` with those questions for the human
//! instead of being published.
//!
//! Three step kinds:
//!   * `builtin` — implemented natively (molty memory, wiki, Moltbook reads).
//!   * `app`     — another Space App's MCP, called via `POST {origin}/api/mcp/message`
//!                 (the deterministic app→app path; same trick as apps/search).
//!   * `daemon`  — an MCP server registered on the SenClaw daemon, called via
//!                 `POST /api/mcp-servers/:name/test`.
//!
//! Workflows matching the flow (`comment` / `post` / `both`) run **in parallel**;
//! the steps inside one workflow run sequentially and can chain outputs through
//! `save_as` + `{{placeholders}}`.

use crate::db::Db;
use crate::llm::{self, truncate};
use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::time::Duration;

/// Hard caps so a runaway workflow can't hammer the daemon or blow the prompt.
const MAX_WORKFLOWS_PER_RUN: usize = 6;
const MAX_STEPS_PER_WORKFLOW: usize = 8;
/// Per-step output kept for synthesis (chars).
const STEP_OUTPUT_CAP: usize = 3500;
/// Per-step call timeout.
const STEP_TIMEOUT: Duration = Duration::from_secs(45);
/// Everything handed to the synthesis prompt is bounded by this (chars).
const SYNTHESIS_INPUT_CAP: usize = 14_000;

// ---------------------------------------------------------------- model

/// One step of a workflow, as stored in `workflows.steps` (JSON array).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Step {
    /// "builtin" | "app" | "daemon"
    #[serde(default)]
    pub kind: String,
    /// Tool name (builtin name, app tool, or daemon-server tool).
    #[serde(default)]
    pub tool: String,
    /// Space-App id (kind = "app").
    #[serde(default)]
    pub app: String,
    /// Daemon MCP server name (kind = "daemon").
    #[serde(default)]
    pub server: String,
    /// Tool arguments; string values may contain `{{placeholders}}`.
    #[serde(default)]
    pub args: Value,
    /// Save this step's output under this name for later `{{name}}` use.
    #[serde(default)]
    pub save_as: String,
}

impl Step {
    /// Human-readable label for logs/UI.
    pub fn label(&self) -> String {
        match self.kind.as_str() {
            "app" => format!("app:{}/{}", self.app, self.tool),
            "daemon" => format!("mcp:{}/{}", self.server, self.tool),
            _ => format!("builtin:{}", self.tool),
        }
    }
}

/// What the research is about.
#[derive(Debug, Clone, Default)]
pub struct ResearchInput {
    /// "comment" | "post"
    pub flow: String,
    /// The subject to research (post title / idea text).
    pub topic: String,
    /// Target post title (comment flow) or draft title (post flow).
    pub title: String,
    /// Target post content / draft body.
    pub content: String,
    /// Target post id (comment flow), for thread-reading steps.
    pub post_id: String,
}

/// The record of one executed step.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StepRun {
    pub workflow: String,
    pub step: String,
    pub ok: bool,
    /// Truncated output (or error text when `ok == false`).
    pub output: String,
    pub ms: u64,
}

/// The synthesised result handed to the composers and stored on the draft.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResearchBundle {
    pub flow: String,
    pub topic: String,
    /// Markdown synthesis of everything the tools returned.
    pub findings: String,
    pub key_facts: Vec<String>,
    /// Questions for the human — what the tools could NOT establish.
    pub open_questions: Vec<String>,
    /// 0-100: how sufficient the research is to write confidently.
    pub confidence: i64,
    /// Workflow names that ran.
    pub workflows: Vec<String>,
    /// Per-step trail (for the UI).
    pub runs: Vec<StepRun>,
    pub model: String,
}

impl ResearchBundle {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    pub fn from_json(v: &Value) -> Option<Self> {
        serde_json::from_value(v.clone()).ok()
    }

    /// Render as a prompt section for the composers. Empty when nothing useful.
    pub fn render(&self) -> String {
        if self.findings.trim().is_empty() && self.key_facts.is_empty() {
            return String::new();
        }
        let mut s = format!(
            "\nNGHIÊN CỨU ({} bước công cụ · độ tin cậy {}%):\n{}\n",
            self.runs.iter().filter(|r| r.ok).count(),
            self.confidence,
            truncate(self.findings.trim(), 2200)
        );
        if !self.key_facts.is_empty() {
            s.push_str("Dữ kiện chính:\n");
            for f in self.key_facts.iter().take(8) {
                s.push_str(&format!("- {}\n", truncate(f, 300)));
            }
        }
        s
    }

    /// Sources line for the draft reason / UI.
    pub fn sources_line(&self) -> String {
        let ok: Vec<String> = self
            .runs
            .iter()
            .filter(|r| r.ok)
            .map(|r| r.step.clone())
            .collect();
        ok.join(" · ")
    }
}

// ---------------------------------------------------------------- catalog

/// One callable tool, as shown in the workflow-builder UI and to the AI builder.
#[derive(Debug, Clone, Serialize)]
pub struct CatalogTool {
    pub kind: String,
    /// App id or daemon server name ("" for builtin).
    pub target: String,
    pub tool: String,
    pub description: String,
    /// Arg hints (JSON object of name → hint).
    pub args: Value,
}

/// Builtin steps: always available, no external server needed.
pub fn builtin_tools() -> Vec<CatalogTool> {
    let t = |tool: &str, description: &str, args: Value| CatalogTool {
        kind: "builtin".into(),
        target: String::new(),
        tool: tool.into(),
        description: description.into(),
        args,
    };
    vec![
        t(
            "knowledge_recall",
            "Trí nhớ của molty: câu trả lời tổng hợp từ những gì nó đã đăng/học.",
            json!({ "query": "{{topic}}" }),
        ),
        t(
            "knowledge_search",
            "Trí nhớ của molty: các mẩu ký ức thô khớp truy vấn.",
            json!({ "query": "{{topic}}", "limit": 6 }),
        ),
        t(
            "wiki_search",
            "Kho thông tin (wiki của Sếp): tìm tài liệu liên quan.",
            json!({ "query": "{{topic}}", "limit": 5 }),
        ),
        t(
            "wiki_read",
            "Đọc một tài liệu wiki theo path (thường chain sau wiki_search).",
            json!({ "path": "{{doc_path}}" }),
        ),
        t(
            "wiki_context",
            "Tìm + trích đoạn tài liệu wiki tốt nhất cho chủ đề (một bước gọn).",
            json!({ "query": "{{topic}}", "max_chars": 2000 }),
        ),
        t(
            "moltbook_search",
            "Tìm trên Moltbook: các molty khác đã bàn gì về chủ đề này.",
            json!({ "q": "{{topic}}", "type": "all", "limit": 8 }),
        ),
        t(
            "moltbook_get_post",
            "Đọc một bài Moltbook + toàn bộ thảo luận (dùng cho luồng bình luận).",
            json!({ "post_id": "{{post_id}}" }),
        ),
        t(
            "moltbook_feed",
            "Feed Moltbook hiện tại (hot/new/top) — bối cảnh cộng đồng.",
            json!({ "sort": "hot", "limit": 10 }),
        ),
    ]
}

/// Discover Space Apps that expose an MCP endpoint (via the daemon registry).
/// Best-effort: an unreachable daemon just yields an empty list.
async fn discover_apps() -> Vec<(String, String, String, Option<String>)> {
    // (app_id, name, rpc_url, mcp_name)
    let url = format!(
        "{}/api/space/apps",
        llm::base_url().trim_end_matches('/')
    );
    let Ok(resp) = llm::http()
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(body) = resp.json::<Value>().await else {
        return Vec::new();
    };
    let list = body
        .get("apps")
        .and_then(Value::as_array)
        .or_else(|| body.as_array())
        .cloned()
        .unwrap_or_default();
    let me = llm::app_id();
    let mut out = Vec::new();
    for app in list {
        let manifest = app.get("manifest").unwrap_or(&app);
        let id = manifest
            .get("id")
            .or_else(|| app.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if id.is_empty() || id == me {
            continue; // never research through ourselves (recursion)
        }
        if app.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let Some(mcp) = manifest.get("mcp").filter(|m| m.is_object()) else {
            continue;
        };
        let runtime = manifest.get("runtime").cloned().unwrap_or(Value::Null);
        let origin = runtime
            .get("url")
            .and_then(Value::as_str)
            .map(|s| s.trim_end_matches('/').to_string())
            .or_else(|| {
                runtime
                    .get("port")
                    .and_then(Value::as_u64)
                    .filter(|p| *p > 0)
                    .map(|p| format!("http://127.0.0.1:{p}"))
            });
        let Some(origin) = origin else { continue };
        let path = mcp
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("/api/mcp/sse");
        let rpc_path = if path.ends_with("/sse") {
            path.replace("/sse", "/message")
        } else {
            path.to_string()
        };
        let name = manifest
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&id)
            .to_string();
        let mcp_name = mcp.get("name").and_then(Value::as_str).map(String::from);
        out.push((id, name, format!("{origin}{rpc_path}"), mcp_name));
    }
    out
}

/// JSON-RPC `tools/list` against a Space-App MCP endpoint.
async fn app_tools_list(rpc_url: &str) -> Vec<Value> {
    let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
    let Ok(resp) = llm::http()
        .post(rpc_url)
        .timeout(Duration::from_secs(6))
        .json(&req)
        .send()
        .await
    else {
        return Vec::new();
    };
    let Ok(v) = resp.json::<Value>().await else {
        return Vec::new();
    };
    v.get("result")
        .and_then(|r| r.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// The full tool catalog: builtin + Space Apps + daemon MCP servers.
/// `{ builtin: [...], apps: [{id,name,tools:[...]}], daemon: [{name,tools}] }`
pub async fn catalog() -> Value {
    let builtin: Vec<Value> = builtin_tools()
        .iter()
        .map(|t| {
            json!({ "kind": "builtin", "tool": t.tool, "description": t.description, "args": t.args })
        })
        .collect();

    // Space Apps (each tools/list in parallel).
    let apps = discover_apps().await;
    let tool_lists = join_all(apps.iter().map(|(_, _, rpc, _)| app_tools_list(rpc))).await;
    let apps_json: Vec<Value> = apps
        .iter()
        .zip(tool_lists)
        .map(|((id, name, _, mcp_name), tools)| {
            let tools: Vec<Value> = tools
                .iter()
                .take(60)
                .map(|t| {
                    json!({
                        "tool": t.get("name").and_then(Value::as_str).unwrap_or(""),
                        "description": truncate(t.get("description").and_then(Value::as_str).unwrap_or(""), 220),
                        "args": t.get("inputSchema").and_then(|s| s.get("properties")).cloned().unwrap_or(json!({})),
                    })
                })
                .collect();
            json!({ "id": id, "name": name, "mcp_name": mcp_name, "tools": tools })
        })
        .filter(|a| {
            a.get("tools")
                .and_then(Value::as_array)
                .map(|t| !t.is_empty())
                .unwrap_or(false)
        })
        .collect();

    // Daemon-registered MCP servers.
    let mut daemon_json: Vec<Value> = Vec::new();
    let url = format!(
        "{}/api/mcp-servers",
        llm::base_url().trim_end_matches('/')
    );
    if let Ok(resp) = llm::http()
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await
    {
        if let Ok(v) = resp.json::<Value>().await {
            for s in v
                .get("servers")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
            {
                let name = s.get("name").and_then(Value::as_str).unwrap_or("");
                if name.is_empty() {
                    continue;
                }
                let tools: Vec<Value> = s
                    .get("tools")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .take(60)
                            .map(|t| match t {
                                Value::String(n) => json!({ "tool": n, "description": "" }),
                                o => json!({
                                    "tool": o.get("name").and_then(Value::as_str).unwrap_or(""),
                                    "description": truncate(o.get("description").and_then(Value::as_str).unwrap_or(""), 220),
                                }),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                daemon_json.push(json!({
                    "name": name,
                    "builtin": s.get("builtin").and_then(Value::as_bool).unwrap_or(false),
                    "description": truncate(s.get("description").and_then(Value::as_str).unwrap_or(""), 220),
                    "tools": tools,
                }));
            }
        }
    }

    json!({ "builtin": builtin, "apps": apps_json, "daemon": daemon_json })
}

// ---------------------------------------------------------------- execution

/// Substitute `{{key}}` placeholders inside every string of `v`.
fn substitute(v: &Value, vars: &HashMap<String, String>) -> Value {
    match v {
        Value::String(s) => {
            let mut out = s.clone();
            for (k, val) in vars {
                let pat = format!("{{{{{k}}}}}");
                if out.contains(&pat) {
                    out = out.replace(&pat, val);
                }
            }
            Value::String(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, vars)).collect()),
        Value::Object(o) => {
            let mut m = Map::new();
            for (k, x) in o {
                m.insert(k.clone(), substitute(x, vars));
            }
            Value::Object(m)
        }
        other => other.clone(),
    }
}

/// Compact a tool result into a bounded string for the synthesis prompt.
fn compact(v: &Value) -> String {
    let s = match v {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    truncate(s.trim(), STEP_OUTPUT_CAP)
}

/// Execute one builtin step.
async fn run_builtin(db: &Db, tool: &str, args: &Value) -> Result<Value, String> {
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let n = |k: &str, d: i64| args.get(k).and_then(Value::as_i64).unwrap_or(d);
    match tool {
        "knowledge_recall" => {
            let space = crate::api::memory_space(db);
            let answer = crate::senclaw::knowledge_recall(&space, &s("query")).await?;
            Ok(json!({ "answer": answer }))
        }
        "knowledge_search" => {
            let space = crate::api::memory_space(db);
            let hits =
                crate::senclaw::knowledge_search(&space, &s("query"), n("limit", 6) as u32).await?;
            Ok(json!(hits
                .iter()
                .map(|(name, summary, score)| json!({ "name": name, "summary": summary, "score": score }))
                .collect::<Vec<_>>()))
        }
        "wiki_search" => {
            let hits = crate::senclaw::wiki_search(&s("query"), n("limit", 5) as usize).await?;
            Ok(json!(hits
                .iter()
                .map(|(path, title, snippet)| json!({ "path": path, "title": title, "snippet": snippet }))
                .collect::<Vec<_>>()))
        }
        "wiki_read" => {
            let path = s("path");
            if path.trim().is_empty() {
                return Err("path trống (chain sau wiki_search với save_as)".into());
            }
            crate::senclaw::wiki_read(&path).await.map(Value::String)
        }
        "wiki_context" => {
            let q = s("query");
            let max = n("max_chars", 2000).clamp(200, 6000) as usize;
            Ok(Value::String(crate::senclaw::wiki_context(&q, max).await))
        }
        "moltbook_search" => {
            let client = crate::api::client(db);
            if !client.is_authenticated() {
                return Err("chưa kết nối agent Moltbook".into());
            }
            let kind = {
                let k = s("type");
                if k.is_empty() {
                    "all".into()
                } else {
                    k
                }
            };
            client
                .search(&s("q"), &kind, n("limit", 8))
                .await
                .map_err(|e| e.to_string())
        }
        "moltbook_get_post" => {
            let client = crate::api::client(db);
            if !client.is_authenticated() {
                return Err("chưa kết nối agent Moltbook".into());
            }
            let pid = s("post_id");
            if pid.trim().is_empty() {
                return Err("post_id trống".into());
            }
            let post = client.get_post(&pid).await.map_err(|e| e.to_string())?;
            let comments = client
                .comments(&pid, "best", None)
                .await
                .unwrap_or(json!({}));
            Ok(json!({ "post": post, "comments": comments }))
        }
        "moltbook_feed" => {
            let client = crate::api::client(db);
            if !client.is_authenticated() {
                return Err("chưa kết nối agent Moltbook".into());
            }
            let sort = {
                let x = s("sort");
                if x.is_empty() {
                    "hot".into()
                } else {
                    x
                }
            };
            let v = client
                .posts(&sort, None, None)
                .await
                .map_err(|e| e.to_string())?;
            let items = crate::engine::extract_posts(&v);
            let lim = n("limit", 10).clamp(1, 30) as usize;
            Ok(json!(items
                .iter()
                .take(lim)
                .map(|p| json!({ "id": p.id, "submolt": p.submolt, "author": p.author, "title": p.title, "content": truncate(&p.content, 300), "score": p.score }))
                .collect::<Vec<_>>()))
        }
        other => Err(format!("builtin không tồn tại: {other}")),
    }
}

/// Execute an `app` step: JSON-RPC `tools/call` on the peer app's MCP endpoint.
async fn run_app_tool(app_id: &str, tool: &str, args: &Value) -> Result<Value, String> {
    let apps = discover_apps().await;
    let Some((_, _, rpc_url, _)) = apps.iter().find(|(id, _, _, _)| id == app_id) else {
        return Err(format!("app '{app_id}' không chạy hoặc không có MCP"));
    };
    let req = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": tool, "arguments": args },
    });
    let resp = llm::http()
        .post(rpc_url)
        .timeout(STEP_TIMEOUT)
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("{tool}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{tool}: HTTP {}", resp.status()));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("{tool}: phản hồi không phải JSON-RPC: {e}"))?;
    if let Some(err) = v.get("error").filter(|e| !e.is_null()) {
        return Err(format!("{tool}: {err}"));
    }
    let result = v.get("result").cloned().unwrap_or(Value::Null);
    // Unwrap the MCP content envelope when present.
    let text = result
        .get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        return Err(format!("{tool}: {}", truncate(&text, 300)));
    }
    if text.is_empty() {
        return Ok(result);
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}

/// Execute a `daemon` step through the daemon's MCP test endpoint.
async fn run_daemon_tool(server: &str, tool: &str, args: &Value) -> Result<Value, String> {
    let url = format!(
        "{}/api/mcp-servers/{}/test",
        llm::base_url().trim_end_matches('/'),
        server
    );
    let resp = llm::http()
        .post(&url)
        .timeout(STEP_TIMEOUT)
        .json(&json!({ "tool": tool, "args": args }))
        .send()
        .await
        .map_err(|e| format!("{server}/{tool}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{server}/{tool}: HTTP {}", resp.status()));
    }
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("{server}/{tool}: {e}"))?;
    if v.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    } else {
        Err(v
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("daemon MCP call failed")
            .to_string())
    }
}

/// Parse a workflow's steps JSON (bounded).
pub fn parse_steps(steps_json: &str) -> Vec<Step> {
    serde_json::from_str::<Vec<Step>>(steps_json)
        .unwrap_or_default()
        .into_iter()
        .take(MAX_STEPS_PER_WORKFLOW)
        .collect()
}

/// Placeholders referenced by `v` whose value in `vars` is missing or empty —
/// e.g. `{{post_id}}` when researching a NEW post (no target). Such a step is
/// inapplicable to this run and should be skipped, not surfaced as an error.
fn missing_placeholders(v: &Value, vars: &HashMap<String, String>) -> Vec<String> {
    let mut out = Vec::new();
    match v {
        Value::String(s) => {
            let mut rest = s.as_str();
            while let Some(start) = rest.find("{{") {
                let Some(end) = rest[start + 2..].find("}}") else {
                    break;
                };
                let key = rest[start + 2..start + 2 + end].trim().to_string();
                if vars.get(&key).map(|v| v.trim().is_empty()).unwrap_or(true)
                    && !out.contains(&key)
                {
                    out.push(key);
                }
                rest = &rest[start + 2 + end + 2..];
            }
        }
        Value::Array(a) => {
            for x in a {
                for k in missing_placeholders(x, vars) {
                    if !out.contains(&k) {
                        out.push(k);
                    }
                }
            }
        }
        Value::Object(o) => {
            for x in o.values() {
                for k in missing_placeholders(x, vars) {
                    if !out.contains(&k) {
                        out.push(k);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// Run ONE workflow's steps sequentially, chaining outputs via `save_as`.
async fn run_workflow(
    db: &Db,
    wf_name: &str,
    steps: &[Step],
    base_vars: &HashMap<String, String>,
) -> Vec<StepRun> {
    let mut vars = base_vars.clone();
    let mut runs = Vec::new();
    for step in steps {
        let started = std::time::Instant::now();
        // A step whose args reference a placeholder with no value here (e.g.
        // {{post_id}} while researching a brand-new post) doesn't apply to this
        // run — skip it quietly instead of calling the tool with a blank.
        let missing = missing_placeholders(&step.args, &vars);
        if !missing.is_empty() {
            runs.push(StepRun {
                workflow: wf_name.to_string(),
                step: step.label(),
                ok: false,
                output: format!(
                    "bỏ qua — không áp dụng cho lượt này (thiếu {})",
                    missing
                        .iter()
                        .map(|k| format!("{{{{{k}}}}}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                ms: 0,
            });
            continue;
        }
        let args = substitute(&step.args, &vars);
        let result = match step.kind.as_str() {
            "app" => run_app_tool(&step.app, &step.tool, &args).await,
            "daemon" => run_daemon_tool(&step.server, &step.tool, &args).await,
            _ => run_builtin(db, &step.tool, &args).await,
        };
        let ms = started.elapsed().as_millis() as u64;
        match result {
            Ok(v) => {
                let out = compact(&v);
                if !step.save_as.trim().is_empty() {
                    vars.insert(step.save_as.trim().to_string(), truncate(&out, 1500));
                }
                // A special convenience: the first wiki_search hit's path.
                if step.tool == "wiki_search" {
                    if let Some(p) = v
                        .as_array()
                        .and_then(|a| a.first())
                        .and_then(|h| h.get("path"))
                        .and_then(Value::as_str)
                    {
                        vars.entry("doc_path".into()).or_insert_with(|| p.to_string());
                    }
                }
                runs.push(StepRun {
                    workflow: wf_name.to_string(),
                    step: step.label(),
                    ok: true,
                    output: out,
                    ms,
                });
            }
            Err(e) => {
                // Best-effort: record and continue with the remaining steps.
                runs.push(StepRun {
                    workflow: wf_name.to_string(),
                    step: step.label(),
                    ok: false,
                    output: truncate(&e, 400),
                    ms,
                });
            }
        }
    }
    runs
}

// ---------------------------------------------------------------- synthesis

const SYNTH_SYSTEM: &str = "You are the research analyst for an AI agent about to write on \
Moltbook (the social network for AI agents). You are given the subject, what the agent is about to \
do (comment on a post / write a post), and RAW OUTPUTS from research tools (its own memory, the \
human's wiki, Moltbook search, other apps' data, web search...). Synthesise them:\n\
- findings: a compact markdown synthesis (<= 250 words) of what the tools actually established that \
is RELEVANT to writing this comment/post. Merge duplicates, resolve conflicts, note disagreements.\n\
- key_facts: the concrete facts/claims worth citing, each self-contained (max 8).\n\
- open_questions: things that materially affect what to write but the tools could NOT establish, or \
where sources conflict — phrase each as a direct question to the human (max 4). Empty if none.\n\
- confidence: 0-100, how sufficient this research is to write confidently. Below ~60 means the \
human should be asked first.\n\
Ground EVERYTHING in the tool outputs — never invent. Write findings/key_facts/open_questions in \
the SAME language as the subject (Vietnamese subject → Vietnamese output).\n\
Return ONLY valid JSON (no prose, no fences):\n\
{\"findings\":\"...\",\"key_facts\":[\"...\"],\"open_questions\":[\"...\"],\"confidence\":72}";

#[derive(Deserialize, Default)]
struct RawSynthesis {
    #[serde(default)]
    findings: String,
    #[serde(default)]
    key_facts: Vec<String>,
    #[serde(default)]
    open_questions: Vec<String>,
    #[serde(default)]
    confidence: i64,
}

/// Synthesise the step outputs into a bundle. `extract_prompts` are the user's
/// extra extraction instructions (global setting + per-workflow prompts).
async fn synthesize(
    input: &ResearchInput,
    runs: &[StepRun],
    workflows: Vec<String>,
    extract_prompts: &[String],
) -> Result<ResearchBundle, String> {
    let mut prompt = format!(
        "Việc sắp làm: {}\nChủ đề: {}\n",
        if input.flow == "post" {
            "viết một BÀI MỚI lên Moltbook"
        } else {
            "BÌNH LUẬN vào một bài trên Moltbook"
        },
        input.topic
    );
    if !input.title.trim().is_empty() {
        prompt.push_str(&format!("Bài liên quan: {}\n", truncate(&input.title, 200)));
    }
    if !input.content.trim().is_empty() {
        prompt.push_str(&format!(
            "Nội dung bài: {}\n",
            truncate(&input.content, 600)
        ));
    }
    let extras: Vec<&String> = extract_prompts
        .iter()
        .filter(|p| !p.trim().is_empty())
        .collect();
    if !extras.is_empty() {
        prompt.push_str("\nYÊU CẦU TRÍCH XUẤT THÊM từ người dùng (tuân thủ khi tổng hợp):\n");
        for p in extras {
            prompt.push_str(&format!("- {}\n", truncate(p.trim(), 300)));
        }
    }
    prompt.push_str("\nKẾT QUẢ CÔNG CỤ:\n");
    let mut used = prompt.chars().count();
    for r in runs {
        let block = if r.ok {
            format!("\n--- [{} · {}] ---\n{}\n", r.workflow, r.step, r.output)
        } else {
            format!("\n--- [{} · {}] LỖI: {} ---\n", r.workflow, r.step, r.output)
        };
        let len = block.chars().count();
        if used + len > SYNTHESIS_INPUT_CAP {
            prompt.push_str("\n(… các kết quả sau bị cắt vì quá dài …)\n");
            break;
        }
        used += len;
        prompt.push_str(&block);
    }
    prompt.push_str("\nTrả JSON tổng hợp ngay.");

    // One retry on parse failure / truncation, same pattern as the planner.
    let mut last_err = String::new();
    let mut out: Option<(RawSynthesis, String)> = None;
    for attempt in 0..2u8 {
        let p = if attempt == 0 {
            prompt.clone()
        } else {
            format!("{prompt}\n\nLƯU Ý: lần trước {last_err}. Trả JSON NGẮN GỌN hơn (findings ≤ 150 từ).")
        };
        let (text, model, finish) = llm::bridge_llm(SYNTH_SYSTEM, &p, 2200).await?;
        match llm::parse_json::<RawSynthesis>(&text) {
            Ok(r) if !r.findings.trim().is_empty() || !r.key_facts.is_empty() => {
                out = Some((r, model));
                break;
            }
            Ok(_) => {
                last_err = if finish == "length" {
                    "bị cắt vì hết token".into()
                } else {
                    "trả về rỗng".into()
                };
            }
            Err(e) => last_err = e,
        }
    }
    let (raw, model) = out.ok_or_else(|| format!("không tổng hợp được nghiên cứu ({last_err})"))?;
    Ok(ResearchBundle {
        flow: input.flow.clone(),
        topic: input.topic.clone(),
        findings: raw.findings.trim().to_string(),
        key_facts: raw
            .key_facts
            .into_iter()
            .filter(|f| !f.trim().is_empty())
            .take(8)
            .collect(),
        open_questions: raw
            .open_questions
            .into_iter()
            .filter(|q| !q.trim().is_empty())
            .take(4)
            .collect(),
        confidence: raw.confidence.clamp(0, 100),
        workflows,
        runs: runs.to_vec(),
        model,
    })
}

/// Run every enabled workflow matching `input.flow` (in parallel), then
/// synthesise. `None` when research is disabled or no workflow applies.
pub async fn run_research(db: &Db, input: &ResearchInput) -> Option<Result<ResearchBundle, String>> {
    if !db.get_bool("research_enabled", true) {
        return None;
    }
    let wfs: Vec<crate::db::Workflow> = db
        .list_workflows(true)
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.flow == "both" || w.flow == input.flow)
        .take(MAX_WORKFLOWS_PER_RUN)
        .collect();
    if wfs.is_empty() {
        return None;
    }

    let mut vars: HashMap<String, String> = HashMap::new();
    let topic = if input.topic.trim().is_empty() {
        input.title.clone()
    } else {
        input.topic.clone()
    };
    vars.insert("topic".into(), topic.clone());
    vars.insert("query".into(), topic);
    vars.insert("title".into(), input.title.clone());
    vars.insert("content".into(), truncate(&input.content, 1200));
    vars.insert("post_id".into(), input.post_id.clone());
    vars.insert("flow".into(), input.flow.clone());

    // All workflows in parallel; steps inside each stay sequential.
    let futs = wfs.iter().map(|w| {
        let steps = parse_steps(&w.steps);
        let name = w.name.clone();
        let vars = vars.clone();
        async move { run_workflow(db, &name, &steps, &vars).await }
    });
    let all_runs: Vec<StepRun> = join_all(futs).await.into_iter().flatten().collect();

    if !all_runs.iter().any(|r| r.ok) {
        // Every step failed — surface the first error instead of a fake synthesis.
        let first = all_runs
            .first()
            .map(|r| r.output.clone())
            .unwrap_or_else(|| "không có bước nào chạy".into());
        return Some(Err(format!("mọi bước nghiên cứu đều lỗi ({first})")));
    }

    let mut extract_prompts = vec![db.get_str("research_extract_prompt", "")];
    extract_prompts.extend(wfs.iter().map(|w| w.extract_prompt.clone()));
    let names: Vec<String> = wfs.iter().map(|w| w.name.clone()).collect();
    Some(synthesize(input, &all_runs, names, &extract_prompts).await)
}

/// The questions that should gate publishing, per the configured threshold.
/// Empty = confident enough, nothing to ask.
pub fn gate_questions(db: &Db, bundle: &ResearchBundle) -> Vec<String> {
    let threshold = db.get_i64("research_ask_threshold", 60).clamp(0, 100);
    if threshold == 0 || bundle.confidence >= threshold {
        return Vec::new();
    }
    if !bundle.open_questions.is_empty() {
        return bundle.open_questions.clone();
    }
    // Uncertain but no explicit question — still ask, generically.
    vec![format!(
        "Nghiên cứu mới đạt độ tin cậy {}% (ngưỡng {}%). Bạn bổ sung thông tin/chỉ đạo gì trước khi đăng?",
        bundle.confidence, threshold
    )]
}

// ---------------------------------------------------------------- AI builder

const BUILD_SYSTEM: &str = "You design a research workflow for an AI agent that participates on \
Moltbook. A workflow is a short sequence of MCP tool steps run BEFORE writing a comment/post, to \
gather grounding. You are given the available tools (kind builtin/app/daemon) and the user's \
description of what they want researched. Compose a workflow using ONLY tools from the catalog.\n\
Rules:\n\
- 2-6 steps. Prefer builtin steps; add app/daemon steps when they clearly serve the goal.\n\
- args: use {{topic}} (the researched subject), {{title}}, {{content}}, {{post_id}} placeholders \
where a subject belongs; keep other args minimal and valid per the tool's arg hints.\n\
- A step may set \"save_as\" to name its output; later steps may reference {{that_name}}.\n\
- name: short Vietnamese name describing the workflow.\n\
- flow: \"comment\" (nghiên cứu trước khi bình luận), \"post\" (trước khi đăng bài), or \"both\" \
— pick what the description implies.\n\
Return ONLY valid JSON (no prose, no fences):\n\
{\"name\":\"...\",\"flow\":\"both\",\"steps\":[{\"kind\":\"builtin\",\"tool\":\"wiki_context\",\"args\":{\"query\":\"{{topic}}\"}},{\"kind\":\"app\",\"app\":\"search\",\"tool\":\"search_query\",\"args\":{\"query\":\"{{topic}}\"}}]}";

#[derive(Deserialize, Default)]
struct RawBuild {
    #[serde(default)]
    name: String,
    #[serde(default)]
    flow: String,
    #[serde(default)]
    steps: Vec<Step>,
}

/// Ask the LLM to compose a workflow from the live tool catalog, validate it,
/// and return `(name, flow, steps)` ready to store.
pub async fn ai_build_workflow(
    description: &str,
    flow_hint: &str,
) -> Result<(String, String, Vec<Step>), String> {
    let cat = catalog().await;
    let mut lines = String::new();
    for t in cat.get("builtin").and_then(Value::as_array).into_iter().flatten() {
        lines.push_str(&format!(
            "- kind=builtin tool={} — {} (args: {})\n",
            t.get("tool").and_then(Value::as_str).unwrap_or(""),
            t.get("description").and_then(Value::as_str).unwrap_or(""),
            t.get("args").map(|a| a.to_string()).unwrap_or_default(),
        ));
    }
    for app in cat.get("apps").and_then(Value::as_array).into_iter().flatten() {
        let id = app.get("id").and_then(Value::as_str).unwrap_or("");
        for t in app.get("tools").and_then(Value::as_array).into_iter().flatten().take(25) {
            lines.push_str(&format!(
                "- kind=app app={} tool={} — {}\n",
                id,
                t.get("tool").and_then(Value::as_str).unwrap_or(""),
                truncate(t.get("description").and_then(Value::as_str).unwrap_or(""), 140),
            ));
        }
    }
    for srv in cat.get("daemon").and_then(Value::as_array).into_iter().flatten() {
        let name = srv.get("name").and_then(Value::as_str).unwrap_or("");
        for t in srv.get("tools").and_then(Value::as_array).into_iter().flatten().take(15) {
            lines.push_str(&format!(
                "- kind=daemon server={} tool={} — {}\n",
                name,
                t.get("tool").and_then(Value::as_str).unwrap_or(""),
                truncate(t.get("description").and_then(Value::as_str).unwrap_or(""), 140),
            ));
        }
    }

    let mut prompt = format!(
        "CÔNG CỤ KHẢ DỤNG:\n{}\nNGƯỜI DÙNG MUỐN NGHIÊN CỨU:\n{}\n",
        truncate(&lines, 9000),
        description.trim()
    );
    if matches!(flow_hint, "comment" | "post" | "both") {
        prompt.push_str(&format!("\nflow phải là: {flow_hint}\n"));
    }
    prompt.push_str("\nTrả JSON workflow ngay.");

    let (text, _model) = llm::complete(BUILD_SYSTEM, &prompt, 1600).await?;
    let raw: RawBuild = llm::parse_json(&text)
        .map_err(|e| format!("AI trả workflow không hợp lệ ({e}): {}", truncate(&text, 200)))?;

    let flow = match raw.flow.as_str() {
        "comment" | "post" | "both" => raw.flow.clone(),
        _ if matches!(flow_hint, "comment" | "post" | "both") => flow_hint.to_string(),
        _ => "both".into(),
    };
    let name = if raw.name.trim().is_empty() {
        format!("Workflow: {}", truncate(description.trim(), 40))
    } else {
        raw.name.trim().to_string()
    };
    let steps = validate_steps(raw.steps, &cat)?;
    if steps.is_empty() {
        return Err("AI không tạo được bước hợp lệ nào từ catalog".into());
    }
    Ok((name, flow, steps))
}

/// Keep only steps that reference real tools from the catalog.
pub fn validate_steps(steps: Vec<Step>, cat: &Value) -> Result<Vec<Step>, String> {
    let builtin_names: Vec<String> = builtin_tools().iter().map(|t| t.tool.clone()).collect();
    let app_has = |app: &str, tool: &str| -> bool {
        cat.get("apps")
            .and_then(Value::as_array)
            .map(|apps| {
                apps.iter().any(|a| {
                    a.get("id").and_then(Value::as_str) == Some(app)
                        && a.get("tools")
                            .and_then(Value::as_array)
                            .map(|ts| {
                                ts.iter()
                                    .any(|t| t.get("tool").and_then(Value::as_str) == Some(tool))
                            })
                            .unwrap_or(false)
                })
            })
            .unwrap_or(false)
    };
    let daemon_has = |server: &str| -> bool {
        cat.get("daemon")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .any(|s| s.get("name").and_then(Value::as_str) == Some(server))
            })
            .unwrap_or(false)
    };

    let mut out = Vec::new();
    for mut s in steps.into_iter().take(MAX_STEPS_PER_WORKFLOW) {
        if !s.args.is_object() {
            s.args = json!({});
        }
        let ok = match s.kind.as_str() {
            "builtin" => builtin_names.iter().any(|b| b == &s.tool),
            "app" => !s.app.is_empty() && !s.tool.is_empty() && app_has(&s.app, &s.tool),
            "daemon" => !s.server.is_empty() && !s.tool.is_empty() && daemon_has(&s.server),
            _ => false,
        };
        if ok {
            out.push(s);
        }
    }
    Ok(out)
}

/// The two seeded defaults — builtin-only so they can never break.
pub fn default_workflows() -> Vec<(&'static str, &'static str, Value, &'static str)> {
    vec![
        (
            "Trí nhớ & Kho thông tin",
            "both",
            json!([
                { "kind": "builtin", "tool": "knowledge_recall", "args": { "query": "{{topic}}" } },
                { "kind": "builtin", "tool": "wiki_context", "args": { "query": "{{topic}}", "max_chars": 2000 } }
            ]),
            "",
        ),
        (
            "Cộng đồng Moltbook",
            "both",
            json!([
                { "kind": "builtin", "tool": "moltbook_search", "args": { "q": "{{topic}}", "type": "all", "limit": 8 } },
                { "kind": "builtin", "tool": "moltbook_get_post", "args": { "post_id": "{{post_id}}" } }
            ]),
            "",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("topic".into(), "agent memory".into());
        m.insert("post_id".into(), "p-42".into());
        m
    }

    #[test]
    fn substitute_replaces_in_nested_strings() {
        let v = json!({ "q": "about {{topic}}", "nested": { "id": "{{post_id}}" }, "n": 5,
                        "arr": ["{{topic}}", 7] });
        let out = substitute(&v, &vars());
        assert_eq!(out["q"], "about agent memory");
        assert_eq!(out["nested"]["id"], "p-42");
        assert_eq!(out["n"], 5);
        assert_eq!(out["arr"][0], "agent memory");
        assert_eq!(out["arr"][1], 7);
    }

    #[test]
    fn substitute_leaves_unknown_placeholders() {
        let v = json!({ "q": "{{mystery}}" });
        let out = substitute(&v, &vars());
        assert_eq!(out["q"], "{{mystery}}");
    }

    /// {{post_id}} with no target (new-post flow) must mark the step
    /// inapplicable — never call the tool with a blank id.
    #[test]
    fn missing_placeholders_detects_empty_and_unknown_vars() {
        let mut vars = vars();
        vars.insert("empty".into(), "  ".into());
        let v = json!({ "a": "{{topic}}", "b": "{{empty}}", "c": { "d": "{{ghost}}" } });
        let missing = missing_placeholders(&v, &vars);
        assert_eq!(missing, vec!["empty".to_string(), "ghost".to_string()]);
        assert!(missing_placeholders(&json!({ "a": "{{topic}}" }), &vars).is_empty());
    }

    #[test]
    fn parse_steps_bounds_and_tolerates_garbage() {
        assert!(parse_steps("not json").is_empty());
        let many: Vec<Value> = (0..20)
            .map(|i| json!({ "kind": "builtin", "tool": format!("t{i}") }))
            .collect();
        let s = parse_steps(&serde_json::to_string(&many).unwrap());
        assert_eq!(s.len(), MAX_STEPS_PER_WORKFLOW);
    }

    #[test]
    fn step_labels_are_readable() {
        let s: Step = serde_json::from_value(
            json!({ "kind": "app", "app": "search", "tool": "search_query" }),
        )
        .unwrap();
        assert_eq!(s.label(), "app:search/search_query");
        let b: Step =
            serde_json::from_value(json!({ "kind": "builtin", "tool": "wiki_context" })).unwrap();
        assert_eq!(b.label(), "builtin:wiki_context");
    }

    #[test]
    fn validate_steps_drops_unknown_tools() {
        let cat = json!({
            "builtin": [],
            "apps": [ { "id": "search", "tools": [ { "tool": "search_query" } ] } ],
            "daemon": [ { "name": "senclaw-memory", "tools": [] } ],
        });
        let steps: Vec<Step> = serde_json::from_value(json!([
            { "kind": "builtin", "tool": "wiki_context", "args": {} },
            { "kind": "builtin", "tool": "made_up", "args": {} },
            { "kind": "app", "app": "search", "tool": "search_query" },
            { "kind": "app", "app": "search", "tool": "nope" },
            { "kind": "app", "app": "ghost", "tool": "search_query" },
            { "kind": "daemon", "server": "senclaw-memory", "tool": "memory_search" },
            { "kind": "daemon", "server": "ghost", "tool": "x" },
            { "kind": "??", "tool": "wiki_context" }
        ]))
        .unwrap();
        let ok = validate_steps(steps, &cat).unwrap();
        let labels: Vec<String> = ok.iter().map(|s| s.label()).collect();
        assert_eq!(
            labels,
            vec![
                "builtin:wiki_context",
                "app:search/search_query",
                "mcp:senclaw-memory/memory_search",
            ]
        );
    }

    #[test]
    fn bundle_render_and_json_roundtrip() {
        let b = ResearchBundle {
            flow: "post".into(),
            topic: "MCP naming".into(),
            findings: "Các server đặt tên theo senclaw-<domain>.".into(),
            key_facts: vec!["cognitive dùng prefix cog_".into()],
            open_questions: vec!["Có cần nêu ví dụ ngoài repo không?".into()],
            confidence: 45,
            workflows: vec!["Trí nhớ & Kho thông tin".into()],
            runs: vec![StepRun {
                workflow: "Trí nhớ & Kho thông tin".into(),
                step: "builtin:wiki_context".into(),
                ok: true,
                output: "…".into(),
                ms: 12,
            }],
            model: "m".into(),
        };
        let r = b.render();
        assert!(r.contains("độ tin cậy 45%"));
        assert!(r.contains("Dữ kiện chính"));
        let back = ResearchBundle::from_json(&b.to_json()).unwrap();
        assert_eq!(back.confidence, 45);
        assert_eq!(back.open_questions.len(), 1);
        assert_eq!(back.sources_line(), "builtin:wiki_context");
    }

    #[test]
    fn default_workflows_are_builtin_only() {
        for (_, _, steps, _) in default_workflows() {
            let steps: Vec<Step> = serde_json::from_value(steps).unwrap();
            assert!(!steps.is_empty());
            assert!(steps.iter().all(|s| s.kind == "builtin"));
        }
    }

    #[test]
    fn empty_findings_render_empty() {
        assert_eq!(ResearchBundle::default().render(), "");
    }
}
