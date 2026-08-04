//! Thin REST client for the SenClaw daemon's UI API. The Skill Builder app uses
//! this both to *read* the current capability inventory (skills, sub-agents, MCP
//! servers) — the context the AI reasons over when drafting a new skill — and to
//! *write* the finished skill back into the daemon's managed skills directory.
//!
//! Everything goes through `SENCLAW_BASE_URL` (injected by the daemon when it
//! launches a Space App); the app never touches the filesystem of the daemon
//! directly.

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct Daemon {
    base_url: String,
    http: reqwest::Client,
}

/// A compact, UI-facing view of everything the agent runtime can already do.
/// This is the "danh sách skill / sub-agent / MCP hiện tại" the requirement
/// analysis is grounded in.
#[derive(Serialize, Default)]
pub struct Inventory {
    pub skills: Vec<Value>,
    pub subagents: Vec<Value>,
    pub mcp_servers: Vec<Value>,
}

impl Daemon {
    pub fn from_env() -> Self {
        let base =
            std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".into());
        Self {
            base_url: base.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let url = format!("{}{}", self.base_url, path);
        let v: Value = self
            .http
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| anyhow!("GET {path} failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("GET {path} bad json: {e}"))?;
        Ok(v)
    }

    /// Read skills, sub-agents and MCP servers, trimmed to the fields the
    /// requirement-analysis prompt actually needs (name + description + a few
    /// signals). Each sub-fetch is best-effort: a daemon that lacks one endpoint
    /// still yields a partial inventory rather than a hard failure.
    pub async fn inventory(&self) -> Inventory {
        let mut inv = Inventory::default();

        if let Ok(v) = self.get("/api/skills").await {
            if let Some(arr) = v.get("skills").and_then(Value::as_array) {
                inv.skills = arr
                    .iter()
                    .filter(|s| !s.get("disabled").and_then(Value::as_bool).unwrap_or(false))
                    .map(|s| {
                        json!({
                            "name": s.get("name").cloned().unwrap_or(Value::Null),
                            "description": s.get("description").cloned().unwrap_or(Value::Null),
                            "triggers": s.get("triggers").cloned().unwrap_or(json!([])),
                            "source": s.get("source").cloned().unwrap_or(Value::Null),
                        })
                    })
                    .collect();
            }
        }

        if let Ok(v) = self.get("/api/subagents").await {
            if let Some(arr) = v.get("subagents").and_then(Value::as_array) {
                inv.subagents = arr
                    .iter()
                    .filter(|s| !s.get("disabled").and_then(Value::as_bool).unwrap_or(false))
                    .map(|s| {
                        json!({
                            "name": s.get("name").cloned().unwrap_or(Value::Null),
                            "description": s.get("description").cloned().unwrap_or(Value::Null),
                        })
                    })
                    .collect();
            }
        }

        if let Ok(v) = self.get("/api/mcp-servers").await {
            // The endpoint may return either a bare array or `{ servers: [...] }`.
            let arr = v
                .as_array()
                .cloned()
                .or_else(|| v.get("servers").and_then(Value::as_array).cloned())
                .or_else(|| v.get("mcpServers").and_then(Value::as_array).cloned())
                .unwrap_or_default();
            inv.mcp_servers = arr
                .iter()
                .map(|s| {
                    json!({
                        "name": s.get("name").cloned().unwrap_or(Value::Null),
                        "description": s.get("description").cloned().unwrap_or(Value::Null),
                        "transport": s.get("transport").cloned().unwrap_or(Value::Null),
                        "tools": s.get("tools").cloned().unwrap_or(json!([])),
                    })
                })
                .collect();
        }

        inv
    }

    /// List installed skills (raw daemon shape, for the "installed" panel).
    pub async fn list_skills(&self) -> Result<Value> {
        self.get("/api/skills").await
    }

    /// Install (or overwrite) a skill via `POST /api/skills/create`. `triggers`
    /// are written into the SKILL.md frontmatter by the daemon so the skill can
    /// auto-surface on matching prompts.
    pub async fn create_skill(
        &self,
        name: &str,
        description: &str,
        content: &str,
        triggers: &[String],
        overwrite: bool,
    ) -> Result<Value> {
        let url = format!("{}/api/skills/create", self.base_url);
        let resp = self
            .http
            .post(&url)
            .json(&json!({
                "name": name,
                "description": description,
                "content": content,
                "triggers": triggers,
                "overwrite": overwrite,
            }))
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| anyhow!("create skill failed: {e}"))?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = body
                .get("error")
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| format!("daemon returned {status}"));
            return Err(anyhow!(msg));
        }
        Ok(body)
    }

    /// Remove an installed skill via `DELETE /api/skills/{name}`.
    pub async fn delete_skill(&self, name: &str) -> Result<Value> {
        let url = format!("{}/api/skills/{}", self.base_url, name);
        let resp = self
            .http
            .delete(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| anyhow!("delete skill failed: {e}"))?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(anyhow!("daemon returned {status}"));
        }
        Ok(body)
    }
}
