//! Bridge to the SenClaw daemon's shared LLM (replaces kaizen's Dify
//! integration — 4 API keys, streaming `/chat-messages` — with one call).
//!
//! The bridge takes only system/prompt/maxTokens/profile: no temperature knob
//! and no streaming. `finish == "length"` must be treated as an error.

use std::sync::OnceLock;
use std::time::Duration;

use serde_json::{json, Value};

use crate::config;

fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .expect("build http client")
    })
}

fn describe(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        out.push_str(&format!(": {s}"));
        src = s.source();
    }
    out
}

/// One completion through the daemon bridge. Returns `(text, finish)`.
pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<(String, String), String> {
    let url = format!(
        "{}/api/space/apps/{}/bridge",
        config::senclaw_base_url().trim_end_matches('/'),
        config::app_id()
    );
    let body = json!({
        "action": "llm.request",
        "payload": { "system": system, "prompt": user, "maxTokens": max_tokens },
    });

    let mut last_err = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(700 * attempt as u64)).await;
        }
        let resp = match http().post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("bridge llm.request failed ({url}): {}", describe(&e));
                continue;
            }
        };
        let v: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                last_err = format!("invalid bridge response: {}", describe(&e));
                continue;
            }
        };
        return match v.get("status").and_then(Value::as_str) {
            Some("ok") => Ok((
                v.get("text").and_then(Value::as_str).unwrap_or("").to_string(),
                v.get("finish").and_then(Value::as_str).unwrap_or("").to_string(),
            )),
            Some("pending") => Err("bridge LLM chưa được bật trong daemon này".to_string()),
            _ => Err(v
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown LLM error")
                .to_string()),
        };
    }
    Err(last_err)
}

/// Extract a JSON array from model output — port of kaizen's
/// `extractQuestionsJsonArray`: strip a ```json fence, try the whole text,
/// then the outermost `[...]` span.
pub fn extract_json_array(text: &str) -> Result<Vec<Value>, String> {
    let mut t = text.trim();
    if t.is_empty() {
        return Err("Phản hồi AI trống.".to_string());
    }
    if let Some(stripped) = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .and_then(|s| s.trim_end().strip_suffix("```"))
    {
        t = stripped.trim();
    }
    let try_arr = |s: &str| -> Option<Vec<Value>> {
        serde_json::from_str::<Value>(s)
            .ok()
            .and_then(|v| v.as_array().cloned())
    };
    if let Some(a) = try_arr(t) {
        return Ok(a);
    }
    if let Some(start) = t.find('[') {
        if let Some(end) = t.rfind(']').filter(|e| start < *e) {
            if let Some(a) = try_arr(&t[start..=end]) {
                return Ok(a);
            }
        }
        // Truncated-array repair (mindmap lesson): drop the trailing partial
        // object and close the bracket — works whether or not a `]` survived.
        let tail = &t[start..];
        if let Some(cut) = tail.rfind('}') {
            if let Some(a) = try_arr(&format!("{}]", &tail[..=cut])) {
                return Ok(a);
            }
        }
    }
    Err("Không tìm thấy mảng JSON hợp lệ trong phản hồi AI.".to_string())
}

/// Generate multiple-choice grammar questions. Prompt kept equivalent to
/// kaizen's Dify prompt; validation of each item happens at the caller.
pub async fn generate_grammar_questions(
    topic: &str,
    level: &str,
    count: u32,
    grammar_content: Option<&str>,
) -> Result<Vec<Value>, String> {
    let count = count.clamp(1, 50);
    let context = match grammar_content {
        // Ground the questions in the actual lesson text when we have it —
        // kaizen couldn't do this (Dify only got the topic name).
        Some(c) if !c.trim().is_empty() => {
            let c: String = c.chars().take(6000).collect();
            format!("\nBase the questions on this grammar lesson content:\n\"\"\"\n{c}\n\"\"\"\n")
        }
        _ => String::new(),
    };
    let prompt = format!(
        "Generate {count} multiple-choice English grammar questions for level {level} about \"{topic}\".{context}\
         Return ONLY a JSON array with objects containing structure: \
         {{ \"content\": \"question text\", \"options\": [{{\"id\": \"A\", \"text\": \"opt A\"}}, {{\"id\": \"B\", \"text\": \"opt B\"}}, {{\"id\": \"C\", \"text\": \"opt C\"}}, {{\"id\": \"D\", \"text\": \"opt D\"}}], \
         \"correctAnswerId\": \"A\", \"explanation\": \"why this is correct\" }}. \
         Vary which option is correct. Do not wrap the JSON in markdown code fences."
    );
    let system = "You are an English grammar test writer. You answer with raw JSON only — no prose, no markdown.";
    let (text, finish) = bridge_llm(system, &prompt, 16_000).await?;
    if finish == "length" {
        return Err(format!(
            "model cắt output giữa chừng (finish=length) khi sinh {count} câu — giảm số câu hỏi"
        ));
    }
    extract_json_array(&text)
}

