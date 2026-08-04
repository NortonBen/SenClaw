//! The proactive-sales engine, absorbed from the standalone AI Sale app.
//!
//! The split it was built on is kept exactly: the *decision* — send, queue for
//! review, escalate, churn — is deterministic Rust in [`crate::guardrail`] and
//! here; only the *wording* of a message is the LLM's job. The agent is never
//! handed a raw channel send, so the rules can't be talked around by a clever
//! prompt.
//!
//! [`send`] is the single chokepoint: every outbound byte, whoever asks —
//! scheduler, inbound reply, an approved review, MCP — goes through it.
//!
//! Two things changed in the merge, and both matter:
//!
//! 1. **No more `leads`.** Sales state is columns on `customers` and everything
//!    keys on `customer_id`. See [`crate::db_sale`].
//! 2. **`send` actually sends.** The original's `Gate::Send` branch wrote a
//!    `messages` row with status `sent` and transmitted nothing — a Phase-0
//!    placeholder that reported success for messages which never left the
//!    building. The CRM has a real channel layer, so a send that can't be routed
//!    is now an error, and one that fails in transit is recorded `failed`.

use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};
use tokio::sync::broadcast;

use crate::channels::ChannelManager;
use crate::db::{now_secs, Db};
use crate::db_inbox::Conversation;
use crate::db_sale::{Job, SaleState};
use crate::guardrail::{self, Gate};
use crate::{llm, senclaw};

const MAX_TOKENS: u32 = 700;
const HOUR_MS: i64 = 60 * 60 * 1000;
const DAY_MS: i64 = 24 * HOUR_MS;

/// The channel manager, for the paths that have no state handle to thread it
/// through — [`on_inbound`] is called from deep inside the poller. Set once at
/// startup by [`set_channels`] / [`spawn_scheduler`].
static CHANNELS: OnceLock<Arc<ChannelManager>> = OnceLock::new();

/// Wire the channel manager in. Idempotent: a second call is ignored, so the
/// startup path can call it and [`spawn_scheduler`] can too.
pub fn set_channels(c: Arc<ChannelManager>) {
    let _ = CHANNELS.set(c);
}

fn channels() -> Option<&'static Arc<ChannelManager>> {
    CHANNELS.get()
}

