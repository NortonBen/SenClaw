//! Client for the SenClaw daemon: per-agent knowledge spaces (via the
//! space-app bridge `knowledge.*` actions) and the shared wiki (REST).
//!
//! Every staff member owns an isolated knowledge space named
//! `ai-office:<agent-key>` — memories saved there never leak into another
//! agent's recall.

use crate::llm::{app_id, base_url, http};
use serde_json::{json, Value};

/// Knowledge-space id for one staff member.
pub fn agent_space(agent_key: &str) -> String {
    format!("ai-office:{}", agent_key)
}

fn bridge_url() -> String {
    format!(
        "{}/api/space/apps/{}/bridge",
        base_url().trim_end_matches('/'),
        app_id()
    )
}

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

/// Run a FULL tool-enabled agent through the daemon (default tools + the
/// app's own MCP + browser/web-search). Isolates memory per `space`. Returns
/// the agent's final text. Slower than `bridge_llm` (it's an agent loop).
pub async fn agent_run(
    space: &str,
    system: &str,
    prompt: &str,
    workspace: &str,
    timeout_secs: u64,
) -> Result<String, String> {
    let resp = http()
        .post(bridge_url())
        .json(&json!({
            "action": "agent.run",
            "payload": {
                "system": system,
                "prompt": prompt,
                "space": space,
                "workspace": workspace,
                "timeoutSeconds": timeout_secs,
            },
        }))
        .timeout(std::time::Duration::from_secs(timeout_secs + 30))
        .send()
        .await
        .map_err(|e| format!("agent.run failed: {}", e))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("invalid agent.run response: {}", e))?;
    match v.get("status").and_then(|x| x.as_str()) {
        Some("ok") => Ok(v.get("text").and_then(|x| x.as_str()).unwrap_or("").trim().to_string()),
        _ => Err(v
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("agent.run error (daemon chưa hỗ trợ?)")
            .to_string()),
    }
}

/// Save a memory into an agent's private knowledge space.
pub async fn knowledge_save(space: &str, text: &str, source: &str) -> Result<(), String> {
    bridge(
        "knowledge.save",
        json!({ "text": text, "space": space, "source": source }),
    )
    .await
    .map(|_| ())
}

/// Recall from an agent's private space: returns a synthesized answer (or
/// joined snippets when the daemon has no LLM), empty when nothing relevant.
pub async fn knowledge_recall(space: &str, query: &str) -> Result<String, String> {
    let v = bridge(
        "knowledge.recall",
        json!({ "query": query, "space": space, "limit": 5 }),
    )
    .await?;
    Ok(v.get("answer").and_then(|x| x.as_str()).unwrap_or("").trim().to_string())
}

