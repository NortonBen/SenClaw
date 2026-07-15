//! Bridge to the SenClaw daemon's active LLM. The Moltbook app never talks to a
//! provider directly — every completion goes through the daemon's space-app
//! bridge. Also hosts the engine's planner/composer prompts and a tolerant JSON
//! parser for their structured replies.

use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub fn base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}
pub fn app_id() -> String {
    std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "moltbook".to_string())
}
pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(125))
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

/// One completion through the daemon bridge. Returns `(text, model, finish)`
/// where `finish == "length"` means the provider cut the output at the cap.
/// Transport errors are retried; application errors are surfaced as-is.
pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<(String, String, String), String> {
    let url = format!("{}/api/space/apps/{}/bridge", base_url().trim_end_matches('/'), app_id());
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
        return match v.get("status").and_then(|x| x.as_str()) {
            Some("ok") => Ok((
                v.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("finish").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            )),
            Some("pending") => Err("bridge LLM chưa được bật trong daemon này".to_string()),
            _ => Err(v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown LLM error")
                .to_string()),
        };
    }
    Err(last_err)
}

/// Convenience: `(text, model)`, dropping the finish reason.
pub async fn complete(system: &str, user: &str, max_tokens: u32) -> Result<(String, String), String> {
    bridge_llm(system, user, max_tokens).await.map(|(t, m, _)| (t, m))
}

pub async fn list_models() -> Result<Value, String> {
    let url = format!("{}/api/llm-config", base_url().trim_end_matches('/'));
    let v: Value = http()
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(v)
}

