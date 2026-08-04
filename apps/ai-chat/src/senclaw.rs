//! SenClaw daemon client for the pieces the AI Chat app *reuses* rather than
//! rebuilds: knowledge (scoped spaces), the skills + persona inventory, the
//! registered-MCP inventory (for the per-bot allowlist picker), the shared
//! wiki, and speech (STT/TTS).

use crate::llm::{base_url, bridge_url, http};
use serde_json::{json, Value};
use std::collections::HashMap;

async fn bridge(action: &str, payload: Value) -> Result<Value, String> {
    let resp = http()
        .post(bridge_url())
        .json(&json!({ "action": action, "payload": payload }))
        .send()
        .await
        .map_err(|e| format!("bridge {} failed: {}", action, e))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("invalid bridge response: {}", e))?;
    match v.get("status").and_then(|x| x.as_str()) {
        Some("ok") => Ok(v),
        _ => Err(v
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown bridge error")
            .to_string()),
    }
}

// ---- knowledge (reuses SenClaw's cognitive store, scoped by `space`) ----

/// Save a memory into a knowledge space.
pub async fn knowledge_save(space: &str, text: &str, source: &str) -> Result<(), String> {
    bridge(
        "knowledge.save",
        json!({ "text": text, "space": space, "source": source }),
    )
    .await
    .map(|_| ())
}

/// Upload a file into a knowledge space. Forwards to the daemon's
/// `/api/cognitive/upload`, which extracts the text (pdf/docx/txt/md/…) and
/// cognifies it — so the app doesn't need its own file parsers.
pub async fn knowledge_upload(
    space: &str,
    filename: &str,
    content_type: &str,
    bytes: Vec<u8>,
) -> Result<Value, String> {
    let base = base_url().trim_end_matches('/').to_string();
    // The daemon's extract_text keys off the filename extension + content-type;
    // set the mime only when the browser provided a valid one.
    let raw = reqwest::multipart::Part::bytes(bytes).file_name(filename.to_string());
    let part = if content_type.is_empty() {
        raw
    } else {
        raw.mime_str(content_type).unwrap_or_else(|_| {
            reqwest::multipart::Part::text(String::new()).file_name(filename.to_string())
        })
    };
    let form = reqwest::multipart::Form::new()
        .text("space", space.to_string())
        .text("source", format!("ai-chat:upload:{filename}"))
        .part("file", part);
    let resp = http()
        .post(format!("{base}/api/cognitive/upload"))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(180))
        .send()
        .await
        .map_err(|e| format!("upload failed: {e}"))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "daemon upload {}: {}",
            code,
            body.chars().take(240).collect::<String>()
        ));
    }
    resp.json().await.map_err(|e| e.to_string())
}

/// Recall: a synthesized, grounded answer over one space (empty when nothing
/// relevant, or joined snippets when the daemon has no cognitive LLM).
/// Uses `hybrid` mode so the actual CHUNK text is retrieved — the daemon's
/// default `graph` mode returns bare entities, which don't carry the fact.
pub async fn knowledge_recall(space: &str, query: &str, limit: i64) -> Result<String, String> {
    let v = bridge(
        "knowledge.recall",
        json!({ "query": query, "space": space, "limit": limit, "mode": "hybrid" }),
    )
    .await?;
    Ok(v.get("answer")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string())
}

/// Raw hits for the Knowledge screen (id/name/summary/score).
pub async fn knowledge_search(space: &str, query: &str, limit: i64) -> Result<Value, String> {
    let v = bridge(
        "knowledge.search",
        json!({ "query": query, "space": space, "limit": limit, "mode": "hybrid" }),
    )
    .await?;
    Ok(json!({ "hits": v.get("hits").cloned().unwrap_or(json!([])) }))
}

/// Browse the top nodes of a space (count/summary for the UI). Probes the
/// scoped-spaces endpoint first so an old daemon can't return the GLOBAL graph.
pub async fn knowledge_nodes(space: &str, limit: i64) -> Result<Value, String> {
    let base = base_url().trim_end_matches('/').to_string();
    let supports = match http()
        .get(format!("{}/api/cognitive/spaces", base))
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => resp
            .json::<Value>()
            .await
            .map(|v| v.get("spaces").is_some())
            .unwrap_or(false),
        _ => false,
    };
    if !supports {
        return Err(
            "daemon SenClaw chưa hỗ trợ knowledge spaces — cập nhật daemon rồi khởi động lại"
                .into(),
        );
    }
    let url = format!(
        "{}/api/cognitive/top-nodes?space={}&limit={}",
        base,
        urlencode(space),
        limit
    );
    let resp = http().get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("daemon trả về {}", resp.status()));
    }
    resp.json().await.map_err(|e| e.to_string())
}

pub async fn knowledge_count(space: &str) -> Result<usize, String> {
    let v = knowledge_nodes(space, 200).await?;
    Ok(v["nodes"].as_array().map(|a| a.len()).unwrap_or(0))
}

// ---- inventories for the per-bot policy picker ----

/// Curated core tools (from the virtual-worker pool) offered in the allowlist,
/// with a Vietnamese label. MCP tools come from `mcp_inventory`.
const CORE_TOOLS: &[(&str, &str)] = &[
    ("Read", "Đọc tệp trong workspace"),
    ("Write", "Ghi/ tạo tệp"),
    ("Edit", "Sửa tệp"),
    ("Bash", "Chạy lệnh shell (rủi ro cao)"),
    ("Glob", "Tìm tệp theo mẫu"),
    ("Grep", "Tìm nội dung trong tệp"),
    ("TodoWrite", "Ghi danh sách việc cần làm"),
    ("Skill", "Gọi kỹ năng (skill) được phép"),
    ("WebSearch", "Tìm kiếm web"),
    ("WebFetch", "Tải & đọc một URL"),
];

