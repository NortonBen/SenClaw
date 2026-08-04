//! Nền tảng thông tin ngoài — **khám phá MCP động**. App không gắn cứng địa chỉ
//! nào: nó hỏi daemon (`GET /api/mcp-servers`) xem hiện có những MCP server nào,
//! chấm điểm các tool có khả năng tìm kiếm, rồi gọi chúng qua JSON-RPC
//! (`POST <url>/message`) để lấy bằng chứng cho một câu hỏi dự đoán.
//!
//! Người dùng chọn nguồn ở tab Cài đặt (hoặc để **auto** — app tự chọn các
//! nguồn điểm cao nhất đang chạy). Không có nguồn nào → pipeline vẫn chạy, chỉ
//! ghi chú rõ trong `evidence_note`.

use serde_json::{json, Value};
use std::time::Duration;

/// Tool nào đáng coi là "tìm kiếm": điểm càng cao càng ưu tiên khi auto.
/// Trả về None nếu tool không phục vụ việc tra cứu thông tin.
pub fn score_tool(name: &str, description: &str) -> Option<i32> {
    let n = name.to_lowercase();
    let d = description.to_lowercase();
    // Loại các tool ghi/sửa/xoá — chỉ lấy tool đọc.
    if [
        "create", "add", "delete", "remove", "update", "send", "post", "write", "set_", "approve",
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
        _ if n.contains("_query") || n == "query" => 45,
        _ if n.contains("find") => 40,
        _ => 0,
    };
    if score == 0 {
        // Không khớp tên thì dựa vào mô tả (vd "tìm kiếm liên nguồn").
        if d.contains("tìm kiếm") || d.contains("search the web") || d.contains("tra cứu") {
            score = 35;
        } else {
            return None;
        }
    }
    // Ưu tiên nguồn tin tức / web / nghiên cứu; hạ nguồn nội bộ hẹp.
    for (kw, bonus) in [
        ("news", 25),
        ("tin tức", 25),
        ("web", 20),
        ("research", 20),
        ("bài viết", 10),
        ("nguồn", 10),
    ] {
        if n.contains(kw) || d.contains(kw) {
            score += bonus;
        }
    }
    for (kw, penalty) in [
        ("code", 40),
        ("graph", 25),
        ("email", 20),
        ("json", 30),
        ("test", 25),
    ] {
        if n.contains(kw) {
            score -= penalty;
        }
    }
    Some(score)
}

/// Một nguồn tìm kiếm khả dụng.
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
    /// JSON-RPC endpoint suy từ url SSE mà daemon công bố.
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

/// Hỏi daemon danh sách MCP server rồi lọc ra các nguồn tìm kiếm gọi được
/// (transport http — stdio server chỉ agent gọi được, app thì không).
pub async fn discover(http: &reqwest::Client, daemon_base: &str) -> Vec<Source> {
    let url = format!("{}/api/mcp-servers", daemon_base.trim_end_matches('/'));
    let Ok(resp) = http.get(&url).timeout(Duration::from_secs(8)).send().await else {
        return vec![];
    };
    let Ok(v) = resp.json::<Value>().await else {
        return vec![];
    };
    let mut out: Vec<Source> = Vec::new();
    for srv in v["servers"].as_array().unwrap_or(&vec![]) {
        let (Some(server), Some(surl)) = (srv["name"].as_str(), srv["url"].as_str()) else {
            continue;
        };
        if srv["transport"].as_str() != Some("http") || surl.is_empty() {
            continue;
        }
        for t in srv["tools"].as_array().unwrap_or(&vec![]) {
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

/// Chọn nguồn theo cài đặt: danh sách khoá `server.tool`, hoặc "auto"/rỗng →
/// lấy `auto_top` nguồn điểm cao nhất (mỗi server tối đa một tool).
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

/// Tên tham số truy vấn của một tool, đọc từ `tools/list` của chính server đó.
/// None khi không tra được (caller dùng mặc định "query").
async fn query_param(
    http: &reqwest::Client,
    msg_url: &str,
    tool: &str,
) -> Option<(String, Option<String>)> {
    let v: Value = http
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
    let q = [
        "query", "q", "keyword", "text", "question", "search", "term",
    ]
    .iter()
    .find(|k| props.contains_key(**k))
    .map(|k| k.to_string())?;
    let limit = ["limit", "count", "max_results", "top_k", "n"]
        .iter()
        .find(|k| props.contains_key(**k))
        .map(|k| k.to_string());
    Some((q, limit))
}

/// Trích bằng chứng từ payload trả về của một tool bất kỳ (shape không biết
/// trước): tìm mảng object đầu tiên có vẻ là kết quả, map sang {title, snippet, url}.
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
                "snippet": truncate(&snippet, 400),
                "url": pick(&["url", "link", "href", "source_url"]),
            }))
        })
        .collect()
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max).collect::<String>() + "…"
    }
}