pub async fn set_active_model(id: &str) -> Result<(), String> {
    let url = format!("{}/api/llm-config/active", base_url().trim_end_matches('/'));
    http()
        .post(&url)
        .json(&json!({ "id": id }))
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

// ---- engine planner / composers ----

/// One post the planner considered. `id` is the Moltbook post id.
pub struct FeedItem {
    pub id: String,
    pub submolt: String,
    pub author: String,
    pub title: String,
    pub content: String,
    pub score: i64,
}

#[derive(Deserialize, Default)]
pub struct EngagementPlan {
    #[serde(default)]
    pub upvotes: Vec<String>,
    #[serde(default)]
    pub comments: Vec<PlanComment>,
    #[serde(default)]
    pub new_post: Option<PlanPost>,
    #[serde(default)]
    pub note: String,
}
#[derive(Deserialize, Clone)]
pub struct PlanComment {
    #[serde(default)]
    pub post_id: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub why: String,
}
#[derive(Deserialize, Clone)]
pub struct PlanPost {
    #[serde(default)]
    pub submolt: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub why: String,
}

// Encodes Moltbook's OFFICIAL etiquette (from moltbook.com/rules.md +
// heartbeat.md): authenticity, engagement-over-posting, reply-to-your-repliers
// first, quality-over-quantity, and the anti-spam norms.
const PLAN_SYSTEM: &str = "You are an autonomous AI agent ('molty') on Moltbook, the social \
network for AI agents. Plan this heartbeat's engagements following Moltbook's OFFICIAL etiquette:\n\
- Authenticity: engage because you have something to say — never to farm karma or 'be seen'.\n\
- Engagement OVER posting: replying, upvoting and commenting is almost always more valuable than \
a new post.\n\
- TOP PRIORITY: reply to molties who replied to YOU (the 'people replied to you' list) BEFORE \
anything else — people are talking to you.\n\
- Quality over quantity: NO one-word comments, NO emoji spam, NO duplicates, NO low-effort filler. \
A comment must add a real perspective, question, or build (1-3 sentences), in the SAME language as \
the post. Upvote generously but only what you genuinely find good.\n\
- Post at most ONE new thing, and only if it is genuinely valuable; otherwise leave new_post null. \
Never post out of obligation.\n\
Return ONLY valid JSON (no prose, no code fences) in EXACTLY this shape:\n\
{\"upvotes\":[\"<post_id>\"],\"comments\":[{\"post_id\":\"<id>\",\"content\":\"...\",\"why\":\"1 short reason\"}],\"new_post\":{\"submolt\":\"...\",\"title\":\"...\",\"content\":\"...\",\"why\":\"...\"}|null,\"note\":\"1 sentence summary of what you chose and why\"}\n\
Only use post_ids that appear in the feed or the reply list. Respect the engagement budget given.";

/// Ask the LLM to plan this heartbeat's engagements. `priority` is the list of
/// `(post_id, snippet)` where someone replied to one of YOUR posts — the #1
/// heartbeat action per Moltbook, so it's surfaced first and its ids are valid
/// comment targets even though they aren't in the browse feed.
pub async fn plan_engagements(
    voice: &str,
    items: &[FeedItem],
    priority: &[(String, String)],
    comment_budget: i64,
    default_submolt: &str,
    allow_new_post: bool,
) -> Result<(EngagementPlan, String), String> {
    let mut prompt = String::new();
    prompt.push_str(&format!("Your persona/voice:\n{voice}\n\n"));
    prompt.push_str(&format!(
        "Engagement budget for this check-in: up to {comment_budget} comment(s), a few upvotes, and {} new post. Default submolt for a new post: m/{}.\n\n",
        if allow_new_post { "at most ONE" } else { "ZERO (skip new_post, set it null)" },
        default_submolt.trim_start_matches("m/"),
    ));
    if !priority.is_empty() {
        prompt.push_str("People replied to YOU here — respond to these FIRST (they count against your comment budget):\n");
        for (pid, snippet) in priority.iter().take(10) {
            prompt.push_str(&format!("--- post_id={pid} (reply on your post) ---\n{}\n\n", truncate(snippet, 300)));
        }
    }
    prompt.push_str("Browse feed:\n");
    if items.is_empty() {
        prompt.push_str("(the feed is empty right now)\n");
    }
    for it in items.iter().take(25) {
        prompt.push_str(&format!(
            "--- post_id={} · {} · by {} · score {} ---\n{}\n{}\n\n",
            it.id,
            it.submolt,
            it.author,
            it.score,
            it.title,
            truncate(&it.content, 400),
        ));
    }
    prompt.push_str("Return the JSON plan now.");
    let (text, model) = complete(PLAN_SYSTEM, &prompt, 1800).await?;
    let mut plan: EngagementPlan = parse_json(&text)
        .map_err(|e| format!("could not parse engagement plan ({e}):\n{}", truncate(&text, 300)))?;
    // Enforce the budget and drop new_post when disallowed — never trust the LLM
    // to respect the cap on its own.
    plan.comments.truncate(comment_budget.max(0) as usize);
    if !allow_new_post {
        plan.new_post = None;
    }
    // Valid comment targets = browse feed + priority (your-post) ids. Upvotes are
    // feed-only (you don't upvote your own posts).
    let feed_ids: std::collections::HashSet<&str> = items.iter().map(|i| i.id.as_str()).collect();
    let mut comment_ids = feed_ids.clone();
    for (pid, _) in priority {
        comment_ids.insert(pid.as_str());
    }
    plan.upvotes.retain(|id| feed_ids.contains(id.as_str()));
    plan.comments.retain(|c| comment_ids.contains(c.post_id.as_str()) && !c.content.trim().is_empty());
    Ok((plan, model))
}

const REPLY_SYSTEM: &str = "You are an AI agent on Moltbook writing a reply to another agent's post. \
Write a single substantive comment (1-4 sentences) that genuinely adds to the conversation — a \
question, a counterpoint, a build, or a specific experience. Match the post's language. No \
sycophancy, no filler, no hashtags, no sign-off. Return ONLY the comment text.";

/// Draft a single reply to one post. `instruction` is optional extra steer.
pub async fn compose_reply(
    voice: &str,
    post_title: &str,
    post_content: &str,
    instruction: &str,
) -> Result<(String, String), String> {
    let mut prompt = format!("Your voice:\n{voice}\n\nThe post you're replying to:\nTitle: {post_title}\n{post_content}\n");
    if !instruction.trim().is_empty() {
        prompt.push_str(&format!("\nExtra guidance from your human: {}\n", instruction.trim()));
    }
    prompt.push_str("\nWrite the comment now.");
    complete(REPLY_SYSTEM, &prompt, 500).await.map(|(t, m)| (t.trim().to_string(), m))
}

const POST_SYSTEM: &str = "You are an AI agent on Moltbook drafting an original post. Write \
something worth other agents' time: a genuine observation, a lesson learned, a useful pattern, or \
an honest question. Return ONLY valid JSON (no prose, no code fences): \
{\"title\":\"<max 300 chars, no clickbait>\",\"content\":\"<the body, plain text>\"}. \
Match the language of the topic if one is given, else write in English.";

pub struct DraftedPost {
    pub title: String,
    pub content: String,
}

/// Draft a brand-new post (title + content) for a submolt around an optional topic.
pub async fn compose_post(voice: &str, submolt: &str, topic: &str) -> Result<(DraftedPost, String), String> {
    let mut prompt = format!("Your voice:\n{voice}\n\nSubmolt: m/{}\n", submolt.trim_start_matches("m/"));
    if topic.trim().is_empty() {
        prompt.push_str("Topic: your choice — something you genuinely want to share right now.\n");
    } else {
        prompt.push_str(&format!("Topic: {}\n", topic.trim()));
    }
    prompt.push_str("\nReturn the JSON now.");
    let (text, model) = complete(POST_SYSTEM, &prompt, 900).await?;
    #[derive(Deserialize)]
    struct Out {
        #[serde(default)]
        title: String,
        #[serde(default)]
        content: String,
    }
    let out: Out = parse_json(&text)
        .map_err(|e| format!("could not parse drafted post ({e}):\n{}", truncate(&text, 300)))?;
    Ok((DraftedPost { title: out.title.trim().to_string(), content: out.content.trim().to_string() }, model))
}

const CHALLENGE_SYSTEM: &str = "You are given an obfuscated math word problem used by Moltbook to \
verify that a poster is an AI (not a human). Solve it and return ONLY the numeric answer as a \
string with exactly 2 decimal places, e.g. \"15.00\". No words, no units, no explanation.";

/// Solve a Moltbook content-verification challenge. Returns the numeric answer
/// string (2 decimals). Best-effort — the caller decides what to do on failure.
pub async fn solve_challenge(challenge_text: &str) -> Result<(String, String), String> {
    let (text, model) = complete(CHALLENGE_SYSTEM, challenge_text, 60).await?;
    let answer = normalize_answer(&text);
    if answer.is_empty() {
        return Err(format!("không trích được đáp số từ: {}", truncate(&text, 120)));
    }
    Ok((answer, model))
}

/// Pull the first number out of the model's reply and format it with 2 decimals.
fn normalize_answer(text: &str) -> String {
    let mut num = String::new();
    let mut seen_digit = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            num.push(c);
            seen_digit = true;
        } else if c == '.' && seen_digit && !num.contains('.') {
            num.push(c);
        } else if (c == '-' || c == '+') && num.is_empty() {
            if c == '-' {
                num.push(c);
            }
        } else if seen_digit {
            break;
        }
    }
    match num.parse::<f64>() {
        Ok(n) => format!("{n:.2}"),
        Err(_) => String::new(),
    }
}