/// Read an i64 knob from env (test override) else a default (production).
///
/// The `SALE_*_MS` knobs keep the original's millisecond unit — the names and
/// meanings travelled with the operators who set them — and are converted to the
/// seconds this schema stores at the point of use.
fn knob(env_key: &str, default: i64) -> i64 {
    std::env::var(env_key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

/// What happened to one outbound attempt.
///
/// `Failed` is distinct from an `Err`: the message was routed and recorded (with
/// status `failed`), the platform just wouldn't take it. Callers that advance
/// state on success — [`run_job`] — must treat it as a failure, which is why
/// [`next_action`] folds it back into `Err`.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "action", rename_all = "lowercase")]
pub enum SendOutcome {
    Sent { message_id: i64 },
    Review { review_id: i64, reason: String },
    Blocked { reason: String },
    Failed { error: String },
}

impl SendOutcome {
    /// Stable tag for logs + API payloads: `sent|review|blocked|failed`.
    pub fn action(&self) -> &'static str {
        match self {
            SendOutcome::Sent { .. } => "sent",
            SendOutcome::Review { .. } => "review",
            SendOutcome::Blocked { .. } => "blocked",
            SendOutcome::Failed { .. } => "failed",
        }
    }

    /// Human-readable one-liner, for the action log and the API's `detail`.
    pub fn detail(&self) -> String {
        match self {
            SendOutcome::Sent { .. } => "đã gửi".to_string(),
            SendOutcome::Review { reason, .. } => reason.clone(),
            SendOutcome::Blocked { reason } => reason.clone(),
            SendOutcome::Failed { error } => error.clone(),
        }
    }

    pub fn is_sent(&self) -> bool {
        matches!(self, SendOutcome::Sent { .. })
    }

    pub fn review_id(&self) -> Option<i64> {
        match self {
            SendOutcome::Review { review_id, .. } => Some(*review_id),
            _ => None,
        }
    }

    pub fn message_id(&self) -> Option<i64> {
        match self {
            SendOutcome::Sent { message_id } => Some(*message_id),
            _ => None,
        }
    }

    /// Flat JSON for the REST + MCP surfaces: one shape for every variant, so a
    /// caller reads `action` and finds the ids where it expects them.
    pub fn to_json(&self) -> Value {
        json!({
            "action": self.action(),
            "detail": self.detail(),
            "reviewId": self.review_id(),
            "messageId": self.message_id(),
        })
    }
}

/// Publish a UI event. Thin alias over the API's helper, so every event on the
/// bus carries the same envelope.
pub fn emit(events: &broadcast::Sender<String>, kind: &str, payload: Value) {
    crate::api::emit(events, kind, payload);
}

// ---- the send chokepoint ----

/// The ONLY path to a customer's inbox. Runs the guardrail, then either delivers
/// over the real channel layer or diverts to the review queue.
///
/// `bypass_risky` is set only by [`approve_review`], where a human has read the
/// words. Unsubscribe and the rate limit still apply even then.
#[allow(clippy::too_many_arguments)]
pub async fn send(
    db: &Arc<Db>,
    events: &broadcast::Sender<String>,
    channels: &Arc<ChannelManager>,
    customer_id: i64,
    channel_kind: &str,
    text: &str,
    is_reply: bool,
    bypass_risky: bool,
) -> Result<SendOutcome, String> {
    let state = db
        .sale_state(customer_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("không có khách hàng {customer_id}"))?;
    let now = now_secs();

    match guardrail::gate(
        db,
        customer_id,
        state.unsubscribed,
        text,
        is_reply,
        bypass_risky,
        now,
    ) {
        Gate::Blocked(why) => {
            let _ = db.log_action(
                Some(customer_id),
                "blocked",
                &why,
                &json!([{ "tool": "sale_send", "result": why }]).to_string(),
                0,
                false,
                now,
            );
            Ok(SendOutcome::Blocked { reason: why })
        }
        Gate::Review(reason) => {
            let review_id = db
                .create_review(customer_id, text, channel_kind, &reason, now)
                .map_err(|e| e.to_string())?;
            let _ = db.log_action(
                Some(customer_id),
                "queue_review",
                &reason,
                &json!([{ "tool": "sale_send", "result": reason }]).to_string(),
                0,
                true,
                now,
            );
            emit(
                events,
                "review",
                json!({ "customerId": customer_id, "reviewId": review_id, "reason": reason }),
            );
            Ok(SendOutcome::Review { review_id, reason })
        }
        Gate::Send => {
            let (ch, conv) = resolve_route(db, &state, channel_kind, now)?;
            match channels.send_raw(&ch, &conv.external_id, text).await {
                Ok(()) => {
                    let message_id = db
                        .add_conv_message(conv.id, "outbound", "assistant", text, "sent", now)
                        .map_err(|e| e.to_string())?;
                    let _ = db.log_action(
                        Some(customer_id),
                        "send",
                        "",
                        &json!([{ "tool": "sale_send", "channel": channel_kind }]).to_string(),
                        0,
                        false,
                        now,
                    );
                    emit(
                        events,
                        "message",
                        json!({
                            "conversationId": conv.id,
                            "customerId": customer_id,
                            "channel": ch.kind,
                            "externalId": conv.external_id,
                            "direction": "outbound",
                            "role": "assistant",
                            "content": text,
                            "createdAt": now,
                        }),
                    );
                    Ok(SendOutcome::Sent { message_id })
                }
                Err(e) => {
                    // Record what we tried to say and that it didn't land. The
                    // row is `failed`, so it never counts against the rate limit
                    // and never reads as delivered.
                    let _ =
                        db.add_conv_message(conv.id, "outbound", "assistant", text, "failed", now);
                    let _ = db.log_action(
                        Some(customer_id),
                        "send_failed",
                        &e,
                        &json!([{ "tool": "sale_send", "channel": channel_kind, "error": e }])
                            .to_string(),
                        0,
                        false,
                        now,
                    );
                    Ok(SendOutcome::Failed { error: e })
                }
            }
        }
    }
}

/// Find the account to send from and the thread to send into.
///
/// Prefers a thread that already exists on this kind — reply where the
/// conversation lives. Failing that, an enabled account of the kind plus an
/// identity the customer has claimed is enough to open one. If neither holds
/// there is no route, and that is an error: the original wrote a `sent` row
/// anyway, which is the one lie this port exists to remove.
fn resolve_route(
    db: &Arc<Db>,
    state: &SaleState,
    kind: &str,
    now: i64,
) -> Result<(crate::db_inbox::Channel, Conversation), String> {
    let existing = db
        .list_conversations(None, Some(kind), Some(state.customer_id), None, 1)
        .map_err(|e| e.to_string())?
        .into_iter()
        .next();

    if let Some(conv) = existing {
        // A thread seeded before any account was wired carries `channel_id = 0`;
        // fall back to whichever enabled account of the kind exists.
        let ch = match db.get_channel(conv.channel_id).map_err(|e| e.to_string())? {
            Some(c) => c,
            None => db
                .channel_of_kind(kind)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("không có kênh '{kind}' đang bật để gửi"))?,
        };
        return Ok((ch, conv));
    }

    let ch = db
        .channel_of_kind(kind)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("không có kênh '{kind}' đang bật để gửi"))?;
    let identity = db
        .list_channels(state.customer_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.kind == kind && !c.value.trim().is_empty())
        .ok_or_else(|| {
            format!(
                "khách hàng {} chưa có định danh '{kind}' để gửi",
                state.customer_id
            )
        })?;
    let conv = db
        .get_or_create_conversation(ch.id, kind, identity.value.trim(), &state.name, now)
        .map_err(|e| e.to_string())?;
    Ok((ch, conv))
}

