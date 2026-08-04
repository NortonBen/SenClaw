//! The core "forge" logic: turn a plain-language skill *requirement* (what the
//! skill is for + when it should run) into a ready-to-install SenClaw skill.
//!
//! The AI is given the daemon's live capability inventory (existing skills,
//! sub-agents, MCP tools) so it can (a) avoid duplicating an existing skill,
//! (b) compose the new skill out of tools/sub-agents that already exist, and
//! (c) pick sensible `triggers` for auto-loading.

use app_space_sdk::SpaceClient;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::daemon::Inventory;

/// The finished draft — the exact fields `POST /api/skills/create` needs, plus
/// the AI's reasoning so the user can judge it before installing.
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct DraftSkill {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub triggers: Vec<String>,
    #[serde(default)]
    pub content: String,
    /// MCP tools / sub-agents the skill body relies on (for UI display).
    #[serde(default)]
    pub uses_mcp: Vec<String>,
    #[serde(default)]
    pub uses_subagents: Vec<String>,
    /// One short paragraph: why this design, what it reuses, what's new.
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub model: String,
}

const SYSTEM: &str = "You are SenClaw Skill Builder — an expert at authoring high-quality \
agent *skills* for the SenClaw personal-AI framework. A skill is a Markdown file (SKILL.md) \
with YAML frontmatter (name, description, triggers) plus a body of instructions telling the \
agent HOW to accomplish a task, usually by orchestrating MCP tools and sub-agents that \
already exist.\n\n\
You are given: (1) the user's requirement — what the skill is for and when it should run; \
(2) the LIVE INVENTORY of skills, sub-agents and MCP servers/tools already available in this \
SenClaw instance. Your job is to design ONE new skill that satisfies the requirement.\n\n\
Design rules:\n\
- REUSE what exists. Prefer composing MCP tools and sub-agents from the inventory over \
inventing new capabilities. Reference tools by their exact `mcp__<server>__<tool>` name.\n\
- If an existing skill already covers the requirement, say so in `rationale` and still \
produce the best complementary skill (do NOT refuse).\n\
- The body must be concrete: name the tools to call, the order, decision points, and how to \
present the result. Keep it tight and skimmable (headers + short bullet lists).\n\
- `triggers` are short lowercase keyword phrases (2-6 words) a user might actually type that \
should auto-surface this skill. Include natural phrasings in BOTH the user's language and \
English when relevant. 6-14 triggers.\n\
- `description` is a single dense sentence in the 'Use when …' style used for skill matching: \
what the skill does and the situations it applies to.\n\
- `name` is a slug (lowercase, digits, hyphens only), descriptive and unique vs. the inventory.\n\n\
Return ONLY valid JSON, no prose, no markdown fences, in EXACTLY this shape:\n\
{\"name\":\"my-skill\",\"description\":\"Use when …\",\"triggers\":[\"...\"],\
\"uses_mcp\":[\"mcp__server__tool\"],\"uses_subagents\":[\"agent-name\"],\
\"rationale\":\"one short paragraph\",\"content\":\"# Title\\n\\n<markdown body>\"}\n\
Write the description, triggers and body in the SAME language as the user's requirement \
(default Vietnamese if mixed), but keep tool identifiers verbatim.";

/// Build the compact inventory block injected into the prompt.
fn inventory_block(inv: &Inventory) -> String {
    fn line(v: &Value) -> String {
        let name = v.get("name").and_then(Value::as_str).unwrap_or("?");
        let desc = v
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .replace('\n', " ");
        let desc = truncate(&desc, 200);
        format!("- {name}: {desc}")
    }

    let mut s = String::new();
    s.push_str("## Existing skills\n");
    if inv.skills.is_empty() {
        s.push_str("(none)\n");
    } else {
        for v in inv.skills.iter().take(80) {
            s.push_str(&line(v));
            s.push('\n');
        }
    }
    s.push_str("\n## Existing sub-agents\n");
    if inv.subagents.is_empty() {
        s.push_str("(none)\n");
    } else {
        for v in inv.subagents.iter().take(60) {
            s.push_str(&line(v));
            s.push('\n');
        }
    }
    s.push_str("\n## Available MCP servers & tools\n");
    if inv.mcp_servers.is_empty() {
        s.push_str("(none)\n");
    } else {
        for srv in inv.mcp_servers.iter().take(40) {
            let name = srv.get("name").and_then(Value::as_str).unwrap_or("?");
            let desc = srv
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .replace('\n', " ");
            s.push_str(&format!("- server `{name}`: {}\n", truncate(&desc, 160)));
            if let Some(tools) = srv.get("tools").and_then(Value::as_array) {
                for t in tools.iter().take(24) {
                    // tools may be strings or {name, description} objects
                    let (tn, td) = match t {
                        Value::String(s) => (s.as_str(), ""),
                        _ => (
                            t.get("name").and_then(Value::as_str).unwrap_or("?"),
                            t.get("description").and_then(Value::as_str).unwrap_or(""),
                        ),
                    };
                    if td.is_empty() {
                        s.push_str(&format!("    - {tn}\n"));
                    } else {
                        s.push_str(&format!(
                            "    - {tn}: {}\n",
                            truncate(&td.replace('\n', " "), 120)
                        ));
                    }
                }
            }
        }
    }
    s
}

