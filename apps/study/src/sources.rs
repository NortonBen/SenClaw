//! External knowledge, discovered rather than hard-coded.
//!
//! Ported from `apps/predict/src/evidence.rs`. The app never stores a URL for
//! a search backend: it asks the daemon (`GET /api/mcp-servers`) what is
//! running, scores the tools that look like lookups, and calls them over
//! JSON-RPC. Installing `zeach`, `search` or `news` is therefore enough to give
//! Study a research surface; uninstalling one does not break it.
//!
//! Three rules this app adds on top of the port, all of them about not letting
//! the outside world quietly become the syllabus:
//!
//! 1. **Read-only tools only.** Anything whose name suggests a mutation
//!    (`create`, `delete`, `send`, …) is excluded from scoring. A source of
//!    evidence must not have side effects.
//! 2. **External evidence is labelled.** It is merged into answers, but as
//!    "nguồn ngoài" — never presented as something the learner's own material
//!    said. Quiz generation ignores it entirely (see `quiz.rs`): testing
//!    someone on material you never gave them is indefensible.
//! 3. **Retrieved text is data, not instructions.** Everything is passed
//!    through `llm::sanitize_retrieved` before it can reach a prompt, and what
//!    was stripped is reported.

use std::time::Duration;

use serde_json::{json, Value};

use crate::config;

/// Score a tool as a lookup surface. `None` = not a lookup tool.
pub fn score_tool(name: &str, description: &str) -> Option<i32> {
    let n = name.to_lowercase();
    let d = description.to_lowercase();
    // Writers are excluded outright — a "source" that can delete things is not
    // a source.
    if [
        "create", "add", "delete", "remove", "update", "send", "post", "write", "set_", "approve",
        "upload", "import",
    ]
    .iter()
    .any(|w| n.contains(w))
    {
        return None;
    }
    let mut score = match () {
        _ if n.ends_with("_search") || n == "search" => 100,
        _ if n.contains("search") => 80,
        _ if n.contains("research") => 75,
        _ if n.contains("_ask") || n == "ask" => 60,
        _ if n.contains("_query") || n == "query" => 45,
        _ if n.contains("find") => 40,
        _ => 0,
    };
    if score == 0 {
        if d.contains("tìm kiếm") || d.contains("search the web") || d.contains("tra cứu") {
            score = 35;
        } else {
            return None;
        }
    }
    for (kw, bonus) in [
        ("research", 25),
        ("web", 20),
        ("news", 15),
        ("tin tức", 15),
        ("wiki", 15),
        ("nguồn", 10),
    ] {
        if n.contains(kw) || d.contains(kw) {
            score += bonus;
        }
    }
    for (kw, penalty) in [("code", 30), ("graph", 20), ("json", 30), ("test", 25)] {
        if n.contains(kw) {
            score -= penalty;
        }
    }
    // A tool that enumerates *other* tools or reports status is a lookup by
    // name only — it returns a catalogue, not evidence, and auto-select kept
    // picking one (`moltbook_research_tools`) over a real search tool.
    if ["_tools", "_list", "_status", "_config", "_help", "_schema"]
        .iter()
        .any(|suffix| n.ends_with(suffix))
    {
        score -= 60;
    }
    // Never recommend ourselves: study_ask already runs over the local corpus,
    // and calling back in would double-count the same evidence.
    if n.starts_with("study_") {
        return None;
    }
    Some(score)
}

#[derive(Debug, Clone)]
pub struct Source {
    pub server: String,
    pub tool: String,
    pub url: String,
    pub description: String,
    pub score: i32,
}

impl Source {
    pub fn key(&self) -> String {
        format!("{}.{}", self.server, self.tool)
    }

    /// JSON-RPC endpoint derived from the SSE url the daemon publishes.
    pub fn message_url(&self) -> String {
        if self.url.ends_with("/sse") {
            format!("{}/message", self.url.trim_end_matches("/sse"))
        } else {
            self.url.clone()
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "key": self.key(), "server": self.server, "tool": self.tool,
            "description": self.description, "score": self.score,
        })
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("build http client")
}