/// Which channel to reach this customer on when the caller didn't say.
///
/// The thread they last spoke in, else an identity they've claimed that we have
/// an enabled account for. `None` when there's no route at all — the caller
/// reports that rather than drafting a message with nowhere to go.
fn preferred_kind(db: &Arc<Db>, customer_id: i64) -> Option<String> {
    if let Ok(convs) = db.list_conversations(None, None, Some(customer_id), None, 1) {
        if let Some(c) = convs.into_iter().next() {
            return Some(c.channel_kind);
        }
    }
    let identities = db.list_channels(customer_id).ok()?;
    identities
        .into_iter()
        .find(|i| matches!(db.channel_of_kind(&i.kind), Ok(Some(_))))
        .map(|i| i.kind)
}

// ---- drafting ----

/// The grounding block for a draft: sales state, CRM profile, long-term memory,
/// product wiki, recent transcript. Everything the model is allowed to know.
async fn build_context(db: &Arc<Db>, state: &SaleState, query: &str) -> String {
    let mut ctx = format!(
        "## Trạng thái bán hàng\n- Tên: {}\n- Giai đoạn: {}\n- Nhiệt độ: {}\n- Điểm: {}\n- Tín hiệu: {}\n",
        if state.name.trim().is_empty() { "(chưa rõ)" } else { state.name.trim() },
        state.sale_stage,
        state.temperature,
        state.lead_score,
        state.intent_signals.join(", "),
    );

    // The CRM profile — in-process, so this is the real record rather than the
    // company/notes pair the old app scraped back over HTTP.
    if let Ok(Some(_)) = db.get_customer(state.customer_id) {
        if let Ok(profile) = db.compact_context(state.customer_id) {
            if !profile.trim().is_empty() {
                ctx.push_str(&format!("\n## Hồ sơ CRM\n{}\n", profile.trim()));
            }
        }
    }

    let space = senclaw::lead_space(state.customer_id);
    if let Ok(recall) = senclaw::knowledge_recall(&space, query, 5).await {
        if !recall.trim().is_empty() {
            ctx.push_str(&format!("\n## Trí nhớ về khách\n{}\n", recall.trim()));
        }
    }
    if let Ok(product) = senclaw::wiki_search(query).await {
        if !product.trim().is_empty() {
            ctx.push_str(&format!(
                "\n## Kiến thức sản phẩm (tham khảo)\n{}\n",
                product.trim()
            ));
        }
    }

    let history = db
        .recent_messages_of_customer(state.customer_id, 10)
        .unwrap_or_default();
    if !history.is_empty() {
        ctx.push_str("\n## Hội thoại gần đây\n");
        for m in &history {
            let who = if m.direction == "inbound" {
                "Khách"
            } else {
                "Mình"
            };
            ctx.push_str(&format!("{}: {}\n", who, m.content));
        }
    }
    ctx
}