/// Generate a draft skill for `requirement` grounded in `inv`.
/// `when_to_run` is optional extra guidance about triggering conditions.
pub async fn draft(
    requirement: &str,
    when_to_run: &str,
    inv: &Inventory,
) -> Result<DraftSkill, String> {
    let mut prompt = String::new();
    prompt.push_str("# Requirement\n");
    prompt.push_str("## Kỹ năng dùng để làm gì (purpose)\n");
    prompt.push_str(requirement.trim());
    prompt.push('\n');
    if !when_to_run.trim().is_empty() {
        prompt.push_str("\n## Khi nào nên chạy (when to run / triggers)\n");
        prompt.push_str(when_to_run.trim());
        prompt.push('\n');
    }
    prompt.push_str("\n# Live inventory\n");
    prompt.push_str(&inventory_block(inv));
    prompt.push_str("\nNow design the skill and return the JSON.");

    let (text, model) = bridge_llm(SYSTEM, &prompt, 3200).await?;
    let mut draft = parse_draft(&text).ok_or_else(|| {
        format!(
            "could not parse skill JSON from model output:\n{}",
            truncate(&text, 500)
        )
    })?;

    draft.name = slugify(&draft.name);
    if draft.name.is_empty() {
        return Err("model did not return a valid skill name".into());
    }
    if draft.content.trim().is_empty() {
        return Err("model returned an empty skill body".into());
    }
    draft.model = model;
    Ok(draft)
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for c in s.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if (c == '-' || c == '_' || c == ' ') && !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

async fn bridge_llm(
    system: &str,
    prompt: &str,
    max_tokens: u32,
) -> Result<(String, String), String> {
    if std::env::var("SENCLAW_SPACE_APP_ID").is_err() {
        std::env::set_var("SENCLAW_SPACE_APP_ID", "skill-builder");
    }
    SpaceClient::from_env()
        .llm_request(system, prompt, max_tokens)
        .await
        .map_err(|e| e.to_string())
}

// ---- JSON extraction / repair (mirrors the mindmap app's tolerant parser) ----

fn parse_draft(text: &str) -> Option<DraftSkill> {
    if let Ok(d) = serde_json::from_str::<DraftSkill>(text.trim()) {
        return Some(d);
    }
    let cleaned = strip_fences(text);
    if let Ok(d) = serde_json::from_str::<DraftSkill>(cleaned.trim()) {
        return Some(d);
    }
    if let Some(block) = first_json_object(&cleaned) {
        if let Ok(d) = serde_json::from_str::<DraftSkill>(&block) {
            return Some(d);
        }
    }
    let repaired = repair_truncated_json(&cleaned)?;
    serde_json::from_str::<DraftSkill>(&repaired).ok()
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

fn strip_fences(text: &str) -> String {
    let t = text.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        return rest.trim_end_matches("```").to_string();
    }
    t.to_string()
}

fn first_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{' || b == b'[')?;
    let open = bytes[start];
    let close = if open == b'{' { b'}' } else { b']' };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            x if x == open => depth += 1,
            x if x == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn repair_truncated_json(text: &str) -> Option<String> {
    let start = text.find(|c| c == '{' || c == '[')?;
    let s = &text[start..];
    let bytes = s.as_bytes();
    let mut in_str = false;
    let mut esc = false;
    let mut last_close: Option<usize> = None;
    for (i, &b) in bytes.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'}' => last_close = Some(i),
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
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    let mut out = head.to_string();
    while let Some(closer) = stack.pop() {
        out.push(closer as char);
    }
    Some(out)
}