/// Browse an agent's space (for the staff-detail dialog / desktop UI).
pub async fn knowledge_nodes(space: &str, limit: i64) -> Result<Value, String> {
    let base = base_url().trim_end_matches('/').to_string();
    // Older daemons ignore the unknown `space` query param and would return
    // the GLOBAL graph as if it were this agent's memory — probe the spaces
    // endpoint (added together with scoped search) to rule that out. The
    // probe must parse as JSON with a `spaces` key: an old daemon answers
    // unknown /api paths with the SPA's index.html (status 200).
    let daemon_supports_spaces = match http()
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
    if !daemon_supports_spaces {
        return Err(
            "daemon SenClaw chưa hỗ trợ knowledge spaces — cập nhật daemon rồi khởi động lại".into(),
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

// ---- speech (STT / TTS) via the daemon ----

/// Transcribe an audio clip through the daemon's Whisper endpoint.
/// Returns the recognized text. Needs a Whisper model installed in the daemon.
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
        return Err(format!("daemon STT {}: {}", code, body.chars().take(200).collect::<String>()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(v.get("text").and_then(|x| x.as_str()).unwrap_or("").trim().to_string())
}

/// Synthesize speech through the daemon's TTS endpoint. Returns WAV bytes.
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
        return Err(format!("daemon TTS {}: {}", code, body.chars().take(200).collect::<String>()));
    }
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

// ---- skills / sub-agents inventory ----

/// Everything a staff member can "hold": daemon skills + cowork personas
/// (sub-agents). Returns `name → description`.
pub async fn skills_inventory() -> Result<std::collections::HashMap<String, String>, String> {
    let mut out = std::collections::HashMap::new();
    let base = base_url().trim_end_matches('/').to_string();
    if let Ok(resp) = http().get(format!("{}/api/skills", base)).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                for s in v["skills"].as_array().unwrap_or(&Vec::new()) {
                    let name = s["name"].as_str().unwrap_or("").to_string();
                    if !name.is_empty() {
                        out.insert(name, s["description"].as_str().unwrap_or("").to_string());
                    }
                }
            }
        }
    }
    if let Ok(resp) = http().get(format!("{}/api/cowork/personas", base)).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                for p in v.as_array().unwrap_or(&Vec::new()) {
                    let name = p["name"].as_str().unwrap_or("").to_string();
                    if !name.is_empty() {
                        out.insert(
                            format!("persona:{}", name),
                            p["description"].as_str().unwrap_or("").to_string(),
                        );
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Same inventory but shaped for the UI picker: two labelled groups.
pub async fn skills_inventory_grouped() -> Value {
    let base = base_url().trim_end_matches('/').to_string();
    let mut skills: Vec<Value> = Vec::new();
    let mut personas: Vec<Value> = Vec::new();
    if let Ok(resp) = http().get(format!("{}/api/skills", base)).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                for s in v["skills"].as_array().unwrap_or(&Vec::new()) {
                    if let Some(name) = s["name"].as_str().filter(|n| !n.is_empty()) {
                        skills.push(serde_json::json!({
                            "name": name,
                            "description": s["description"].as_str().unwrap_or(""),
                        }));
                    }
                }
            }
        }
    }
    if let Ok(resp) = http().get(format!("{}/api/cowork/personas", base)).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                for p in v.as_array().unwrap_or(&Vec::new()) {
                    if let Some(name) = p["name"].as_str().filter(|n| !n.is_empty()) {
                        personas.push(serde_json::json!({
                            "name": format!("persona:{}", name),
                            "description": p["description"].as_str().unwrap_or(""),
                        }));
                    }
                }
            }
        }
    }
    serde_json::json!({ "skills": skills, "personas": personas })
}

/// Count of items in one knowledge space (for the staff-detail summary —
/// the full listing lives in desktop_app's Knowledge screen, not here).
pub async fn knowledge_count(space: &str) -> Result<usize, String> {
    let v = knowledge_nodes(space, 100).await?;
    Ok(v["nodes"].as_array().map(|a| a.len()).unwrap_or(0))
}

// ---- wiki (kho tài liệu chung của văn phòng) ----

/// Search the wiki; returns up to `limit` hits as `(path, title, snippet)`.
pub async fn wiki_search(query: &str, limit: usize) -> Result<Vec<(String, String, String)>, String> {
    let url = format!(
        "{}/api/wiki/search?q={}&limit={}",
        base_url().trim_end_matches('/'),
        urlencode(query),
        limit
    );
    let resp = http().get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("wiki search: daemon trả về {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(v["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|r| {
                    (
                        r["path"].as_str().unwrap_or("").to_string(),
                        r["title"].as_str().unwrap_or("").to_string(),
                        r["snippet"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Read one wiki document's body.
pub async fn wiki_read(path: &str) -> Result<String, String> {
    let url = format!(
        "{}/api/wiki/file?path={}",
        base_url().trim_end_matches('/'),
        urlencode(path)
    );
    let resp = http().get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("wiki read: daemon trả về {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(v["content"].as_str().unwrap_or("").to_string())
}

/// Write a document into the wiki (auto-commits in the wiki's git repo).
pub async fn wiki_write(path: &str, content: &str, commit_msg: &str) -> Result<(), String> {
    let url = format!("{}/api/wiki/file", base_url().trim_end_matches('/'));
    let resp = http()
        .put(&url)
        .json(&json!({
            "path": path,
            "content": content,
            "tags": ["ai-office"],
            "source": "app:ai-office",
            "commitMsg": commit_msg,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("wiki write: daemon trả về {}", resp.status()));
    }
    Ok(())
}

fn urlencode(s: &str) -> String {
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