/// Gọi một nguồn MCP với truy vấn cho trước. Trả về danh sách bằng chứng đã
/// gắn nhãn nguồn; rỗng khi server không phản hồi hoặc không có kết quả.
pub async fn query_source(
    http: &reqwest::Client,
    src: &Source,
    query: &str,
    limit: i64,
) -> Vec<Value> {
    let msg_url = src.message_url();
    let (qkey, limit_key) = query_param(http, &msg_url, &src.tool)
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
    let Ok(resp) = http
        .post(&msg_url)
        .json(&body)
        .timeout(Duration::from_secs(45))
        .send()
        .await
    else {
        return vec![];
    };
    let Ok(v) = resp.json::<Value>().await else {
        return vec![];
    };
    let Some(text) = v["result"]["content"][0]["text"].as_str() else {
        return vec![];
    };
    let mut items = match serde_json::from_str::<Value>(text) {
        Ok(parsed) => extract_items(&parsed, limit as usize),
        // Tool trả văn bản thuần → coi cả khối là một tài liệu.
        Err(_) if !text.trim().is_empty() => {
            vec![
                json!({ "title": format!("{} · {}", src.server, src.tool), "snippet": truncate(text, 600), "url": "" }),
            ]
        }
        Err(_) => vec![],
    };
    for it in items.iter_mut() {
        it["source"] = json!(src.key());
    }
    items
}

/// Thu thập bằng chứng cho nhiều truy vấn qua các nguồn đã chọn.
/// Trả về `(items, note)` — note nói rõ đã dùng nguồn nào / vì sao rỗng.
pub async fn gather(
    http: &reqwest::Client,
    sources: &[&Source],
    queries: &[String],
    per_query: i64,
) -> (Vec<Value>, String) {
    if sources.is_empty() {
        return (
            vec![],
            "Chưa chọn nguồn MCP tìm kiếm nào (hoặc không có server nào đang chạy) — dự đoán chỉ dựa trên dữ liệu nội bộ."
                .to_string(),
        );
    }
    let mut items: Vec<Value> = Vec::new();
    for q in queries.iter().filter(|q| !q.trim().is_empty()) {
        for src in sources {
            for it in query_source(http, src, q, per_query).await {
                let title = it["title"].as_str().unwrap_or("").to_string();
                let dup = !title.is_empty()
                    && items
                        .iter()
                        .any(|x| x["title"].as_str() == Some(title.as_str()));
                if !dup {
                    items.push(it);
                }
            }
        }
    }
    let used: Vec<String> = sources.iter().map(|s| s.key()).collect();
    let note = if items.is_empty() {
        format!(
            "Đã hỏi {} nhưng không có kết quả liên quan.",
            used.join(", ")
        )
    } else {
        format!("{} bằng chứng từ {}.", items.len(), used.join(", "))
    };
    (items, note)
}

