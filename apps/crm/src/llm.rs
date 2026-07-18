//! Thin bridge to the daemon's active LLM via app-space-sdk. The CRM app never
//! calls an LLM provider directly — every call goes through SenClaw.

use app_space_sdk::SpaceClient;
use serde_json::{json, Value};

use crate::db::{Customer, Deal, Interaction, Task};

fn client() -> SpaceClient {
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "crm");
    }
    SpaceClient::from_env()
}

async fn bridge(system: &str, user: &str, max_tokens: u32) -> Result<(String, String), String> {
    client()
        .llm_request(system, user, max_tokens)
        .await
        .map_err(|e| e.to_string())
}

/// One-shot completion on the daemon's active model. Returns `(text, model)`.
///
/// Every prompt in this file is a fixed CRM task with its own system prompt;
/// this is the escape hatch for callers that compose their own — the sales
/// engine, whose system prompt is assembled from the brand-voice setting.
pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<(String, String), String> {
    bridge(system, user, max_tokens).await
}

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

const SUMMARY_SYSTEM: &str = "You are the CRM assistant. Given a customer profile and the most \
recent interactions, produce a short briefing in the SAME language as the input: \
(1) one sentence summarising who they are, (2) 2-4 bullet points of the latest activity in \
reverse-chronological order, (3) a one-line 'Next step' recommendation. Plain markdown only \
— no preface, no code fences.";

/// Compose a briefing for a customer + their recent interactions.
pub async fn summarize(customer: &Customer, interactions: &[Interaction]) -> Result<(String, String), String> {
    let mut prompt = String::new();
    prompt.push_str("Customer profile:\n");
    prompt.push_str(&format!("- Name: {}\n", customer.name));
    if !customer.company.is_empty() {
        prompt.push_str(&format!("- Company: {}\n", customer.company));
    }
    if !customer.title.is_empty() {
        prompt.push_str(&format!("- Title: {}\n", customer.title));
    }
    if !customer.email.is_empty() {
        prompt.push_str(&format!("- Email: {}\n", customer.email));
    }
    if !customer.phone.is_empty() {
        prompt.push_str(&format!("- Phone: {}\n", customer.phone));
    }
    if !customer.role.is_empty() {
        prompt.push_str(&format!("- Role: {}\n", customer.role));
    }
    if !customer.tags.is_empty() {
        prompt.push_str(&format!("- Tags: {}\n", customer.tags.join(", ")));
    }
    if !customer.notes.trim().is_empty() {
        prompt.push_str(&format!("- Notes: {}\n", customer.notes.trim()));
    }
    prompt.push_str("\nRecent interactions (newest first):\n");
    if interactions.is_empty() {
        prompt.push_str("(none yet)\n");
    } else {
        for i in interactions.iter().take(20) {
            let ts = format_ts(i.occurred_at);
            let det = if i.details.trim().is_empty() {
                String::new()
            } else {
                format!(" — {}", truncate(i.details.trim(), 200))
            };
            prompt.push_str(&format!("- [{}] {}: {}{}\n", ts, i.kind, i.summary, det));
        }
    }
    prompt.push_str("\nWrite the briefing now.");
    bridge(SUMMARY_SYSTEM, &prompt, 700).await
}

const NEXT_SYSTEM: &str = "You suggest ONE concrete next action for a CRM user to take with \
this customer. Return one short sentence in the SAME language as the input, starting with a \
verb. No preamble, no bullets, no code fences.";

pub async fn suggest_next_step(customer: &Customer, interactions: &[Interaction]) -> Result<(String, String), String> {
    let mut prompt = String::from("Customer:\n");
    prompt.push_str(&format!("{} ({}) — {}\n", customer.name, customer.role, customer.company));
    if !customer.tags.is_empty() {
        prompt.push_str(&format!("Tags: {}\n", customer.tags.join(", ")));
    }
    if !customer.notes.trim().is_empty() {
        prompt.push_str(&format!("Notes: {}\n", truncate(customer.notes.trim(), 400)));
    }
    prompt.push_str("\nRecent interactions:\n");
    if interactions.is_empty() {
        prompt.push_str("(none yet — this is a fresh lead)\n");
    } else {
        for i in interactions.iter().take(10) {
            prompt.push_str(&format!("- [{}] {}: {}\n", format_ts(i.occurred_at), i.kind, i.summary));
        }
    }
    prompt.push_str("\nSuggest the single next action.");
    bridge(NEXT_SYSTEM, &prompt, 120).await
}