/// Ask the daemon which MCP servers are up, and keep the callable lookup tools.
///
/// `transport == "http"` only: a stdio server is reachable by the agent, not by
/// a Space App.
pub async fn discover() -> Vec<Source> {
    let url = format!(
        "{}/api/mcp-servers",
        config::senclaw_base_url().trim_end_matches('/')
    );
    let Ok(resp) = http().get(&url).timeout(Duration::from_secs(8)).send().await else {
        return vec![];
    };
    let Ok(v) = resp.json::<Value>().await else {
        return vec![];
    };
    parse_servers(&v)
}

/// Pure half of [`discover`], so the shape handling is testable.
pub fn parse_servers(v: &Value) -> Vec<Source> {
    let empty = vec![];
    let mut out: Vec<Source> = Vec::new();
    for srv in v["servers"].as_array().unwrap_or(&empty) {
        let (Some(server), Some(surl)) = (srv["name"].as_str(), srv["url"].as_str()) else {
            continue;
        };
        if srv["transport"].as_str() != Some("http") || surl.is_empty() {
            continue;
        }
        for t in srv["tools"].as_array().unwrap_or(&empty) {
            let Some(tool) = t["name"].as_str() else {
                continue;
            };
            let desc = t["description"].as_str().unwrap_or("");
            if let Some(score) = score_tool(tool, desc) {
                out.push(Source {
                    server: server.to_string(),
                    tool: tool.to_string(),
                    url: surl.to_string(),
                    description: desc.chars().take(160).collect(),
                    score,
                });
            }
        }
    }
    out.sort_by(|a, b| b.score.cmp(&a.score).then(a.server.cmp(&b.server)));
    out
}

/// Pick sources: an explicit `server.tool` list, or `auto` = the highest-scoring
/// tools, at most one per server.
pub fn select<'a>(all: &'a [Source], setting: &str, auto_top: usize) -> Vec<&'a Source> {
    let setting = setting.trim();
    if !setting.is_empty() && setting != "auto" {
        let wanted: Vec<&str> = setting
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        return all
            .iter()
            .filter(|s| wanted.contains(&s.key().as_str()))
            .collect();
    }
    let mut picked: Vec<&Source> = Vec::new();
    for s in all {
        if s.score > 60 && !picked.iter().any(|p| p.server == s.server) {
            picked.push(s);
            if picked.len() >= auto_top {
                break;
            }
        }
    }
    picked
}

/// The tool's query (and limit) argument names, read from its own schema.
async fn query_param(msg_url: &str, tool: &str) -> Option<(String, Option<String>)> {
    let v: Value = http()
        .post(msg_url)
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }))
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let t = v["result"]["tools"]
        .as_array()?
        .iter()
        .find(|t| t["name"].as_str() == Some(tool))?;
    let props = t["inputSchema"]["properties"].as_object()?;
    let q = ["query", "q", "keyword", "text", "question", "search", "term"]
        .iter()
        .find(|k| props.contains_key(**k))
        .map(|k| k.to_string())?;
    let limit = ["limit", "count", "max_results", "top_k", "n"]
        .iter()
        .find(|k| props.contains_key(**k))
        .map(|k| k.to_string());
    Some((q, limit))
}

