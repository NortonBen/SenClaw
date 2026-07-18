//! The conversation pipeline. Channel-agnostic and *pure* with respect to
//! transport: `process_inbound` persists the turn, runs the bot, and RETURNS
//! the reply — the caller (a channel adapter / the WS handler) is responsible
//! for delivering it. This keeps `engine` free of any dependency on the
//! channel layer (no cycle).
//!
//! Flow:  persist user msg → (handoff? forward to operator, stop)
//!        → build context (history + knowledge.recall + session context)
//!        → tools allowed? agent.run with the bot's allowlist : llm.request
//!        → strip [HANDOFF] sentinel → persist reply → auto-ingest → return.

use crate::db::{Bot, Db, Session, HANDOFF_BOT, HANDOFF_PENDING};
use crate::{crm, llm, senclaw};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::broadcast;

const HISTORY_LIMIT: i64 = 12;
const AGENT_TIMEOUT_SECS: u64 = 240;
const MAX_TOKENS: u32 = 1200;
/// A bot emits this sentinel to request a human takeover.
const HANDOFF_SENTINEL: &str = "[HANDOFF]";
/// A bot emits this (followed by a JSON object) to log a support ticket while
/// continuing the conversation — mirrors the Go `report_customer_issue` tool.
const ISSUE_SENTINEL: &str = "[ISSUE]";

pub struct Outcome {
    /// Text to send back to the customer (None when a human now owns the chat).
    pub reply: Option<String>,
    /// The bot asked to escalate on this turn.
    pub escalated: bool,
}

/// Fields a bot supplies with an `[ISSUE]` sentinel.
struct IssueData {
    title: String,
    description: String,
    priority: String,
    category: String,
    sentiment: String,
    summary: String,
    tags: Vec<String>,
}

/// Result of one model turn.
struct AnswerResult {
    text: String,
    escalated: bool,
    issue: Option<IssueData>,
}

/// Knowledge-space id for a bot/session, honoring the bot's `knowledge_scope`.
pub fn knowledge_space(bot: &Bot, session: &Session) -> String {
    match bot.knowledge_scope.as_str() {
        "session" => format!("ai-chat:{}:sess:{}", bot.key, session.id),
        "user" => format!("ai-chat:{}:user:{}", bot.key, session.external_id),
        _ => format!("ai-chat:{}", bot.key),
    }
}

/// Broadcast a live event to the WS/Support-Inbox subscribers.
pub fn emit(events: &broadcast::Sender<String>, ev: serde_json::Value) {
    let _ = events.send(ev.to_string());
}

fn crm_enabled(db: &Arc<Db>) -> bool {
    db.get_setting("crm_enabled").ok().flatten().map(|v| v != "0").unwrap_or(true)
}

/// Generic display names a recognized customer should replace.
fn is_placeholder_name(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    n.is_empty() || ["khách web", "khách", "web"].contains(&n.as_str())
}

/// Once a CRM profile is known, rename the session to the real customer so every
/// surface (inbox list, conversation header, chat list) shows who's chatting
/// instead of the generic "Khách web".
fn apply_crm_name(db: &Arc<Db>, session: &Session, crm: &serde_json::Value) {
    if crm.get("none").is_some() {
        return;
    }
    let Some(name) = crm.get("name").and_then(|v| v.as_str()).map(str::trim).filter(|s| !s.is_empty())
    else {
        return;
    };
    if is_placeholder_name(&session.customer_name) {
        let _ = db.set_customer_name(session.id, name);
    }
}

/// Once per session, try to recognize the customer in the CRM and cache the
/// result in the session context (a hit stores the profile; a miss stores
/// `{none:true}` so we don't re-query every turn). Fully fail-safe.
async fn enrich_crm(db: &Arc<Db>, session: &Session) {
    if !crm_enabled(db) {
        return;
    }
    // Already looked up — just make sure the display name reflects the match
    // (also backfills sessions recognized before this behaviour existed).
    if let Some(crm) = session.context.get("crm") {
        apply_crm_name(db, session, crm);
        return;
    }
    // Only look up when we have a MEANINGFUL identity — a real display name or a
    // platform id. Anonymous web sessions (placeholder name + generated web id)
    // would otherwise fuzzy-match an unrelated CRM customer.
    if !crm::has_identity(&session.external_id, &session.customer_name) {
        let mut ctx = session.context.clone();
        if !ctx.is_object() {
            ctx = json!({});
        }
        ctx["crm"] = json!({ "none": true });
        let _ = db.set_session_context(session.id, &ctx);
        return;
    }
    // Base URL is auto-discovered from the daemon — no manual config.
    let base = crm::resolve_base().await;
    let crm_val = crm::lookup(&base, &session.external_id, &session.customer_name)
        .await
        .unwrap_or_else(|| json!({ "none": true }));
    apply_crm_name(db, session, &crm_val);
    let mut ctx = session.context.clone();
    if !ctx.is_object() {
        ctx = json!({});
    }
    ctx["crm"] = crm_val;
    let _ = db.set_session_context(session.id, &ctx);
}