const REPORT_SYSTEM: &str = "You are the CRM analyst. Given a snapshot of a personal CRM — \
totals, per-stage pipeline, top open deals, most recently active customers, upcoming birthdays \
and overdue tasks — write an executive briefing in the SAME language as the input. Format \
(strict markdown, no code fences, no preface):\n\
**Tổng quan:** one sentence with the key numbers (customers, pipeline value, hot deals).\n\
**Highlights:**\n- 3-5 bullets naming specific customers/deals/activity that stand out.\n\
**Cần chú ý:** 1-3 bullets flagging risks or overdue items (skip the section if there are none).\n\
**Đề xuất:** one sentence with the single most impactful next action.\n\
Be concrete, refer to names/amounts stored in the snapshot — do not invent facts. Keep the \
whole reply under ~180 words.";

/// A snapshot of everything the aggregate report grounds on. Kept as a plain
/// struct so the API layer can hand-craft it without pulling more DB helpers.
pub struct ReportSnapshot<'a> {
    pub stats: &'a Value,
    pub top_deals: &'a [Deal],
    pub top_active_customers: &'a [(Customer, i64)],
    pub recent_activity: &'a [Value],
    pub upcoming: &'a Value,
    pub overdue_tasks: &'a [Task],
}

/// Generate the AI aggregate report. Returns (markdown, model).
pub async fn aggregate_report(snap: &ReportSnapshot<'_>) -> Result<(String, String), String> {
    let mut prompt = String::new();

    prompt.push_str("Snapshot (");
    prompt.push_str(&iso_today());
    prompt.push_str("):\n");

    // Totals — pluck them out so the LLM has a rock-solid grounding to cite.
    let s = snap.stats;
    prompt.push_str(&format!(
        "- Customers: {}\n- Open deals: {} (pipeline value {})\n- Won deals value: {}\n- Open tasks: {} (overdue {})\n",
        s.get("customers").and_then(|v| v.as_i64()).unwrap_or(0),
        s.get("open_deals").and_then(|v| v.as_i64()).unwrap_or(0),
        fmt_money(s.get("pipeline_value").and_then(|v| v.as_f64()).unwrap_or(0.0)),
        fmt_money(s.get("won_value").and_then(|v| v.as_f64()).unwrap_or(0.0)),
        s.get("open_tasks").and_then(|v| v.as_i64()).unwrap_or(0),
        s.get("overdue_tasks").and_then(|v| v.as_i64()).unwrap_or(0),
    ));

    if let Some(by_stage) = s.get("by_stage").and_then(|v| v.as_object()) {
        if !by_stage.is_empty() {
            prompt.push_str("Pipeline by stage:\n");
            for (stage, info) in by_stage {
                let count = info.get("count").and_then(|v| v.as_i64()).unwrap_or(0);
                let value = info.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
                prompt.push_str(&format!("- {stage}: {count} deals ({})\n", fmt_money(value)));
            }
        }
    }

    if !snap.top_deals.is_empty() {
        prompt.push_str("\nTop open deals (by value):\n");
        for d in snap.top_deals.iter().take(5) {
            prompt.push_str(&format!(
                "- \"{}\" · {} · {} · {}% · {}\n",
                d.title,
                d.customer_name,
                fmt_money_currency(d.amount, &d.currency),
                d.probability,
                d.stage,
            ));
        }
    }

    if !snap.top_active_customers.is_empty() {
        prompt.push_str("\nMost active customers (by interaction count):\n");
        for (c, n) in snap.top_active_customers.iter().take(5) {
            let tags = if c.tags.is_empty() { String::new() } else { format!(" · tags: {}", c.tags.join(",")) };
            prompt.push_str(&format!(
                "- {} ({}) · {} interactions · role {}{}\n",
                c.name, c.company, n, c.role, tags,
            ));
        }
    }

    if !snap.recent_activity.is_empty() {
        prompt.push_str("\nMost recent activity:\n");
        for a in snap.recent_activity.iter().take(8) {
            let cust = a.get("customer_name").and_then(|v| v.as_str()).unwrap_or("?");
            let kind = a.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
            let summary = a.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            prompt.push_str(&format!("- [{kind}] {cust}: {summary}\n"));
        }
    }

    if let Some(birthdays) = snap.upcoming.get("birthdays").and_then(|v| v.as_array()) {
        if !birthdays.is_empty() {
            prompt.push_str("\nUpcoming birthdays:\n");
            for b in birthdays.iter().take(5) {
                let name = b.get("customer_name").and_then(|v| v.as_str()).unwrap_or("?");
                let bday = b.get("birthday").and_then(|v| v.as_str()).unwrap_or("");
                prompt.push_str(&format!("- {name} — {bday}\n"));
            }
        }
    }

    if !snap.overdue_tasks.is_empty() {
        prompt.push_str("\nOverdue tasks:\n");
        for t in snap.overdue_tasks.iter().take(5) {
            let who = t.customer_name.as_deref().unwrap_or("no customer");
            prompt.push_str(&format!("- \"{}\" · {who}\n", t.title));
        }
    }

    prompt.push_str("\nWrite the briefing now.");
    bridge(REPORT_SYSTEM, &prompt, 900).await
}