/// Extract a JSON object from model output (fence-tolerant), for the drafts
/// that return one record instead of a list.
pub fn extract_json_object(text: &str) -> Result<Value, String> {
    let mut t = text.trim();
    if t.is_empty() {
        return Err("Phản hồi AI trống.".to_string());
    }
    if let Some(stripped) = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))
        .and_then(|s| s.trim_end().strip_suffix("```"))
    {
        t = stripped.trim();
    }
    if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(t) {
        return Ok(v);
    }
    if let (Some(start), Some(end)) = (t.find('{'), t.rfind('}')) {
        if start < end {
            if let Ok(v @ Value::Object(_)) = serde_json::from_str::<Value>(&t[start..=end]) {
                return Ok(v);
            }
        }
    }
    Err("Không tìm thấy JSON hợp lệ trong phản hồi AI.".to_string())
}

const JSON_SYSTEM: &str = "You are a curriculum author for Vietnamese learners of English. You answer with raw JSON only — no prose, no markdown fences.";

/// Draft a full grammar lesson (markdown body) — the admin screen shows it for
/// review before anything is saved.
pub async fn draft_grammar_lesson(topic: &str, level: &str, note: &str) -> Result<Value, String> {
    let extra = if note.trim().is_empty() {
        String::new()
    } else {
        format!("\nAdditional requirements: {note}")
    };
    let prompt = format!(
        "Write an English grammar lesson about \"{topic}\" for CEFR level {level}, aimed at Vietnamese learners.{extra}\n\
         Return ONLY a JSON object: {{\"title\": \"...\", \"description\": \"one sentence\", \"content\": \"markdown\"}}.\n\
         The markdown `content` must contain, in this order: a short intro, a `## Công thức` section with formula tables or bullet lists, \
         a `## Cách dùng` section, a `## Ví dụ` section with at least 5 example sentences (English + Vietnamese translation), \
         and a `## Lỗi thường gặp` section. Explanations in Vietnamese, example sentences in English. \
         Escape newlines properly so the JSON stays valid."
    );
    let (text, finish) = bridge_llm(JSON_SYSTEM, &prompt, 16_000).await?;
    if finish == "length" {
        return Err("model cắt output giữa chừng (finish=length) — thử rút gọn yêu cầu".into());
    }
    let v = extract_json_object(&text)?;
    if v["content"].as_str().unwrap_or("").trim().is_empty() {
        return Err("AI không trả về nội dung bài giảng.".into());
    }
    Ok(v)
}

/// Draft a vocabulary list ready for the pipe-format importer.
pub async fn draft_vocab_list(topic: &str, level: &str, count: u32) -> Result<Vec<Value>, String> {
    let count = count.clamp(1, 60);
    let prompt = format!(
        "List {count} English vocabulary items about \"{topic}\" suitable for CEFR level {level}, for a Vietnamese learner.\n\
         Return ONLY a JSON array of objects: \
         {{\"word\": \"...\", \"meaning\": \"nghĩa tiếng Việt\", \"example\": \"one natural English sentence\", \
         \"partOfSpeech\": \"noun|verb|adjective|adverb|phrase\", \"ipa\": \"/.../\", \"explain\": \"short English definition\"}}. \
         No duplicates, everyday useful words first."
    );
    let (text, finish) = bridge_llm(JSON_SYSTEM, &prompt, 16_000).await?;
    if finish == "length" {
        return Err(format!("model cắt output giữa chừng khi tạo {count} từ — giảm số lượng"));
    }
    let items = extract_json_array(&text)?;
    let out: Vec<Value> = items
        .into_iter()
        .filter(|v| !v["word"].as_str().unwrap_or("").trim().is_empty())
        .collect();
    if out.is_empty() {
        return Err("AI không trả về từ nào hợp lệ.".into());
    }
    Ok(out)
}