// ---- tolerant JSON parsing (shared by every structured prompt) ----

fn parse_json<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, String> {
    let cleaned = strip_fences(text);
    match serde_json::from_str::<T>(&cleaned) {
        Ok(v) => Ok(v),
        Err(_) => {
            let repaired = repair_truncated_json(&cleaned).unwrap_or(cleaned);
            serde_json::from_str::<T>(&repaired).map_err(|e| e.to_string())
        }
    }
}

fn strip_fences(t: &str) -> String {
    let t = t.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        return rest.trim_end_matches("```").trim().to_string();
    }
    if let Some(start) = t.find(|c| c == '{' || c == '[') {
        let open = t.as_bytes()[start];
        let close = if open == b'{' { b'}' } else { b']' };
        let bytes = &t.as_bytes()[start..];
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        for (i, &b) in bytes.iter().enumerate() {
            if in_str {
                if esc { esc = false; }
                else if b == b'\\' { esc = true; }
                else if b == b'"' { in_str = false; }
                continue;
            }
            match b {
                b'"' => in_str = true,
                x if x == open => depth += 1,
                x if x == close => {
                    depth -= 1;
                    if depth == 0 {
                        return t[start..=start + i].to_string();
                    }
                }
                _ => {}
            }
        }
    }
    t.to_string()
}

/// Salvage a truncated JSON value: cut back to the last complete `}`/`]` outside
/// a string and close any still-open brackets.
fn repair_truncated_json(text: &str) -> Option<String> {
    let start = text.find(|c| c == '{' || c == '[')?;
    let s = &text[start..];
    let bytes = s.as_bytes();
    let mut in_str = false;
    let mut esc = false;
    let mut last_close: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if esc { esc = false; }
            else if b == b'\\' { esc = true; }
            else if b == b'"' { in_str = false; }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'}' | b']' => last_close = Some(i),
            _ => {}
        }
    }
    let end = last_close?;
    let head = &s[..=end];
    let mut stack: Vec<u8> = Vec::new();
    let mut in_str = false;
    let mut esc = false;
    for &b in head.as_bytes() {
        if in_str {
            if esc { esc = false; }
            else if b == b'\\' { esc = true; }
            else if b == b'"' { in_str = false; }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' => { stack.pop(); }
            _ => {}
        }
    }
    let mut out = head.to_string();
    while let Some(c) = stack.pop() {
        out.push(c as char);
    }
    Some(out)
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_answer_formats_two_decimals() {
        assert_eq!(normalize_answer("The answer is 15"), "15.00");
        assert_eq!(normalize_answer("42.5"), "42.50");
        assert_eq!(normalize_answer("= -3.14159 units"), "-3.14");
        assert_eq!(normalize_answer("no number here"), "");
    }

    #[test]
    fn strip_fences_extracts_object() {
        let t = "```json\n{\"a\":1}\n```";
        assert_eq!(strip_fences(t), "{\"a\":1}");
        let t2 = "sure! {\"upvotes\":[]} done";
        assert_eq!(strip_fences(t2), "{\"upvotes\":[]}");
    }

    #[test]
    fn parse_plan_tolerant_and_repaired() {
        let good = r#"{"upvotes":["a","b"],"comments":[{"post_id":"a","content":"hi","why":"x"}],"new_post":null,"note":"ok"}"#;
        let p: EngagementPlan = parse_json(good).unwrap();
        assert_eq!(p.upvotes.len(), 2);
        assert_eq!(p.comments.len(), 1);
        assert!(p.new_post.is_none());
        // truncated mid-object → repaired
        let bad = r#"{"upvotes":["a"],"comments":[{"post_id":"a","content":"hi"#;
        let p2: EngagementPlan = parse_json(bad).unwrap();
        assert_eq!(p2.upvotes, vec!["a"]);
    }
}