/// Normalize + clamp a synthesizer trace: guarantees `p ∈ [0.01,0.99]`,
/// well-typed lists, and drops junk. None when there is no usable `p`.
pub fn normalize_trace(v: &Value) -> Option<Value> {
    let p = v["p"]
        .as_f64()
        .or_else(|| v["p_final"].as_f64())?
        .clamp(0.01, 0.99);
    let arr = |key: &str| -> Vec<Value> {
        v[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|x| x.is_string() || x.is_object())
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let conf = match v["confidence"].as_str().unwrap_or("") {
        c @ ("thấp" | "vừa" | "cao") => c,
        _ => "vừa",
    };
    Some(json!({
        "p": (p * 1000.0).round() / 1000.0,
        "confidence": conf,
        "outside_view": {
            "base_rate": v["outside_view"]["base_rate"].as_f64().map(|b| (b.clamp(0.0, 1.0) * 1000.0).round() / 1000.0),
            "rationale": v["outside_view"]["rationale"].as_str().unwrap_or(""),
        },
        "evidence_for": arr("evidence_for"),
        "evidence_against": arr("evidence_against"),
        "adjustments": arr("adjustments"),
        "premortem": v["premortem"].as_str().unwrap_or(""),
        "granularity_note": v["granularity_note"].as_str().unwrap_or(""),
        "update_triggers": arr("update_triggers"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(server: &str, tool: &str, score: i32) -> Source {
        Source {
            server: server.into(),
            tool: tool.into(),
            url: format!("http://127.0.0.1:9999/api/mcp/sse"),
            description: String::new(),
            score,
        }
    }

    #[test]
    fn tool_scoring_prefers_news_and_web_search() {
        let news = score_tool("news_search", "Tìm kiếm bài viết tin tức").unwrap();
        let web = score_tool("search_query", "Tìm kiếm liên nguồn web").unwrap();
        let code = score_tool("search_code", "Tìm trong mã nguồn").unwrap();
        assert!(news > code && web > code);
        // Tool ghi/xoá bị loại hẳn.
        assert!(score_tool("news_source_add", "Thêm nguồn").is_none());
        assert!(score_tool("crm_contact_delete", "Xoá").is_none());
        // Tool không liên quan tra cứu → None.
        assert!(score_tool("clock_now", "Giờ hiện tại").is_none());
    }

    #[test]
    fn select_auto_and_explicit() {
        let all = vec![
            src("news-mcp", "news_search", 125),
            src("zeach-mcp", "zeach_search", 100),
            src("news-mcp", "news_list", 80),
        ];
        // auto: mỗi server một tool, điểm cao trước
        let auto = select(&all, "auto", 3);
        assert_eq!(auto.len(), 2);
        assert_eq!(auto[0].key(), "news-mcp.news_search");
        assert_eq!(auto[1].key(), "zeach-mcp.zeach_search");
        assert_eq!(select(&all, "", 1).len(), 1);
        // chọn tay
        let picked = select(&all, "zeach-mcp.zeach_search", 3);
        assert_eq!(picked.len(), 1);
        assert_eq!(picked[0].server, "zeach-mcp");
        assert!(select(&all, "không-tồn-tại.x", 3).is_empty());
    }

    #[test]
    fn message_url_from_sse() {
        assert_eq!(
            src("a", "b", 1).message_url(),
            "http://127.0.0.1:9999/api/mcp/message"
        );
        let mut s = src("a", "b", 1);
        s.url = "http://x/api/mcp/message".into();
        assert_eq!(s.message_url(), "http://x/api/mcp/message");
    }

    #[test]
    fn extract_items_handles_various_shapes() {
        // shape kiểu search app
        let a = extract_items(
            &json!({ "evidence": [{ "title": "T1", "snippet": "S1", "meta": {} }] }),
            5,
        );
        assert_eq!(a[0]["title"], "T1");
        // shape kiểu news app
        let b = extract_items(
            &json!({ "articles": [{ "title": "Tin", "summary": "Tóm", "link": "http://x" }] }),
            5,
        );
        assert_eq!(b[0]["url"], "http://x");
        assert_eq!(b[0]["snippet"], "Tóm");
        // mảng trần
        let c = extract_items(&json!([{ "name": "N", "description": "D" }]), 5);
        assert_eq!(c[0]["title"], "N");
        // rác → rỗng
        assert!(extract_items(&json!({ "ok": true }), 5).is_empty());
        assert!(extract_items(&json!({ "items": [{ "x": 1 }] }), 5).is_empty());
    }

    #[tokio::test]
    async fn gather_without_sources_explains_itself() {
        let http = crate::fetch::http();
        let (items, note) = gather(&http, &[], &["x".into()], 5).await;
        assert!(items.is_empty());
        assert!(note.contains("Chưa chọn nguồn"));
    }

    #[test]
    fn normalize_trace_clamps_and_defaults() {
        let t = normalize_trace(&json!({
            "p": 1.7,
            "confidence": "vô-định",
            "outside_view": { "base_rate": -0.2, "rationale": "r" },
            "evidence_for": ["a", { "e": 1 }, 5],
            "premortem": "sai vì...",
            "update_triggers": ["tin A"]
        }))
        .unwrap();
        assert_eq!(t["p"], 0.99);
        assert_eq!(t["confidence"], "vừa");
        assert_eq!(t["outside_view"]["base_rate"], 0.0);
        assert_eq!(t["evidence_for"].as_array().unwrap().len(), 2);
        assert!(normalize_trace(&json!({ "reasoning": "no p" })).is_none());
        assert_eq!(
            normalize_trace(&json!({ "p_final": 0.4 })).unwrap()["p"],
            0.4
        );
    }
}
