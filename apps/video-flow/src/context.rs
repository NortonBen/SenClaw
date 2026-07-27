//! 3-layer agent context — port of `internal/agent/context`.
//! Layer 1 Working: upstream task results injected into the prompt.
//! Layer 2 Memory: project data read from SQLite.
//! Layer 3 Soul: the agent's own system prompt from `souls/`.

use crate::db::{str_of, Db, Row};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct WorkingContext {
    results: BTreeMap<String, String>,
}

impl WorkingContext {
    pub fn set_result(&mut self, label: &str, value: String) {
        self.results.insert(label.to_string(), value);
    }

    pub fn get_result(&self, label: &str) -> Option<&String> {
        self.results.get(label)
    }

    pub fn all_results(&self) -> &BTreeMap<String, String> {
        &self.results
    }

    /// Append `=== Upstream Results ===` sections to the prompt. JSON strings
    /// are pretty-printed once (never double-encoded).
    pub fn inject_into_prompt(&self, prompt: &str) -> String {
        if self.results.is_empty() {
            return prompt.to_string();
        }
        let mut out = String::from(prompt);
        out.push_str("\n\n=== Upstream Results ===\n");
        for (label, raw) in &self.results {
            let pretty = match serde_json::from_str::<Value>(raw) {
                Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| raw.clone()),
                Err(_) => raw.clone(),
            };
            out.push_str(&format!("{label}: {pretty}\n"));
        }
        out
    }
}

pub struct MemoryManager {
    pub db: Db,
    pub project_id: String,
}

impl MemoryManager {
    /// Name/description/story of the project, each truncated to 1000 chars.
    pub fn project_summary(&self) -> String {
        let Ok(Some(p)) = self.db.get("project", &self.project_id) else {
            return String::new();
        };
        let mut s = format!("Project: {}\n", str_of(&p, "name"));
        for (label, key) in [("Description", "description"), ("Story", "story")] {
            let v = str_of(&p, key);
            if !v.is_empty() {
                s.push_str(&format!("{label}: {}\n", crate::llm::truncate(&v, 1000)));
            }
        }
        s
    }

    pub fn list_characters(&self) -> Vec<Row> {
        self.db
            .query(
                "SELECT c.* FROM character c JOIN project_character pc ON pc.character_id = c.id \
                 WHERE pc.project_id = ?1 ORDER BY c.name",
                &[&self.project_id],
            )
            .unwrap_or_default()
    }

    pub fn search_scenes(&self, term: &str) -> Vec<Row> {
        let like = format!("%{}%", term.replace('%', ""));
        self.db
            .query(
                "SELECT s.* FROM scene s JOIN video v ON v.id = s.video_id \
                 WHERE v.project_id = ?1 AND (s.prompt LIKE ?2 OR s.video_prompt LIKE ?2 OR s.narrator_text LIKE ?2) \
                 ORDER BY s.display_order LIMIT 50",
                &[&self.project_id, &like],
            )
            .unwrap_or_default()
    }
}

pub struct AgentContext {
    pub working: WorkingContext,
    pub memory: MemoryManager,
    pub soul: String,
    pub parent_id: String,
    pub project_id: String,
}

impl AgentContext {
    pub fn new(db: Db, souls_dir: &std::path::PathBuf, agent_type: &str, parent_id: &str, project_id: &str) -> Self {
        AgentContext {
            working: WorkingContext::default(),
            memory: MemoryManager { db, project_id: project_id.to_string() },
            soul: crate::souls::load(souls_dir, agent_type),
            parent_id: parent_id.to_string(),
            project_id: project_id.to_string(),
        }
    }
}