fn draft_system(db: &Arc<Db>, intent: &str) -> String {
    // Seeded empty, so `setting_or`'s default never fires — fall back on a blank
    // value too, or the model gets `Brand voice:` and nothing after it.
    let configured = db.setting_or("brand_voice", "");
    let brand = if configured.trim().is_empty() {
        "Ấm áp, chuyên nghiệp, xưng mình – anh/chị."
    } else {
        configured.trim()
    };
    format!(
        "Bạn là trợ lý bán hàng (AI chốt sale) của doanh nghiệp, chăm sóc khách để tiến tới chốt đơn.\n\
         Brand voice: {brand}\n\n\
         QUY TẮC BẮT BUỘC:\n\
         - KHÔNG bịa giá, khuyến mãi, deadline, case study hay cam kết. Khi cần nói về giá/hợp đồng, \
         nói sẽ nhờ tư vấn viên báo lại — không tự đưa con số.\n\
         - Cá nhân hoá theo ngữ cảnh; ngắn gọn (<120 từ), đi thẳng vào giá trị; không sáo rỗng, không spam.\n\
         - Chỉ xuất NỘI DUNG tin nhắn gửi khách. KHÔNG giải thích, KHÔNG markdown, KHÔNG JSON.\n\n\
         Mục tiêu lần này (intent): {intent}"
    )
}

/// Draft-only (for previews and the review flow). Returns the raw message text.
pub async fn draft_message(db: &Arc<Db>, customer_id: i64, intent: &str) -> Result<String, String> {
    let state = db
        .sale_state(customer_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("không có khách hàng {customer_id}"))?;
    let ctx = build_context(db, &state, intent).await;
    let system = draft_system(db, intent);
    let prompt = format!("{ctx}\n\nSoạn một tin nhắn follow-up cho khách.");
    let (text, model) = llm::bridge_llm(&system, &prompt, MAX_TOKENS).await?;
    let text = text.trim().to_string();
    let _ = db.log_action(
        Some(customer_id),
        "draft",
        &format!("intent={intent} model={model}"),
        &json!([{ "tool": "draft_message", "intent": intent }]).to_string(),
        // Crude but free: a token is ~4 chars. Enough for a spend trend line.
        (prompt.chars().count() / 4) as i64,
        false,
        now_secs(),
    );
    Ok(text)
}

/// One proactive turn: draft with the given intent, then route it through the
/// guardrail. `channel` overrides the auto-picked one when an operator names it;
/// `None` falls back to [`preferred_kind`].
///
/// Returns the outcome as JSON with the `draft` folded in, so a caller can show
/// what was said — or what got queued — without a second round trip.
///
/// A transport failure comes back as `Err` even though [`send`] reports it as
/// `Failed` — callers advance sequence state on `Ok`, and a message that didn't
/// land must not advance anything.
pub async fn next_action(
    db: &Arc<Db>,
    events: &broadcast::Sender<String>,
    channels: &Arc<ChannelManager>,
    customer_id: i64,
    intent: &str,
    channel: Option<&str>,
) -> Result<Value, String> {
    let state = db
        .sale_state(customer_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("không có khách hàng {customer_id}"))?;
    if state.unsubscribed {
        return Err("khách đã hủy nhận tin".into());
    }
    let kind = match channel.map(|c| c.trim()).filter(|c| !c.is_empty()) {
        Some(c) => c.to_string(),
        None => preferred_kind(db, customer_id)
            .ok_or_else(|| format!("khách hàng {customer_id} chưa có kênh nào để liên hệ"))?,
    };

    let draft = draft_message(db, customer_id, intent).await?;
    if draft.is_empty() {
        return Err("mô hình trả về rỗng".into());
    }
    let outcome = send(
        db,
        events,
        channels,
        customer_id,
        &kind,
        &draft,
        false,
        false,
    )
    .await?;
    if let SendOutcome::Failed { error } = &outcome {
        return Err(error.clone());
    }
    // Fold the turn into long-term memory (best effort).
    if outcome.is_sent() {
        let space = senclaw::lead_space(customer_id);
        let memo = format!("Đã chủ động gửi ({intent}): {draft}");
        tokio::spawn(async move {
            let _ = senclaw::knowledge_save(&space, &memo, "sale-turn").await;
        });
    }
    let mut out = outcome.to_json();
    out["draft"] = json!(draft);
    out["channel"] = json!(kind);
    Ok(out)
}

// ---- inbound ----