/// Handle one inbound customer message end-to-end.
pub async fn process_inbound(
    db: &Arc<Db>,
    events: &broadcast::Sender<String>,
    bot: &Bot,
    session: &Session,
    text: &str,
) -> Outcome {
    let text = text.trim();
    if text.is_empty() {
        return Outcome { reply: None, escalated: false };
    }
    let _ = db.add_message(session.id, "user", text);
    emit(events, json!({ "type": "message", "sessionId": session.id, "role": "user", "content": text }));

    // CRM enrichment: recognize the customer and cache their profile + real
    // name on the session. Runs BEFORE the handoff check — an operator handling
    // a handed-off chat needs the profile most of all. Reload so we see it.
    enrich_crm(db, session).await;
    let session_owned = db.get_session(session.id).ok().flatten();
    let session = session_owned.as_ref().unwrap_or(session);

    // A human already owns this conversation — the bot stays silent; the
    // operator sees the message through the live event stream / inbox.
    if session.handoff_state != HANDOFF_BOT {
        return Outcome { reply: None, escalated: false };
    }

    let result = match answer(db, bot, session, text).await {
        Ok(v) => v,
        Err(e) => AnswerResult {
            text: format!("Xin lỗi, mình đang gặp trục trặc kỹ thuật ({e}). Bạn thử lại giúp mình nhé."),
            escalated: false,
            issue: None,
        },
    };

    if result.escalated {
        let _ = db.set_handoff(session.id, HANDOFF_PENDING);
        emit(events, json!({ "type": "handoff", "sessionId": session.id, "state": HANDOFF_PENDING }));
    }

    // The bot logged a support ticket but keeps assisting (unlike handoff).
    if let Some(iss) = result.issue {
        if let Ok(issue) = db.create_issue(
            Some(session.id),
            &bot.key,
            &session.external_id,
            &iss.title,
            &iss.description,
            &iss.priority,
            &iss.category,
            &iss.sentiment,
            &iss.summary,
            &iss.tags,
        ) {
            emit(events, json!({ "type": "issue", "sessionId": session.id, "issueId": issue.id, "title": issue.title, "priority": issue.priority }));
        }
    }

    let reply = result.text.trim().to_string();
    if !reply.is_empty() {
        let _ = db.add_message(session.id, "assistant", &reply);
        emit(events, json!({ "type": "message", "sessionId": session.id, "role": "assistant", "content": reply }));
        if bot.auto_ingest {
            // Fire-and-forget: fold the turn into the bot's knowledge space.
            let space = knowledge_space(bot, session);
            let doc = format!("Khách: {text}\nBot: {reply}");
            tokio::spawn(async move {
                let _ = senclaw::knowledge_save(&space, &doc, "chat-turn").await;
            });
        }
    }
    Outcome { reply: Some(reply), escalated: result.escalated }
}

