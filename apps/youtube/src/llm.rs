//! The app's single gateway to SenClaw services. Every LLM call goes through the
//! app-space-sdk (the daemon's Space-App open API) — the app never contacts an LLM
//! provider directly.

use app_space_sdk::SpaceClient;
use serde_json::{json, Value};

fn client() -> SpaceClient {
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "youtube");
    }
    SpaceClient::from_env()
}

/// One-shot completion on SenClaw's active LLM via the SDK open API.
pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<(String, String), String> {
    client()
        .llm_request(system, user, max_tokens)
        .await
        .map_err(|e| e.to_string())
}

const COMMENT_SYSTEM: &str = "You draft a short, friendly, on-topic YouTube comment (or a \
reply to one) on the user's behalf. Keep it natural and specific to the context given — never \
spammy, never link-baiting, no @-mention spam. 1-3 sentences. Return ONLY the comment text, no \
quotes, no preamble. Write in the same language as the context.";

const POST_SYSTEM: &str = "You draft a YouTube Community post for the user's own channel — an \
announcement/update addressed TO their audience, NOT a comment on someone else's video. Write \
1-2 short paragraphs (roughly 30-90 words), warm and direct, optionally ending with one light \
question to invite replies. No hashtag spam (0-2 at most), no links unless the context supplies \
one, no clickbait. Return ONLY the post text, no quotes, no preamble. Write in the same language \
as the context.";

/// Draft a body for a write action. The system prompt depends on `kind`: a
/// community post is an announcement to your own audience, which reads nothing
/// like a comment left on someone else's video.
/// The result is a DRAFT — stored, and must be approved before it can be sent.
pub async fn draft_body(kind: &str, context: &str, instruction: Option<&str>) -> Result<(String, String), String> {
    let (system, closing, budget) = match kind {
        "community_post" => (POST_SYSTEM, "Write the community post now.", 700),
        _ => (COMMENT_SYSTEM, "Write the comment now.", 400),
    };
    let mut prompt = format!("Context:\n{context}\n");
    if let Some(i) = instruction {
        if !i.trim().is_empty() {
            prompt.push_str(&format!("\nInstruction: {i}\n"));
        }
    }
    prompt.push_str(&format!("\n{closing}"));
    bridge_llm(system, &prompt, budget).await
}

const ANALYZE_SYSTEM: &str = "You classify YouTube comments. For EACH input comment, output one \
object with: id (echo it back), sentiment (one of pos|neu|neg), sentimentScore (-1.0..1.0), \
intent (one of question|complaint|praise|suggestion|spam|offtopic|other), topics (array of 0-3 \
short lowercase labels), lang (ISO code like vi/en), isSpam (bool), toxicity (0.0..1.0). Return \
ONLY a JSON array of these objects, no prose, no markdown fences. Judge the comment's own text; \
be conservative with isSpam/toxicity.";

/// One comment's analysis as returned by the model.
#[derive(serde::Deserialize)]
pub struct Analysis {
    pub id: String,
    #[serde(default = "neu")]
    pub sentiment: String,
    #[serde(default, rename = "sentimentScore")]
    pub sentiment_score: f64,
    #[serde(default = "other")]
    pub intent: String,
    #[serde(default)]
    pub topics: Vec<String>,
    #[serde(default)]
    pub lang: String,
    #[serde(default, rename = "isSpam")]
    pub is_spam: bool,
    #[serde(default)]
    pub toxicity: f64,
}
fn neu() -> String { "neu".into() }
fn other() -> String { "other".into() }