/// Compact money for prompts. Uses a bare number + "VND" suffix so the LLM
/// gets consistent units regardless of the source currency.
fn fmt_money(n: f64) -> String {
    if n >= 1_000_000_000.0 {
        format!("{:.1}B", n / 1_000_000_000.0)
    } else if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.0}k", n / 1_000.0)
    } else {
        format!("{n:.0}")
    }
}

fn fmt_money_currency(n: f64, currency: &str) -> String {
    format!("{} {currency}", fmt_money(n))
}

fn iso_today() -> String {
    format_ts(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0))
}

const EXTRACT_SYSTEM: &str = "You extract a knowledge graph from a customer's stored CRM \
context (their profile + notes + interactions). Identify every OTHER person mentioned by name \
and the relationship implied between them and the customer. Return ONLY valid JSON, no prose \
and no code fences, in exactly this shape:\n\
{\"people\":[{\"name\":\"...\",\"role_guess\":\"contact|referrer|colleague|partner|spouse|family|friend|reports_to|supplier|competitor\",\"kind\":\"referred_by|introduced_by|colleague_of|spouse_of|family_of|friend_of|reports_to|partner_of|supplier_of|competitor_of|contact_of\",\"context\":\"the exact quoted snippet that mentions them\",\"confidence\":0.0-1.0}]}\n\
Rules: Only include people who APPEAR in the input text. Do NOT invent. If none, return \
{\"people\":[]}. The customer themselves is NOT in the list. Keep 'context' under 140 chars. \
`kind` describes how the customer relates to the mentioned person (customer -> person).";

/// LLM output of the extract-graph step.
#[derive(serde::Deserialize)]
pub struct ExtractedPeople {
    #[serde(default)]
    pub people: Vec<ExtractedPerson>,
}
#[derive(serde::Deserialize)]
pub struct ExtractedPerson {
    pub name: String,
    #[serde(default)]
    pub role_guess: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub context: String,
    #[serde(default = "half_conf")]
    pub confidence: f64,
}
fn half_conf() -> f64 { 0.5 }

/// Ask the LLM to extract every person mentioned in a customer's stored context.
pub async fn extract_graph(
    customer: &Customer,
    interactions: &[Interaction],
) -> Result<(Vec<ExtractedPerson>, String), String> {
    let mut prompt = String::new();
    prompt.push_str(&format!("Customer: {} (role: {})\n", customer.name, customer.role));
    if !customer.company.is_empty() {
        prompt.push_str(&format!("Company: {}\n", customer.company));
    }
    if !customer.notes.trim().is_empty() {
        prompt.push_str(&format!("Notes:\n{}\n", customer.notes.trim()));
    }
    if !interactions.is_empty() {
        prompt.push_str("\nInteractions:\n");
        for i in interactions.iter().take(20) {
            let detail = if i.details.trim().is_empty() {
                String::new()
            } else {
                format!(" — {}", truncate(i.details.trim(), 250))
            };
            prompt.push_str(&format!("- [{}] {}{}\n", i.kind, i.summary, detail));
        }
    }
    prompt.push_str("\nReturn the JSON now.");
    let (text, model) = bridge(EXTRACT_SYSTEM, &prompt, 1600).await?;
    let cleaned = strip_fences(&text);
    let parsed: ExtractedPeople = match serde_json::from_str(&cleaned) {
        Ok(v) => v,
        Err(_) => {
            // The model output was truncated mid-object. Repair: trim to the last
            // complete `}` outside a string and close the still-open brackets.
            let repaired = repair_truncated_json(&cleaned).unwrap_or_else(|| cleaned.clone());
            serde_json::from_str(&repaired).map_err(|e| {
                format!("could not parse extract JSON ({}):\n{}", e, truncate(&text, 300))
            })?
        }
    };
    Ok((parsed.people, model))
}

