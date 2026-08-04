//! Dynamic skill agents — port of `internal/agent/agents/skill_agent.go` plus
//! the DB loading/composition half of `internal/agent/pool.go`. A skill agent
//! is defined by a `skill_agent` row: playbook skill bodies (resolved via
//! `crate::skillcat`) + optional custom prompt, executed as a ReAct tool-use
//! loop over `crate::tools::Registry`.

use crate::agents::{Agent, Pool, Task, TaskResult};
use crate::context::AgentContext;
use crate::db::{i64_of, str_of, Row};
use serde_json::{json, Map, Value};
use std::sync::Arc;

const MAX_TOOL_LOOP_STEPS: usize = 10;

pub struct SkillAgent {
    agent_type: String,
    name: String,
    description: String,
    skill_body: String,
    tools: crate::tools::Registry,
}

#[async_trait::async_trait]
impl Agent for SkillAgent {
    fn agent_type(&self) -> &str {
        &self.agent_type
    }

    fn name(&self) -> String {
        if self.name.is_empty() {
            self.agent_type.clone()
        } else {
            self.name.clone()
        }
    }

    fn description(&self) -> String {
        self.description.clone()
    }

    fn default_system(&self) -> String {
        self.skill_body.clone()
    }

    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String> {
        let system = self.build_system(&ctx.soul);
        // The daemon bridge is single-turn (system + user), so the ReAct loop
        // grows one transcript prompt instead of a message array.
        let mut transcript = ctx.working.inject_into_prompt(&task.prompt);

        for _step in 0..MAX_TOOL_LOOP_STEPS {
            let (raw, _model) = crate::llm::complete(&system, &transcript, 4000)
                .await
                .map_err(|e| format!("skill_agent {} llm: {e}", self.agent_type))?;

            let Some((tool_name, tool_input)) = parse_envelope(&raw) else {
                // No tool call — final result (plain text or non-JSON).
                let mut data = Map::new();
                data.insert("output".into(), json!(raw));
                data.insert("text".into(), json!(raw));
                data.insert("format".into(), json!("text"));
                data.insert("raw_mode".into(), json!(true));
                return Ok(TaskResult::new(data, crate::llm::truncate(raw.trim(), 120)));
            };

            let (result, error) = match self.tools.execute(&tool_name, tool_input).await {
                Ok(v) => (v, String::new()),
                Err(e) => (Value::Null, e),
            };
            let result_json =
                json!({ "tool": tool_name, "result": result, "error": error }).to_string();

            transcript.push_str(&format!(
                "\n\nASSISTANT:\n{raw}\n\nUSER:\nTool result: {result_json}"
            ));
        }

        Err(format!(
            "skill_agent {}: exceeded max tool loop steps ({MAX_TOOL_LOOP_STEPS})",
            self.agent_type
        ))
    }
}

impl SkillAgent {
    /// System prompt: optional soul override + skill body + upstream-context
    /// note + tool call protocol + tool specs JSON (port of buildSystem).
    fn build_system(&self, soul: &str) -> String {
        let mut sb = String::new();
        let s = soul.trim();
        if !s.is_empty() {
            sb.push_str(s);
            sb.push_str("\n\n---\n\n");
        }
        sb.push_str(&self.skill_body);

        sb.push_str("\n\n## Upstream Pipeline Context\n\n");
        sb.push_str(
            "Your user message may include an `=== Upstream Results ===` section: prior agent \
             outputs keyed by task label. Values may be JSON objects or plain text snippets — \
             read what you need.\n",
        );

        let specs = self.tools.specs();
        if !specs.is_empty() {
            sb.push_str("\n\n## Available Tools\n\n");
            sb.push_str("To call a tool, respond with ONLY this JSON (no markdown):\n");
            sb.push_str(r#"{"tool_call": {"name": "<tool_name>", "input": {<args>}}}"#);
            sb.push_str(
                "\n\nTo finish without tools, reply with plain text or markdown — final answers \
                 do not need to be JSON unless the user prompt asks for structured data.\n\n",
            );
            sb.push_str("### Tool specs\n\n");
            sb.push_str(&serde_json::to_string_pretty(&specs).unwrap_or_default());
        }
        sb
    }
}

/// Tolerantly locate a `{"tool_call":{"name","input"}}` envelope anywhere in
/// the reply (fences, surrounding prose). `None` means the reply is a final
/// answer — either no JSON object, unparseable JSON, or JSON without tool_call.
pub(crate) fn parse_envelope(raw: &str) -> Option<(String, Value)> {
    let candidate = crate::llm::strip_fences(raw);
    let v: Value = serde_json::from_str(&candidate).ok()?;
    let call = v.get("tool_call")?;
    let name = call.get("name")?.as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let input = call.get("input").cloned().unwrap_or_else(|| json!({}));
    Some((name, input))
}

// ---------------------------------------------------------------------------
// DB loading + body composition (pool.go)
// ---------------------------------------------------------------------------

/// Load and register every enabled skill agent from the `skill_agent` table.
pub fn load_skill_agents_from_db(pool: &Arc<Pool>) {
    let rows = pool
        .core
        .db
        .query("SELECT * FROM skill_agent WHERE enabled = 1", &[])
        .unwrap_or_default();
    for row in &rows {
        register_skill_agent(pool, row);
    }
}

/// Register (or re-register) one DB-backed skill agent from its row. A disabled
/// row unregisters instead; an empty composed body is skipped like the Go side.
pub fn register_skill_agent(pool: &Arc<Pool>, row: &Row) {
    let id = str_of(row, "id");
    if id.is_empty() {
        return;
    }
    if row.contains_key("enabled") && i64_of(row, "enabled") == 0 {
        pool.unregister(&id);
        return;
    }
    let ids = parse_skill_agent_skill_ids(&str_of(row, "skill_ids"), &str_of(row, "skill_id"));
    let body = compose_skill_agent_body(pool, &ids, &str_of(row, "prompt"));
    if body.is_empty() {
        return;
    }
    let agent = SkillAgent {
        agent_type: id,
        name: str_of(row, "name"),
        description: extract_skill_description(&body),
        skill_body: body,
        tools: crate::tools::registry(&pool.core),
    };
    pool.register(Arc::new(agent));
}

/// Catalog skill IDs from the skill_ids JSON array, with legacy single
/// skill_id fallback (port of ParseSkillAgentSkillIDs).
pub fn parse_skill_agent_skill_ids(raw_json: &str, legacy_skill_id: &str) -> Vec<String> {
    let raw_json = raw_json.trim();
    if !raw_json.is_empty() && raw_json != "[]" && raw_json != "null" {
        if let Ok(ids) = serde_json::from_str::<Vec<String>>(raw_json) {
            let out: Vec<String> = ids
                .iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != "-" && s != "__none__")
                .collect();
            if !out.is_empty() {
                return out;
            }
        }
    }
    let legacy = legacy_skill_id.trim();
    if !legacy.is_empty() && legacy != "-" && legacy != "__none__" {
        return vec![legacy.to_string()];
    }
    Vec::new()
}