/// `{ core:[{name,description}], servers:[{name,description,builtin,tools:[full]}] }`.
/// The per-bot allowlist stores the FULLY-QUALIFIED tool names selected here.
pub async fn mcp_inventory() -> Value {
    let core: Vec<Value> = CORE_TOOLS
        .iter()
        .map(|(n, d)| json!({ "name": n, "description": d }))
        .collect();
    let mut servers: Vec<Value> = Vec::new();
    let base = base_url().trim_end_matches('/').to_string();
    if let Ok(resp) = http().get(format!("{}/api/mcp-servers", base)).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                for s in v["servers"].as_array().unwrap_or(&Vec::new()) {
                    let name = s["name"].as_str().unwrap_or("").to_string();
                    if name.is_empty() {
                        continue;
                    }
                    let tools: Vec<Value> = s["tools"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|t| tool_full_name(&name, t))
                                .map(|full| json!({ "name": full }))
                                .collect()
                        })
                        .unwrap_or_default();
                    servers.push(json!({
                        "name": name,
                        "description": s["description"].as_str().unwrap_or(""),
                        "builtin": s["builtin"].as_bool().unwrap_or(false),
                        "tools": tools,
                    }));
                }
            }
        }
    }
    json!({ "core": core, "servers": servers })
}

/// Normalize a server tool entry (string or `{name}`) into `mcp__<server>__<tool>`.
fn tool_full_name(server: &str, entry: &Value) -> Option<String> {
    let raw = match entry {
        Value::String(s) => s.clone(),
        Value::Object(_) => entry["name"].as_str()?.to_string(),
        _ => return None,
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if raw.starts_with("mcp__") {
        Some(raw.to_string())
    } else {
        Some(format!("mcp__{server}__{raw}"))
    }
}

/// Skills + personas available on the daemon (the skill allowlist picker).
pub async fn skills_inventory_grouped() -> Value {
    let base = base_url().trim_end_matches('/').to_string();
    let mut skills: Vec<Value> = Vec::new();
    let mut personas: Vec<Value> = Vec::new();
    if let Ok(resp) = http().get(format!("{}/api/skills", base)).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                for s in v["skills"].as_array().unwrap_or(&Vec::new()) {
                    if let Some(name) = s["name"].as_str().filter(|n| !n.is_empty()) {
                        skills.push(json!({ "name": name, "description": s["description"].as_str().unwrap_or("") }));
                    }
                }
            }
        }
    }
    if let Ok(resp) = http()
        .get(format!("{}/api/cowork/personas", base))
        .send()
        .await
    {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                for p in v.as_array().unwrap_or(&Vec::new()) {
                    if let Some(name) = p["name"].as_str().filter(|n| !n.is_empty()) {
                        personas.push(json!({ "name": name, "description": p["description"].as_str().unwrap_or("") }));
                    }
                }
            }
        }
    }
    json!({ "skills": skills, "personas": personas })
}

/// Map skill names → descriptions, for folding into a bot's system prompt.
#[allow(dead_code)]
pub async fn skills_map() -> HashMap<String, String> {
    let mut out = HashMap::new();
    let inv = skills_inventory_grouped().await;
    for s in inv["skills"].as_array().unwrap_or(&Vec::new()) {
        if let Some(n) = s["name"].as_str() {
            out.insert(
                n.to_string(),
                s["description"].as_str().unwrap_or("").to_string(),
            );
        }
    }
    out
}

// ---- wiki (shared knowledge base) ----

pub async fn wiki_write(path: &str, content: &str, commit_msg: &str) -> Result<(), String> {
    let url = format!("{}/api/wiki/file", base_url().trim_end_matches('/'));
    let resp = http()
        .put(&url)
        .json(&json!({
            "path": path, "content": content, "tags": ["ai-chat"],
            "source": "app:ai-chat", "commitMsg": commit_msg,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("wiki write: daemon trả về {}", resp.status()));
    }
    Ok(())
}

// ---- speech ----

#[allow(dead_code)]
pub async fn stt(audio: Vec<u8>, filename: &str, language: Option<&str>) -> Result<String, String> {
    let base = base_url().trim_end_matches('/').to_string();
    let part = reqwest::multipart::Part::bytes(audio).file_name(filename.to_string());
    let mut form = reqwest::multipart::Form::new().part("audio", part);
    if let Some(lang) = language.filter(|l| !l.is_empty()) {
        form = form.text("language", lang.to_string());
    }
    let resp = http()
        .post(format!("{}/api/whisper/transcribe", base))
        .multipart(form)
        .timeout(std::time::Duration::from_secs(90))
        .send()
        .await
        .map_err(|e| format!("stt failed: {}", e))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "daemon STT {}: {}",
            code,
            body.chars().take(200).collect::<String>()
        ));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(v.get("text")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string())
}

#[allow(dead_code)]
pub async fn tts(text: &str) -> Result<Vec<u8>, String> {
    let base = base_url().trim_end_matches('/').to_string();
    let resp = http()
        .post(format!("{}/api/tts/synthesize", base))
        .json(&json!({ "text": text }))
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("tts failed: {}", e))?;
    if !resp.status().is_success() {
        let code = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "daemon TTS {}: {}",
            code,
            body.chars().take(200).collect::<String>()
        ));
    }
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