const COMMON_SYSTEM: &str = "You are given ONE focus customer and a list of OTHER customers in a \
CRM. Find every meaningful COMMON THEME the focus customer shares with one or more other \
customers. Themes are substantive: industry, product, project, market, hobby, city / area, \
event, a mediating person, an interest — NOT trivial facts (everyone has a name). Only include \
OTHER customers whose stored context clearly supports the theme. The focus customer is \
implicit and MUST NOT appear in customer_ids.\n\
Return ONLY valid JSON in EXACTLY this shape (no prose, no code fences):\n\
{\"themes\":[{\"theme\":\"...\",\"why\":\"...\",\"customer_ids\":[<numbers>]}]}\n\
Rules: 'theme' is a short label (2-6 words). 'why' is 1 sentence explaining the shared ground. \
Each theme MUST have at least ONE customer_id. Aim for 3-6 themes. Reply in the SAME language \
as the focus context.";

#[derive(serde::Deserialize)]
pub struct CommonThemesOut {
    #[serde(default)]
    pub themes: Vec<CommonTheme>,
}
#[derive(serde::Deserialize, Clone)]
pub struct CommonTheme {
    pub theme: String,
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub customer_ids: Vec<i64>,
}

/// Ask the LLM to identify shared themes between a focus customer and every
/// other customer given a compact context per customer.
pub async fn find_common_themes(
    focus_id: i64,
    focus_name: &str,
    focus_ctx: &str,
    others: &[(i64, String, String)],
) -> Result<(Vec<CommonTheme>, String), String> {
    let mut prompt = String::new();
    prompt.push_str(&format!("Focus customer (id={focus_id}, name={focus_name}):\n"));
    prompt.push_str(focus_ctx);
    prompt.push_str("\n\nOther customers:\n");
    // Cap the total prompt so we don't blow past the model's context. Trim each
    // other-customer to ~600 chars and cap the total count to 60 records — plenty
    // for the personal-CRM scale but still safe.
    let mut budget = 12_000usize;
    let mut seen = 0usize;
    for (id, name, ctx) in others.iter().take(60) {
        let ctx = if ctx.chars().count() > 500 {
            ctx.chars().take(500).collect::<String>() + "…"
        } else {
            ctx.clone()
        };
        let block = format!("--- id={id}, name={name} ---\n{ctx}\n");
        if block.chars().count() > budget {
            break;
        }
        budget = budget.saturating_sub(block.chars().count());
        prompt.push_str(&block);
        seen += 1;
    }
    if seen == 0 {
        prompt.push_str("(no other customers)\n");
    }
    prompt.push_str("\nReturn the JSON now.");
    let (text, model) = bridge(COMMON_SYSTEM, &prompt, 2000).await?;
    let cleaned = strip_fences(&text);
    let parsed: CommonThemesOut = match serde_json::from_str(&cleaned) {
        Ok(v) => v,
        Err(_) => {
            let repaired = repair_truncated_json(&cleaned).unwrap_or_else(|| cleaned.clone());
            serde_json::from_str(&repaired).map_err(|e| {
                format!("could not parse common-themes JSON ({e}):\n{}", truncate(&text, 400))
            })?
        }
    };
    // Sanitize: drop empty themes and any customer_ids that == focus id.
    let themes: Vec<CommonTheme> = parsed
        .themes
        .into_iter()
        .filter(|t| !t.theme.trim().is_empty() && !t.customer_ids.is_empty())
        .map(|mut t| {
            t.customer_ids.retain(|id| *id != focus_id);
            t
        })
        .filter(|t| !t.customer_ids.is_empty())
        .collect();
    Ok((themes, model))
}