/// Resolve skill IDs against the playbooks catalog and join their bodies plus
/// the custom prompt (port of composeSkillAgentBody + buildSkillBody).
fn compose_skill_agent_body(pool: &Arc<Pool>, ids: &[String], custom: &str) -> String {
    let skills = crate::skillcat::scan(&pool.core.playbooks_dir).unwrap_or_default();
    let mut by_id: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for s in &skills {
        by_id.insert(s.id.clone(), s.body.clone());
        // Legacy skill_id keys in DB used "fk:<name>".
        if !s.id.starts_with("fk:") {
            by_id.insert(format!("fk:{}", s.id), s.body.clone());
        }
    }
    let mut parts: Vec<String> = Vec::new();
    for sid in ids {
        let sid = sid.trim();
        if sid.is_empty() || sid == "-" || sid == "__none__" {
            continue;
        }
        if let Some(b) = by_id.get(sid) {
            if !b.is_empty() {
                parts.push(b.clone());
            }
        }
    }
    let base = parts.join("\n\n---\n\n");
    let custom = custom.trim();
    if custom.is_empty() {
        base
    } else if base.is_empty() {
        custom.to_string()
    } else {
        format!("{base}\n\n---\nCustom instructions:\n{custom}")
    }
}

/// First meaningful non-heading line of a skill body.
fn extract_skill_description(body: &str) -> String {
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        if chars.len() > 120 {
            return chars[..120].iter().collect::<String>() + "…";
        }
        return line.to_string();
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_plain_json() {
        let raw = r#"{"tool_call": {"name": "file_read", "input": {"path": "a.txt"}}}"#;
        let (name, input) = parse_envelope(raw).unwrap();
        assert_eq!(name, "file_read");
        assert_eq!(input["path"], "a.txt");
    }

    #[test]
    fn envelope_fenced_and_prose_wrapped() {
        let fenced = "```json\n{\"tool_call\":{\"name\":\"http_get\",\"input\":{\"url\":\"https://x\"}}}\n```";
        let (name, _) = parse_envelope(fenced).unwrap();
        assert_eq!(name, "http_get");

        let prose = "I will inspect the file now.\n{\"tool_call\":{\"name\":\"file_list\",\"input\":{\"path\":\".\"}}}\nDone.";
        let (name, input) = parse_envelope(prose).unwrap();
        assert_eq!(name, "file_list");
        assert_eq!(input["path"], ".");
    }

    #[test]
    fn envelope_missing_input_defaults_to_empty_object() {
        let raw = r#"{"tool_call": {"name": "repo_list"}}"#;
        let (name, input) = parse_envelope(raw).unwrap();
        assert_eq!(name, "repo_list");
        assert!(input.as_object().unwrap().is_empty());
    }

    #[test]
    fn envelope_final_answers_return_none() {
        // Plain text
        assert!(parse_envelope("The pipeline finished successfully.").is_none());
        // JSON without tool_call → final answer
        assert!(parse_envelope(r#"{"result": "done", "count": 3}"#).is_none());
        // tool_call without a name → final answer
        assert!(parse_envelope(r#"{"tool_call": {"input": {}}}"#).is_none());
        // Text containing braces but not valid JSON
        assert!(parse_envelope("use {curly} braces carefully").is_none());
    }

    #[test]
    fn skill_id_parsing_with_legacy_fallback() {
        assert_eq!(
            parse_skill_agent_skill_ids(r#"["a","b"]"#, "legacy"),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            parse_skill_agent_skill_ids("[]", "legacy"),
            vec!["legacy".to_string()]
        );
        assert_eq!(
            parse_skill_agent_skill_ids(r#"["-","__none__"]"#, "fk:x"),
            vec!["fk:x".to_string()]
        );
        assert!(parse_skill_agent_skill_ids("null", "-").is_empty());
        assert!(parse_skill_agent_skill_ids("", "__none__").is_empty());
    }

    #[test]
    fn description_extraction_skips_headings() {
        let body = "# Title\n\n## Sub\nDoes the thing end-to-end.\nmore";
        assert_eq!(
            extract_skill_description(body),
            "Does the thing end-to-end."
        );
    }
}