/// Run the model for one turn.
async fn answer(db: &Arc<Db>, bot: &Bot, session: &Session, user_text: &str) -> Result<AnswerResult, String> {
    let space = knowledge_space(bot, session);

    // 1. Pre-retrieval: ground the answer in the bot's knowledge (engine-side,
    //    so even a zero-tool bot gets RAG). Best-effort.
    let mut context_block = String::new();
    if bot.use_knowledge {
        if let Ok(answer) = senclaw::knowledge_recall(&space, user_text, 5).await {
            if !answer.trim().is_empty() {
                context_block.push_str(&format!("\n\n## Kiến thức liên quan\n{}", answer.trim()));
            }
        }
    }

    // 2. Recognized customer profile from the CRM (if any).
    if let Some(crm) = session.context.get("crm").filter(|c| c.get("none").is_none()) {
        context_block.push_str(&format!("\n\n{}", crm::profile_block(crm)));
    }

    // 3. Current-chat context carried by the channel (page/cart/order…),
    //    excluding the reserved `crm` key handled above.
    if let Some(obj) = session.context.as_object().filter(|o| o.keys().any(|k| k != "crm")) {
        let pairs: Vec<String> = obj
            .iter()
            .filter(|(k, _)| k.as_str() != "crm")
            .map(|(k, v)| format!("- {k}: {v}"))
            .collect();
        if !pairs.is_empty() {
            context_block.push_str(&format!("\n\n## Bối cảnh hiện tại\n{}", pairs.join("\n")));
        }
    }

    // 3. System prompt = bot prompt + allowed-skill note + handoff instruction.
    let mut system = bot.system_prompt.clone();
    if system.trim().is_empty() {
        system = "Bạn là trợ lý hỗ trợ khách hàng.".into();
    }
    if !bot.allowed_skills.is_empty() {
        system.push_str(&format!(
            "\n\nBạn được phép dùng các kỹ năng (skill) sau khi cần: {}. Chỉ dùng đúng các skill này.",
            bot.allowed_skills.join(", ")
        ));
    }
    system.push_str(&format!(
        "\n\nNếu không thể giúp hoặc khách yêu cầu gặp người thật, hãy trả lời một câu lịch sự và thêm dòng cuối chính xác là \"{HANDOFF_SENTINEL}\" để chuyển cho nhân viên hỗ trợ."
    ));
    // Issue-logging guidance (mirrors report_customer_issue): the bot keeps
    // helping but flags a trackable problem for the support team.
    if bot.auto_issue {
        system.push_str(&format!(
            "\n\nNếu phát hiện khiếu nại, khách bất mãn, hoặc một vấn đề đáng theo dõi mà bạn không thể giải quyết trọn vẹn (hoàn tiền, lỗi sản phẩm, tranh chấp…), hãy VẪN tiếp tục hỗ trợ khách, đồng thời thêm vào CUỐI câu trả lời một dòng đúng định dạng: {ISSUE_SENTINEL}{{\"title\":\"...\",\"priority\":\"low|medium|high|urgent\",\"category\":\"...\",\"sentiment\":\"positive|neutral|negative\",\"summary\":\"...\"}}. Không tạo ticket cho câu hỏi thông thường bạn trả lời được, và không lặp lại ticket cho cùng một vấn đề."
        ));
    }
    system.push_str(&context_block);

    // 4. Build the user turn with recent history.
    let history = db.history(session.id, HISTORY_LIMIT).unwrap_or_default();
    let mut convo = String::new();
    for m in history.iter().filter(|m| m.id != 0) {
        let who = match m.role.as_str() {
            "user" => "Khách",
            "assistant" => "Bot",
            "operator" => "Nhân viên",
            _ => continue,
        };
        convo.push_str(&format!("{who}: {}\n", m.content));
    }
    let prompt = if convo.trim().is_empty() {
        user_text.to_string()
    } else {
        format!("Lịch sử hội thoại gần đây:\n{convo}\nKhách vừa nói: {user_text}\n\nTrả lời khách:")
    };

    // 5. Tool policy. use_tools + a non-empty allowlist → restricted agent.run
    //    (the daemon enforces EXACTLY the listed tools). Empty allowlist →
    //    never "all tools"; fall back to a plain, tool-free completion.
    let text = if bot.use_tools {
        let mut tools = bot.allowed_mcp.clone();
        if !bot.allowed_skills.is_empty() && !tools.iter().any(|t| t == "Skill") {
            tools.push("Skill".to_string());
        }
        if tools.is_empty() {
            let (t, model, _) = llm::bridge_llm(&system, &prompt, MAX_TOKENS).await?;
            record_llm(db, &model, &prompt, &t);
            t
        } else {
            let model = bot.model.as_str();
            let t = llm::agent_run(&system, &prompt, &space, &tools, Some(model).filter(|m| !m.is_empty()), AGENT_TIMEOUT_SECS).await?;
            record_llm(db, &bot.model, &prompt, &t);
            t
        }
    } else {
        let (t, model, _) = llm::bridge_llm(&system, &prompt, MAX_TOKENS).await?;
        record_llm(db, &model, &prompt, &t);
        t
    };

    // 6. Extract the [ISSUE]{json} ticket (if any), then strip both sentinels.
    let (issue, text) = extract_issue(&text);
    let escalated = text.contains(HANDOFF_SENTINEL);
    let clean = text.replace(HANDOFF_SENTINEL, "").trim().to_string();
    Ok(AnswerResult { text: clean, escalated, issue })
}