/// Handle one inbound customer message. The channel layer calls this for every
/// message it lands; the message row and its event are already written by then,
/// so this only decides what happens next.
///
/// Complaint → escalate to a human and say nothing. Otherwise draft a reply and
/// route it through the guardrail, at the stricter reply threshold.
pub async fn on_inbound(
    db: &Arc<Db>,
    events: &broadcast::Sender<String>,
    conv: &Conversation,
    text: &str,
) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    // An unlinked thread has no sales state to drive and nobody to be
    // accountable to. Leave it for a human to attach to a customer.
    if conv.customer_id == 0 {
        return;
    }
    let customer_id = conv.customer_id;
    let now = now_secs();

    // A reply is a strong engagement signal: it resets the silence clock and
    // warms the lead.
    let _ = db.mark_inbound(customer_id, now);
    let inbound_n = db.count_inbound(customer_id).unwrap_or(1);
    let temp = if inbound_n >= 3 { "hot" } else { "warm" };
    let _ = db.bump_score(customer_id, 8, Some(temp), now);

    // Complaint → escalate immediately, no auto-reply. Deliberately ahead of the
    // channel check: an angry customer must reach a human even if we currently
    // have no way to write back.
    let complaint = guardrail::detect_complaint(db, text);
    if !complaint.is_empty() {
        let context = json!({ "matched": complaint, "message": text }).to_string();
        let esc = db
            .create_escalation(customer_id, "complaint", &context, "", now)
            .ok();
        let _ = db.log_action(
            Some(customer_id),
            "escalate",
            "complaint",
            &json!([{ "tool": "on_inbound", "reason": "complaint" }]).to_string(),
            0,
            true,
            now,
        );
        emit(
            events,
            "escalation",
            json!({
                "customerId": customer_id,
                "conversationId": conv.id,
                "escalationId": esc,
                "reason": "complaint",
            }),
        );
        return;
    }

    let Some(channels) = channels() else {
        eprintln!(
            "crm/sale: channel manager chưa được wire — bỏ qua trả lời tự động cho khách {customer_id}"
        );
        return;
    };

    let draft = match draft_message(db, customer_id, "reply_to_customer").await {
        Ok(d) if !d.is_empty() => d,
        Ok(_) => return,
        Err(e) => {
            eprintln!("crm/sale: soạn trả lời cho khách {customer_id} thất bại: {e}");
            return;
        }
    };
    if let Err(e) = send(
        db,
        events,
        channels,
        customer_id,
        &conv.channel_kind,
        &draft,
        true,
        false,
    )
    .await
    {
        eprintln!("crm/sale: gửi trả lời cho khách {customer_id} thất bại: {e}");
    }
}

// ---- review queue ----

/// Approve a queued review and send it. The guardrail's risky rule steps aside —
/// a human read the words — but unsubscribe and the rate limit do not.
pub async fn approve_review(
    db: &Arc<Db>,
    events: &broadcast::Sender<String>,
    channels: &Arc<ChannelManager>,
    review_id: i64,
    edited: Option<&str>,
    by: &str,
) -> Result<Value, String> {
    let review = db
        .get_review(review_id)
        .map_err(|e| e.to_string())?
        .ok_or("không có review")?;
    if review.status != "pending" {
        return Err(format!("review đã {}", review.status));
    }
    let edited = edited.filter(|s| !s.trim().is_empty());
    let content = edited.unwrap_or(&review.draft);
    let outcome = send(
        db,
        events,
        channels,
        review.customer_id,
        &review.channel,
        content,
        false,
        true,
    )
    .await?;
    let status = if edited.is_some() {
        "edited"
    } else {
        "approved"
    };
    let _ = db.resolve_review(review_id, status, edited.unwrap_or(""), by, now_secs());
    Ok(json!({
        "ok": outcome.is_sent(),
        "action": outcome.action(),
        "detail": outcome.detail(),
    }))
}

// ---- sequences ----

fn step_field<'a>(step: &'a Value, key: &str) -> Option<&'a Value> {
    step.get(key)
}