const PATH_AI_SYSTEM: &str = "You are given TWO customers from a CRM (A and B), each with their \
compact context, PLUS an optional shortest BFS path through the explicit relationship graph. \
Your job is to explain how A and B are connected — going BEYOND the explicit path. Look for: \
shared interests, common markets/industries, hobbies, cities, deals, events, mentioned people \
who could act as a bridge, or any other latent common ground. Reply in the SAME language as \
the input. Return ONLY valid JSON in EXACTLY this shape (no prose, no code fences):\n\
{\"summary\":\"<1-2 sentence overall relationship\",\"connections\":[{\"type\":\"shared_interest|common_market|possible_bridge|explicit_path|weak_tie|shared_person\",\"detail\":\"1 sentence Vietnamese\",\"strength\":\"strong|medium|weak\"}]}\n\
Rules: Return 2-6 connections. Only include ones supported by the given context — do not \
invent facts. If BFS path exists, describe it as one 'explicit_path' entry. If A and B seem \
unconnected, return 1 weak-tie entry explaining why they likely aren't connected.";

#[derive(serde::Deserialize)]
pub struct AiPathOut {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub connections: Vec<AiConnection>,
}
#[derive(serde::Deserialize, Clone)]
pub struct AiConnection {
    #[serde(default)]
    pub r#type: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub strength: String,
}

/// LLM-driven connection search: given two customers + their compact contexts +
/// an optional shortest BFS path (as a list of names), reason about every way
/// they might be connected (shared interests, industries, mentioned people…).
pub async fn path_ai(
    from: &Customer,
    from_ctx: &str,
    to: &Customer,
    to_ctx: &str,
    bfs_path_names: Option<&[String]>,
) -> Result<(AiPathOut, String), String> {
    let mut prompt = String::new();
    prompt.push_str(&format!("A (id={}, {}):\n{}\n\n", from.id, from.name, from_ctx));
    prompt.push_str(&format!("B (id={}, {}):\n{}\n\n", to.id, to.name, to_ctx));
    match bfs_path_names {
        Some(p) if !p.is_empty() => {
            prompt.push_str(&format!("Explicit BFS path A→B: {}\n", p.join(" → ")));
        }
        _ => {
            prompt.push_str("Explicit BFS path A→B: none (no direct relationships).\n");
        }
    }
    prompt.push_str("\nReturn the JSON now.");
    let (text, model) = bridge(PATH_AI_SYSTEM, &prompt, 1400).await?;
    let cleaned = strip_fences(&text);
    let parsed: AiPathOut = match serde_json::from_str(&cleaned) {
        Ok(v) => v,
        Err(_) => {
            let repaired = repair_truncated_json(&cleaned).unwrap_or_else(|| cleaned.clone());
            serde_json::from_str(&repaired).map_err(|e| {
                format!("could not parse ai-path JSON ({e}):\n{}", truncate(&text, 400))
            })?
        }
    };
    Ok((parsed, model))
}

/// Salvage a truncated JSON object: cut back to the last complete `}` (outside
/// a string) and append the closers for any still-open brackets. Ported from
/// the mindmap generator's tolerance layer.
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
    while let Some(closer) = stack.pop() {
        out.push(closer as char);
    }
    Some(out)
}

fn strip_fences(t: &str) -> String {
    let t = t.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        return rest.trim_end_matches("```").trim().to_string();
    }
    // First balanced { ... } block.
    if let Some(start) = t.find('{') {
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
                b'{' => depth += 1,
                b'}' => { depth -= 1; if depth == 0 { return t[start..=start+i].to_string(); } }
                _ => {}
            }
        }
    }
    t.to_string()
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

/// Format a Unix timestamp as `YYYY-MM-DD` (UTC) — good enough for prompts.
fn format_ts(secs: i64) -> String {
    // Avoid pulling chrono just for prompt strings.
    let days = secs.div_euclid(86400);
    let (y, m, d) = jd_to_ymd(days + 2440588);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Gregorian date from a Julian Day Number (algorithm from Meeus).
fn jd_to_ymd(jd: i64) -> (i64, i64, i64) {
    let a = jd + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - (146097 * b) / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4800 + m / 10;
    (year, month, day)
}
