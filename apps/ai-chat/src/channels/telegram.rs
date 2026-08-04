//! Telegram adapter over the raw Bot API (long-polling `getUpdates`, no
//! webhook). Config: `{ "token": "<bot token>" }`. The channel `cursor` holds
//! the next `getUpdates` offset.

use crate::channels::Inbound;
use crate::db::{Channel, Db};
use crate::llm::http;
use serde_json::Value;
use std::sync::Arc;

const TG_MAX_LEN: usize = 4000;
const POLL_TIMEOUT_SECS: u32 = 25;

fn token(ch: &Channel) -> Result<String, String> {
    ch.config
        .get("token")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "kênh Telegram thiếu 'token'".to_string())
}

/// Long-poll for new updates. Advances + persists the offset cursor.
pub async fn poll(db: &Arc<Db>, ch: &Channel) -> Result<Vec<Inbound>, String> {
    let token = token(ch)?;
    let offset: i64 = ch.cursor.parse().unwrap_or(0);
    let url = format!("https://api.telegram.org/bot{token}/getUpdates");
    let resp = http()
        .get(&url)
        .query(&[
            ("offset", offset.to_string()),
            ("timeout", POLL_TIMEOUT_SECS.to_string()),
            ("allowed_updates", "[\"message\"]".to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("telegram getUpdates lỗi: {e}"))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("telegram phản hồi lỗi: {e}"))?;
    if !v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false) {
        return Err(v
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("telegram từ chối")
            .to_string());
    }
    let mut out = Vec::new();
    let mut max_id = offset - 1;
    for u in v["result"].as_array().unwrap_or(&Vec::new()) {
        let uid = u["update_id"].as_i64().unwrap_or(0);
        max_id = max_id.max(uid);
        let Some(msg) = u.get("message") else {
            continue;
        };
        let Some(text) = msg["text"].as_str().filter(|t| !t.trim().is_empty()) else {
            continue;
        };
        let chat_id = msg["chat"]["id"].as_i64().unwrap_or(0);
        if chat_id == 0 {
            continue;
        }
        let first = msg["from"]["first_name"].as_str().unwrap_or("");
        let last = msg["from"]["last_name"].as_str().unwrap_or("");
        let name = format!("{first} {last}").trim().to_string();
        out.push(Inbound {
            external_id: chat_id.to_string(),
            customer_name: name,
            text: text.to_string(),
        });
    }
    if max_id >= offset {
        let _ = db.set_channel_sync(ch.id, "ok", "", Some(&(max_id + 1).to_string()));
    }
    Ok(out)
}

/// Send a reply, chunking anything over Telegram's message limit.
pub async fn send(
    _db: &Arc<Db>,
    ch: &Channel,
    external_id: &str,
    text: &str,
) -> Result<(), String> {
    let token = token(ch)?;
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    for chunk in split_message(text) {
        let resp = http()
            .post(&url)
            .json(&serde_json::json!({ "chat_id": external_id, "text": chunk }))
            .send()
            .await
            .map_err(|e| format!("telegram sendMessage lỗi: {e}"))?;
        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!(
                "telegram từ chối gửi: {}",
                body.chars().take(200).collect::<String>()
            ));
        }
    }
    Ok(())
}

/// Quick credential check for the Channels "Test" button.
pub async fn health_check(ch: &Channel) -> Result<String, String> {
    let token = token(ch)?;
    let url = format!("https://api.telegram.org/bot{token}/getMe");
    let v: Value = http()
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if v["ok"].as_bool().unwrap_or(false) {
        Ok(format!(
            "@{}",
            v["result"]["username"].as_str().unwrap_or("bot")
        ))
    } else {
        Err(v["description"]
            .as_str()
            .unwrap_or("token không hợp lệ")
            .to_string())
    }
}

fn split_message(text: &str) -> Vec<String> {
    if text.chars().count() <= TG_MAX_LEN {
        return vec![text.to_string()];
    }
    let mut parts = Vec::new();
    let mut cur = String::new();
    for line in text.split_inclusive('\n') {
        if cur.chars().count() + line.chars().count() > TG_MAX_LEN && !cur.is_empty() {
            parts.push(std::mem::take(&mut cur));
        }
        // A single very long line: hard-split on char boundaries.
        if line.chars().count() > TG_MAX_LEN {
            let mut buf = String::new();
            for ch in line.chars() {
                buf.push(ch);
                if buf.chars().count() >= TG_MAX_LEN {
                    parts.push(std::mem::take(&mut buf));
                }
            }
            cur.push_str(&buf);
        } else {
            cur.push_str(line);
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}