/// Pull an `[ISSUE]{json}` sentinel (expected last in the reply) out of the
/// text; returns the parsed issue (if present) and the reply with the sentinel
/// removed. Tolerant: if the JSON won't parse, the text after the marker
/// becomes the title.
fn extract_issue(text: &str) -> (Option<IssueData>, String) {
    let Some(pos) = text.find(ISSUE_SENTINEL) else {
        return (None, text.to_string());
    };
    let before = text[..pos].trim().to_string();
    let after = text[pos + ISSUE_SENTINEL.len()..].trim();
    let data = match serde_json::from_str::<serde_json::Value>(after) {
        Ok(v) => IssueData {
            title: v.get("title").and_then(|x| x.as_str()).unwrap_or("").trim().to_string(),
            description: v.get("description").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            priority: v.get("priority").and_then(|x| x.as_str()).unwrap_or("medium").to_string(),
            category: v.get("category").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            sentiment: v.get("sentiment").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            summary: v.get("summary").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            tags: v
                .get("tags")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect())
                .unwrap_or_default(),
        },
        Err(_) if !after.is_empty() => IssueData {
            title: after.chars().take(120).collect(),
            description: String::new(),
            priority: "medium".into(),
            category: String::new(),
            sentiment: String::new(),
            summary: after.to_string(),
            tags: Vec::new(),
        },
        Err(_) => return (None, before),
    };
    let data = if data.title.trim().is_empty() { None } else { Some(data) };
    (data, before)
}

/// Support-analysis of one conversation: run the daemon LLM over the transcript
/// to produce sentiment + a 1–5 quality score + a summary + suggested category.
/// Returns the parsed JSON object (best-effort; falls back to a raw wrapper).
pub async fn analyze_session(db: &Arc<Db>, session_id: i64) -> Result<serde_json::Value, String> {
    let messages = db.list_messages(session_id, 100).map_err(|e| e.to_string())?;
    if messages.is_empty() {
        return Err("phiên chưa có tin nhắn".into());
    }
    let transcript = messages
        .iter()
        .map(|m| {
            let who = match m.role.as_str() {
                "user" => "Khách",
                "assistant" => "Bot",
                "operator" => "Nhân viên",
                _ => "Hệ thống",
            };
            format!("{who}: {}", m.content)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let system = "Bạn là chuyên gia đánh giá chất lượng chăm sóc khách hàng (CSKH). \
Đọc hội thoại và trả về DUY NHẤT một JSON: \
{\"sentiment\":\"positive|neutral|negative\",\"quality\":<1-5>,\"resolved\":<true|false>,\"summary\":\"tóm tắt ngắn\",\"category\":\"nhãn ngắn\",\"suggestions\":\"gợi ý cải thiện ngắn\"}. \
Không thêm chữ nào ngoài JSON.";
    let (text, model, _) = llm::bridge_llm(system, &transcript, 500).await?;
    record_llm(db, &model, &transcript, &text);
    // The model may wrap the JSON in prose — extract the first {...} block.
    let json_str = match (text.find('{'), text.rfind('}')) {
        (Some(a), Some(b)) if b > a => &text[a..=b],
        _ => text.as_str(),
    };
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
        return Ok(v);
    }
    // Truncated/loose JSON (the active model sometimes cuts output mid-object):
    // salvage whatever fields are present so the UI still shows an assessment.
    let lenient = json!({
        "sentiment": field_str(&text, "sentiment"),
        "quality": field_num(&text, "quality"),
        "resolved": text.contains("\"resolved\":true") || text.contains("\"resolved\": true"),
        "summary": field_str(&text, "summary"),
        "category": field_str(&text, "category"),
        "suggestions": field_str(&text, "suggestions"),
    });
    if lenient.get("sentiment").map(|v| !v.is_null()).unwrap_or(false)
        || lenient.get("summary").map(|v| !v.is_null()).unwrap_or(false)
    {
        Ok(lenient)
    } else {
        Ok(json!({ "summary": text.trim(), "raw": true }))
    }
}

/// Pull `"key":"value"` out of a possibly-truncated JSON string.
fn field_str(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = text.find(&needle)? + needle.len();
    let rest = text[start..].trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"').unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// Pull `"key":<number>` out of a possibly-truncated JSON string.
fn field_num(text: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\"");
    let start = text.find(&needle)? + needle.len();
    let rest = text[start..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Rough token accounting for the stats panel (chars/4 heuristic).
fn record_llm(db: &Arc<Db>, model: &str, prompt: &str, reply: &str) {
    let _ = db.bump_metric("llm_calls", 1);
    let _ = db.bump_metric("tokens_in", (prompt.chars().count() / 4) as i64);
    let _ = db.bump_metric("tokens_out", (reply.chars().count() / 4) as i64);
    if !model.is_empty() {
        let _ = db.set_setting("last_model", model);
    }
}