/// Analyse a batch of `(id, text)` comments in one LLM call. Returns (analyses, model).
/// Keep batches small (≤15) so the JSON array fits the bridge's output ceiling.
pub async fn analyze_batch(comments: &[(String, String)]) -> Result<(Vec<Analysis>, String), String> {
    if comments.is_empty() {
        return Ok((vec![], String::new()));
    }
    let mut prompt = String::from("Comments to classify (JSON):\n");
    let items: Vec<Value> = comments
        .iter()
        .map(|(id, text)| json!({ "id": id, "text": text }))
        .collect();
    prompt.push_str(&serde_json::to_string(&items).unwrap_or_default());
    prompt.push_str("\n\nReturn the JSON array now.");

    let (text, model) = bridge_llm(ANALYZE_SYSTEM, &prompt, 3000).await?;
    let arr = parse_analyses(&text)
        .ok_or_else(|| format!("không parse được JSON phân tích từ model:\n{}", &text.chars().take(300).collect::<String>()))?;
    Ok((arr, model))
}

/// Extract the JSON array from possibly-fenced/chatty model output.
fn parse_analyses(text: &str) -> Option<Vec<Analysis>> {
    if let Ok(a) = serde_json::from_str::<Vec<Analysis>>(text.trim()) {
        return Some(a);
    }
    // Strip ``` fences.
    let t = text.trim();
    let cleaned = if let Some(rest) = t.strip_prefix("```") {
        rest.splitn(2, '\n').nth(1).unwrap_or(rest).trim_end_matches("```").trim()
    } else {
        t
    };
    if let Ok(a) = serde_json::from_str::<Vec<Analysis>>(cleaned) {
        return Some(a);
    }
    // Fall back to the first balanced [...] block.
    let bytes = cleaned.as_bytes();
    let start = bytes.iter().position(|&b| b == b'[')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            match b {
                b'\\' if !esc => esc = true,
                b'"' if !esc => in_str = false,
                _ => esc = false,
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str::<Vec<Analysis>>(&cleaned[start..=i]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// The daemon's configured LLMs via the SDK → { activeId, configs:[…] }.
pub async fn list_models() -> Result<Value, String> {
    let (active, configs) = client().list_models().await.map_err(|e| e.to_string())?;
    let configs: Vec<Value> = configs
        .into_iter()
        .map(|m| json!({ "id": m.id, "modelName": m.model_name, "provider": m.provider }))
        .collect();
    Ok(json!({ "activeId": active, "configs": configs }))
}

pub async fn set_active_model(id: &str) -> Result<(), String> {
    client().set_active_model(id).await.map_err(|e| e.to_string())
}

/// P10 — save `(author, text)` comments into the app's private knowledge space so
/// `knowledge.recall` can ground future draft replies. One doc per comment (better
/// recall granularity). Returns how many were saved.
pub async fn knowledge_index(items: &[(String, String)], video_id: &str) -> Result<usize, String> {
    let c = client();
    let mut n = 0usize;
    for (author, text) in items {
        let doc = format!("YouTube comment on video {video_id} — {author}: {text}");
        c.knowledge_save(&doc, Some("youtube-comments"), Some("youtube"))
            .await
            .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_analyses_handles_fences() {
        let raw = "```json\n[{\"id\":\"a\",\"sentiment\":\"pos\",\"sentimentScore\":0.8,\
                   \"intent\":\"praise\",\"topics\":[\"chất lượng\"],\"lang\":\"vi\",\
                   \"isSpam\":false,\"toxicity\":0.0}]\n```";
        let a = parse_analyses(raw).expect("parse");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].id, "a");
        assert_eq!(a[0].sentiment, "pos");
        assert!(a[0].sentiment_score > 0.5);
        assert_eq!(a[0].intent, "praise");
    }

    #[test]
    fn parse_analyses_extracts_embedded_array_and_defaults() {
        let raw = "Sure: [{\"id\":\"b\",\"intent\":\"question\"}] done";
        let a = parse_analyses(raw).expect("parse");
        assert_eq!(a[0].id, "b");
        assert_eq!(a[0].intent, "question");
        // Missing fields fall back to defaults, not an error.
        assert_eq!(a[0].sentiment, "neu");
        assert!(!a[0].is_spam);
    }

    #[test]
    fn parse_analyses_rejects_garbage() {
        assert!(parse_analyses("not json at all").is_none());
    }
}