/// Start a follow-up sequence: create the run and enqueue step 0.
///
/// Refuses if a run of the same sequence is already active — two live chains of
/// one sequence means the customer gets every message twice.
pub async fn start_sequence(
    db: &Arc<Db>,
    events: &broadcast::Sender<String>,
    customer_id: i64,
    sequence_key: &str,
) -> Result<i64, String> {
    let state = db
        .sale_state(customer_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("không có khách hàng {customer_id}"))?;
    if state.unsubscribed {
        return Err("khách đã hủy nhận tin".into());
    }
    if db
        .has_active_run(customer_id, sequence_key)
        .map_err(|e| e.to_string())?
    {
        return Err(format!("khách đã đang trong chuỗi '{sequence_key}'"));
    }
    let steps = db.sequence_steps(sequence_key).map_err(|e| e.to_string())?;
    if steps.is_empty() {
        return Err(format!("chuỗi '{sequence_key}' không tồn tại hoặc rỗng"));
    }
    let now = now_secs();
    let run_id = db
        .create_sequence_run(customer_id, sequence_key, now)
        .map_err(|e| e.to_string())?;
    let delay_h = step_field(&steps[0], "delay_hours")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let run_at = now + delay_h * 3600;
    db.enqueue_job(
        customer_id,
        "sequence_step",
        run_at,
        &json!({ "sequence_run_id": run_id, "sequence_key": sequence_key, "step": 0 }).to_string(),
        now,
    )
    .map_err(|e| e.to_string())?;
    let _ = db.log_action(
        Some(customer_id),
        "start_sequence",
        sequence_key,
        &json!([{ "tool": "start_sequence", "key": sequence_key }]).to_string(),
        0,
        false,
        now,
    );
    emit(
        events,
        "sequence",
        json!({ "customerId": customer_id, "runId": run_id, "key": sequence_key }),
    );
    Ok(run_id)
}

/// Enroll a customer into the welcome sequence — the entry point of the
/// automatic flow. Called on capture unless `auto_welcome` is off.
pub async fn enroll_welcome(db: &Arc<Db>, events: &broadcast::Sender<String>, customer_id: i64) {
    if db.setting_or("auto_welcome", "1") == "0" {
        return;
    }
    // Every error here is a correct refusal — unsubscribed, or already enrolled.
    let _ = start_sequence(db, events, customer_id, "welcome").await;
}

// ---- the scheduler ----

/// Process one due job. Errors are non-fatal but never silent: an infra failure
/// (LLM down, channel refused) marks the job `failed` WITHOUT advancing the
/// sequence, so the step is still there to retry rather than quietly skipped.
pub async fn run_job(
    db: &Arc<Db>,
    events: &broadcast::Sender<String>,
    channels: &Arc<ChannelManager>,
    job: Job,
) {
    let now = now_secs();
    let Some(state) = db.sale_state(job.customer_id).ok().flatten() else {
        let _ = db.mark_job(job.id, "done", "khách không còn", now);
        return;
    };
    if state.unsubscribed || state.sale_stage == "churned" {
        let _ = db.mark_job(job.id, "done", "khách đã hủy nhận tin/churned", now);
        return;
    }

    let payload: Value = serde_json::from_str(&job.payload).unwrap_or_else(|_| json!({}));

    // Resolve this job's intent: from the sequence definition for a sequence
    // step, else straight off an ad-hoc payload.
    let (intent, seq): (String, Option<(i64, String, i64)>) = if job.job_type == "sequence_step" {
        let run_id = payload["sequence_run_id"].as_i64().unwrap_or(0);
        let key = payload["sequence_key"].as_str().unwrap_or("").to_string();
        let step = payload["step"].as_i64().unwrap_or(0);
        let steps = db.sequence_steps(&key).unwrap_or_default();
        let intent = steps
            .get(step as usize)
            .and_then(|s| step_field(s, "intent"))
            .and_then(|v| v.as_str())
            .unwrap_or("share_value_content")
            .to_string();
        (intent, Some((run_id, key, step)))
    } else {
        (
            payload["intent"]
                .as_str()
                .unwrap_or("share_value_content")
                .to_string(),
            None,
        )
    };

    match next_action(db, events, channels, job.customer_id, &intent, None).await {
        Ok(_) => {
            let _ = db.mark_job(job.id, "done", "", now_secs());
            // Advance the sequence: enqueue the next step, or complete the run.
            if let Some((run_id, key, step)) = seq {
                let steps = db.sequence_steps(&key).unwrap_or_default();
                let next = step + 1;
                if let Some(next_step) = steps.get(next as usize) {
                    let delay_h = step_field(next_step, "delay_hours")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(24);
                    let run_at = now_secs() + delay_h * 3600;
                    let _ = db.advance_run(run_id, next, "active", now_secs());
                    let _ = db.enqueue_job(
                        job.customer_id,
                        "sequence_step",
                        run_at,
                        &json!({ "sequence_run_id": run_id, "sequence_key": key, "step": next })
                            .to_string(),
                        now_secs(),
                    );
                } else {
                    let _ = db.advance_run(run_id, step, "completed", now_secs());
                }
            }
        }
        Err(e) => {
            // Infra failure — do not advance; leave the sequence resumable.
            let _ = db.mark_job(job.id, "failed", &e, now_secs());
        }
    }
}