/// Pull results out of an unknown payload shape.
pub fn extract_items(payload: &Value, limit: usize) -> Vec<Value> {
    const ARRAY_KEYS: [&str; 9] = [
        "evidence",
        "results",
        "items",
        "articles",
        "hits",
        "docs",
        "documents",
        "posts",
        "matches",
    ];
    let arr: Option<&Vec<Value>> = ARRAY_KEYS
        .iter()
        .find_map(|k| payload.get(*k).and_then(|v| v.as_array()))
        .or_else(|| payload.as_array());
    let Some(arr) = arr else { return vec![] };
    arr.iter()
        .take(limit)
        .filter_map(|it| {
            let pick = |keys: &[&str]| -> String {
                keys.iter()
                    .find_map(|k| it.get(*k).and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string()
            };
            let title = pick(&["title", "name", "headline", "subject", "question"]);
            let snippet = pick(&[
                "snippet",
                "summary",
                "description",
                "content",
                "text",
                "excerpt",
                "answer",
            ]);
            if title.is_empty() && snippet.is_empty() {
                return None;
            }
            Some(json!({
                "title": title,
                "snippet": crate::corpus::head(&snippet, 500),
                "url": pick(&["url", "link", "href", "source_url"]),
            }))
        })
        .collect()
}

/// Query one source. Returns labelled, injection-filtered items.
pub async fn query_source(src: &Source, query: &str, limit: i64) -> (Vec<Value>, Vec<String>) {
    let msg_url = src.message_url();
    let (qkey, limit_key) = query_param(&msg_url, &src.tool)
        .await
        .unwrap_or_else(|| ("query".to_string(), Some("limit".to_string())));
    let mut args = json!({ qkey: query });
    if let Some(lk) = limit_key {
        args[lk] = json!(limit);
    }
    let body = json!({
        "jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": { "name": src.tool, "arguments": args }
    });
    let Ok(resp) = http().post(&msg_url).json(&body).send().await else {
        return (vec![], vec![]);
    };
    let Ok(v) = resp.json::<Value>().await else {
        return (vec![], vec![]);
    };
    let Some(text) = v["result"]["content"][0]["text"].as_str() else {
        return (vec![], vec![]);
    };
    let mut items = match serde_json::from_str::<Value>(text) {
        Ok(parsed) => extract_items(&parsed, limit as usize),
        // A tool that answered in prose is one document.
        Err(_) if !text.trim().is_empty() => vec![json!({
            "title": format!("{} · {}", src.server, src.tool),
            "snippet": crate::corpus::head(text, 700),
            "url": "",
        })],
        Err(_) => vec![],
    };

    // Retrieved text is data. Strip instruction-shaped lines before any of it
    // can reach a prompt, and keep what was removed so the UI can show it.
    let mut dropped_all = Vec::new();
    for it in items.iter_mut() {
        let (clean, dropped) = crate::llm::sanitize_retrieved(it["snippet"].as_str().unwrap_or(""));
        it["snippet"] = json!(clean);
        it["source"] = json!(src.key());
        it["external"] = json!(true);
        for d in dropped {
            dropped_all.push(format!("{}: {}", src.key(), crate::corpus::head(&d, 120)));
        }
    }
    items.retain(|it| {
        !it["snippet"].as_str().unwrap_or("").trim().is_empty()
            || !it["title"].as_str().unwrap_or("").trim().is_empty()
    });
    (items, dropped_all)
}

#[derive(Debug, Default)]
pub struct Gathered {
    pub items: Vec<Value>,
    pub note: String,
    /// Instruction-shaped lines removed from retrieved content.
    pub filtered: Vec<String>,
}

/// Query several sources for several queries.
pub async fn gather(sources: &[&Source], queries: &[String], per_query: i64) -> Gathered {
    if sources.is_empty() {
        return Gathered {
            items: vec![],
            note: "Chưa có nguồn MCP tìm kiếm nào đang chạy — câu trả lời chỉ dựa trên tài liệu của bạn.".into(),
            filtered: vec![],
        };
    }
    let mut out = Gathered::default();
    for q in queries.iter().filter(|q| !q.trim().is_empty()) {
        for src in sources {
            let (items, filtered) = query_source(src, q, per_query).await;
            out.filtered.extend(filtered);
            for it in items {
                let title = it["title"].as_str().unwrap_or("").to_string();
                let dup = !title.is_empty()
                    && out
                        .items
                        .iter()
                        .any(|x| x["title"].as_str() == Some(title.as_str()));
                if !dup {
                    out.items.push(it);
                }
            }
        }
    }
    let used: Vec<String> = sources.iter().map(|s| s.key()).collect();
    out.note = if out.items.is_empty() {
        format!("Đã hỏi {} nhưng không có kết quả liên quan.", used.join(", "))
    } else {
        format!(
            "{} kết quả từ nguồn ngoài: {}.",
            out.items.len(),
            used.join(", ")
        )
    };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_tools_outrank_generic_query_tools() {
        let s = score_tool("zeach_search", "Nghiên cứu sâu có dẫn chứng").unwrap();
        let q = score_tool("lakehouse_query", "chạy SQL").unwrap();
        assert!(s > q, "search {s} should beat query {q}");
    }

    #[test]
    fn tools_with_side_effects_are_never_sources() {
        for t in [
            "crm_create_contact",
            "email_send",
            "kanban_update_card",
            "space_event_delete",
            "study_doc_upload",
        ] {
            assert!(score_tool(t, "làm gì đó").is_none(), "{t} must be excluded");
        }
    }

    #[test]
    fn our_own_tools_are_excluded_so_evidence_is_not_double_counted() {
        assert!(score_tool("study_ask", "hỏi trong tài liệu").is_none());
    }

    #[test]
    fn a_catalogue_tool_loses_to_a_real_search_tool() {
        // Observed live: `moltbook_research_tools` was auto-selected over
        // `news_search`. It lists tools; it does not return evidence.
        let cat = score_tool("moltbook_research_tools", "danh sách công cụ nghiên cứu").unwrap();
        let real = score_tool("news_search", "tìm kiếm tin tức").unwrap();
        assert!(real > cat, "catalogue {cat} must not outrank search {real}");
    }

    #[test]
    fn a_tool_that_only_describes_itself_as_search_still_qualifies() {
        assert!(score_tool("lookup_thing", "tra cứu thông tin nội bộ").is_some());
        assert!(score_tool("render_pdf", "vẽ tài liệu ra PDF").is_none());
    }

    #[test]
    fn only_http_servers_are_reachable_from_an_app() {
        let v = json!({"servers": [
            {"name": "a-mcp", "transport": "http", "url": "http://x/sse",
             "tools": [{"name": "a_search", "description": "web search"}]},
            {"name": "b-mcp", "transport": "stdio", "url": "",
             "tools": [{"name": "b_search", "description": "web search"}]},
        ]});
        let got = parse_servers(&v);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].server, "a-mcp");
    }

    #[test]
    fn the_message_url_is_derived_from_the_sse_url() {
        let s = Source {
            server: "x".into(),
            tool: "t".into(),
            url: "http://127.0.0.1:4530/api/mcp/sse".into(),
            description: String::new(),
            score: 100,
        };
        assert_eq!(s.message_url(), "http://127.0.0.1:4530/api/mcp/message");
    }

    #[test]
    fn auto_selection_takes_at_most_one_tool_per_server() {
        let mk = |server: &str, tool: &str, score: i32| Source {
            server: server.into(),
            tool: tool.into(),
            url: "http://x/sse".into(),
            description: String::new(),
            score,
        };
        let all = vec![
            mk("a", "a_search", 100),
            mk("a", "a_find", 90),
            mk("b", "b_search", 85),
        ];
        let picked = select(&all, "auto", 5);
        assert_eq!(picked.len(), 2);
        assert_eq!(picked[0].server, "a");
        assert_eq!(picked[1].server, "b");
    }

    #[test]
    fn an_explicit_selection_is_honoured_exactly() {
        let all = vec![Source {
            server: "a".into(),
            tool: "a_search".into(),
            url: "http://x/sse".into(),
            description: String::new(),
            score: 100,
        }];
        assert_eq!(select(&all, "a.a_search", 5).len(), 1);
        assert!(select(&all, "khong.co", 5).is_empty());
    }

    #[test]
    fn results_are_extracted_from_several_payload_shapes() {
        assert_eq!(
            extract_items(&json!({"results": [{"title": "A", "url": "u"}]}), 5).len(),
            1
        );
        assert_eq!(extract_items(&json!([{"name": "B", "text": "x"}]), 5).len(), 1);
        assert_eq!(extract_items(&json!({"items": [{}]}), 5).len(), 0);
        assert_eq!(extract_items(&json!({"nope": 1}), 5).len(), 0);
    }
}