/// Draft a dictation passage. The caller splits it into timed segments.
pub async fn draft_dictation_passage(
    topic: &str,
    level: &str,
    sentences: u32,
) -> Result<String, String> {
    let sentences = sentences.clamp(3, 30);
    let prompt = format!(
        "Write a natural English passage about \"{topic}\" for CEFR level {level}, exactly {sentences} sentences, \
         suitable for a dictation exercise (clear, everyday vocabulary, no lists, no headings). \
         Return ONLY the passage as plain text."
    );
    let (text, finish) = bridge_llm(
        "You are a listening-practice script writer. Answer with plain text only.",
        &prompt,
        8_000,
    )
    .await?;
    if finish == "length" {
        return Err("model cắt output giữa chừng — giảm số câu".into());
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("AI không trả về nội dung.".into());
    }
    Ok(text)
}

/// Split a passage into dictation segments, distributing `duration_seconds`
/// proportionally to segment length so the timings are usable straight away
/// (the editor lets the user nudge them against the real audio).
pub fn split_into_segments(text: &str, duration_seconds: f64) -> Vec<Value> {
    let mut sentences: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in text.trim().chars() {
        current.push(ch);
        if matches!(ch, '.' | '!' | '?' | '\n') {
            let s = current.trim().to_string();
            if !s.is_empty() {
                sentences.push(s);
            }
            current.clear();
        }
    }
    let tail = current.trim().to_string();
    if !tail.is_empty() {
        sentences.push(tail);
    }

    let total_chars: usize = sentences.iter().map(|s| s.chars().count()).sum();
    let mut out = Vec::with_capacity(sentences.len());
    let mut cursor = 0.0f64;
    for (i, s) in sentences.iter().enumerate() {
        let share = if total_chars == 0 || duration_seconds <= 0.0 {
            0.0
        } else {
            duration_seconds * (s.chars().count() as f64 / total_chars as f64)
        };
        let start = (cursor * 100.0).round() / 100.0;
        cursor += share;
        let end = if i == sentences.len() - 1 && duration_seconds > 0.0 {
            duration_seconds
        } else {
            (cursor * 100.0).round() / 100.0
        };
        out.push(json!({
            "content": s,
            "solutions": [],
            "startTime": start,
            "endTime": end,
            "orderIndex": i,
        }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_passage_and_spreads_the_duration() {
        let segs = split_into_segments("It snowed last night. The kids were happy! Really?", 30.0);
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0]["content"], "It snowed last night.");
        assert_eq!(segs[2]["content"], "Really?");
        assert_eq!(segs[0]["startTime"], 0.0);
        // Timings are contiguous and end exactly on the audio length.
        assert_eq!(segs[0]["endTime"], segs[1]["startTime"]);
        assert_eq!(segs[2]["endTime"], 30.0);
        assert_eq!(segs[1]["orderIndex"], 1);
    }

    #[test]
    fn split_without_duration_leaves_timings_at_zero() {
        let segs = split_into_segments("One sentence only", 0.0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0]["startTime"], 0.0);
        assert_eq!(segs[0]["endTime"], 0.0);
    }

    #[test]
    fn extracts_objects_from_fenced_or_chatty_output() {
        let o = r#"{"title":"T","content":"body"}"#;
        assert_eq!(extract_json_object(o).unwrap()["title"], "T");
        assert_eq!(
            extract_json_object(&format!("```json\n{o}\n```")).unwrap()["title"],
            "T"
        );
        assert_eq!(
            extract_json_object(&format!("Here you go:\n{o}\nHope it helps")).unwrap()["title"],
            "T"
        );
        assert!(extract_json_object("[1,2]").is_err());
    }

    #[test]
    fn extracts_plain_fenced_and_prefixed_arrays() {
        let arr = r#"[{"content":"q","options":[],"correctAnswerId":"A"}]"#;
        assert_eq!(extract_json_array(arr).unwrap().len(), 1);
        assert_eq!(
            extract_json_array(&format!("```json\n{arr}\n```")).unwrap().len(),
            1
        );
        assert_eq!(
            extract_json_array(&format!("Here are the questions:\n{arr}\nEnjoy!")).unwrap().len(),
            1
        );
    }

    #[test]
    fn repairs_a_truncated_array() {
        let cut = r#"[{"content":"q1","correctAnswerId":"A"},{"content":"q2","correctAnswerId":"B"},{"content":"q3","correc"#;
        let out = extract_json_array(cut).unwrap();
        assert_eq!(out.len(), 2, "drops the partial trailing object");
        assert_eq!(out[1]["content"], "q2");
    }

    #[test]
    fn rejects_garbage() {
        assert!(extract_json_array("").is_err());
        assert!(extract_json_array("no json here").is_err());
        assert!(extract_json_array("{\"an\":\"object\"}").is_err());
    }
}