/// The automatic re-engagement pass: for every customer who has gone quiet past
/// the inactivity threshold (and the check-in cooldown), either fire another
/// check-in or — after `max_checkins` with no reply — let them go.
///
/// This is what makes the flow self-driving with zero manual input.
pub async fn check_inactive_pass(db: &Arc<Db>, events: &broadcast::Sender<String>) {
    let inactive_ms = knob("SALE_INACTIVE_MS", 3 * DAY_MS);
    let cooldown_ms = knob("SALE_CHECKIN_COOLDOWN_MS", 7 * DAY_MS);
    let max_checkins = knob("SALE_MAX_CHECKINS", 2);
    let now = now_secs();
    // The knobs are millis (the original's unit, and what operators have set);
    // this schema is seconds.
    let inactive_before = now - inactive_ms / 1000;
    let cooldown_before = now - cooldown_ms / 1000;

    let ids = match db.leads_for_checkin(inactive_before, cooldown_before) {
        Ok(l) => l,
        Err(_) => return,
    };
    for id in ids {
        let Some(state) = db.sale_state(id).ok().flatten() else {
            continue;
        };
        if state.checkin_count >= max_checkins {
            // Silent through the whole check-in streak → give up gracefully.
            let _ = db.update_sale_stage(id, Some("churned"), Some("churned"), None, now);
            let _ = db.log_action(
                Some(id),
                "auto_churn",
                &format!(
                    "im lặng, {} lần check-in không hồi đáp",
                    state.checkin_count
                ),
                &json!([{ "tool": "check_inactive" }]).to_string(),
                0,
                false,
                now,
            );
            emit(
                events,
                "lead",
                json!({ "customerId": id, "stage": "churned" }),
            );
        } else {
            // Enqueue an immediate re-engage touch; the scheduler drafts + sends it.
            let _ = db.enqueue_job(
                id,
                "followup",
                now,
                &json!({ "intent": "re_engage_soft", "auto": "checkin" }).to_string(),
                now,
            );
            let _ = db.mark_checkin(id, now);
            let _ = db.log_action(
                Some(id),
                "auto_checkin",
                &format!("khách im lặng, check-in lần #{}", state.checkin_count + 1),
                &json!([{ "tool": "check_inactive" }]).to_string(),
                0,
                false,
                now,
            );
            emit(events, "checkin", json!({ "customerId": id }));
        }
    }
}

/// The heartbeat of the automatic flow: poll due jobs, run them, then sweep for
/// customers who have gone quiet.
pub async fn scheduler_loop(
    db: Arc<Db>,
    events: broadcast::Sender<String>,
    channels: Arc<ChannelManager>,
) {
    let interval_secs: u64 = std::env::var("SALE_SCHED_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    // A job left `running` by a crash is stranded otherwise — nothing else ever
    // revisits that state.
    if let Ok(n) = db.requeue_stuck_jobs() {
        if n > 0 {
            eprintln!("crm/sale: đã trả {n} job kẹt ở trạng thái `running` về hàng đợi");
        }
    }
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
        // 1. Run due follow-up jobs (sequence steps + ad-hoc + auto check-ins).
        if let Ok(due) = db.due_jobs(now_secs(), 20) {
            for job in due {
                let _ = db.mark_job(job.id, "running", "", now_secs());
                run_job(&db, &events, &channels, job).await;
            }
        }
        // 2. Sweep silent customers → auto check-in / auto churn.
        check_inactive_pass(&db, &events).await;
    }
}

/// Spawn the scheduler, wiring the channel manager in on the way past.
pub fn spawn_scheduler(
    db: Arc<Db>,
    events: broadcast::Sender<String>,
    channels: Arc<ChannelManager>,
) {
    set_channels(channels.clone());
    tokio::spawn(async move { scheduler_loop(db, events, channels).await });
}
