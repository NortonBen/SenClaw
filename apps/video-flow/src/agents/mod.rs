//! Agent framework — port of `internal/agent` (base.go + pool.go). Every
//! built-in pipeline agent implements `Agent`; the `Pool` owns the registry and
//! builds the 3-layer context per execution. LLM calls go through
//! `crate::llm` (the SenClaw daemon bridge) — there is no provider layer here.

pub mod builtin;
pub mod skill_agent;

use crate::context::AgentContext;
use crate::state::Core;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

#[derive(Clone, Default, Debug)]
pub struct Task {
    pub id: String,
    pub label: String,
    pub agent_type: String,
    pub prompt: String,
    pub timeout_seconds: i64,
    /// label → raw result JSON of upstream tasks (only depends_on/input_from).
    pub upstream_results: HashMap<String, String>,
}

#[derive(Clone, Default, Debug)]
pub struct TaskResult {
    /// Persisted as the dag_task `result` JSON.
    pub data: Map<String, Value>,
    pub summary: String,
}

impl TaskResult {
    pub fn new(data: Map<String, Value>, summary: impl Into<String>) -> Self {
        TaskResult { data, summary: summary.into() }
    }
}

#[async_trait::async_trait]
pub trait Agent: Send + Sync {
    fn agent_type(&self) -> &str;
    fn name(&self) -> String {
        self.agent_type().to_string()
    }
    fn description(&self) -> String {
        String::new()
    }
    /// The default system prompt used when no soul file overrides it.
    fn default_system(&self) -> String {
        String::new()
    }
    async fn execute(&self, ctx: &mut AgentContext, task: &Task) -> Result<TaskResult, String>;
}

/// Info row for /api/agents and orchestrator planning.
#[derive(Clone)]
pub struct AgentInfo {
    pub agent_type: String,
    pub name: String,
    pub description: String,
    pub kind: String, // "built-in" | "skill"
}

pub struct Pool {
    pub core: Arc<Core>,
    agents: RwLock<HashMap<String, Arc<dyn Agent>>>,
    /// Canonical planning order of the built-ins (Go: DAGAgentTypeOrder).
    pub builtin_order: Vec<&'static str>,
}

impl Pool {
    pub fn new(core: Arc<Core>) -> Arc<Self> {
        let pool = Arc::new(Pool {
            core,
            agents: RwLock::new(HashMap::new()),
            builtin_order: vec![
                "director",
                "screenwriter",
                "scene_plan",
                "shot_design",
                "visual_asset",
                "scene_builder",
                "script_parser",
                "gen_ref",
                "director_frame",
                "character",
                "image",
                "video",
                "audio",
                "media_download",
                "concat",
                "critic",
            ],
        });
        builtin::register_builtins(&pool);
        skill_agent::load_skill_agents_from_db(&pool);
        pool
    }

    pub fn register(&self, agent: Arc<dyn Agent>) {
        self.agents.write().unwrap().insert(agent.agent_type().to_string(), agent);
    }

    pub fn unregister(&self, agent_type: &str) {
        self.agents.write().unwrap().remove(agent_type);
    }

    pub fn get(&self, agent_type: &str) -> Option<Arc<dyn Agent>> {
        self.agents.read().unwrap().get(agent_type).cloned()
    }

    pub fn list_info(&self) -> Vec<AgentInfo> {
        let agents = self.agents.read().unwrap();
        let mut out: Vec<AgentInfo> = Vec::new();
        // Built-ins first in canonical order, then the rest alphabetically.
        for t in &self.builtin_order {
            if let Some(a) = agents.get(*t) {
                out.push(AgentInfo {
                    agent_type: a.agent_type().to_string(),
                    name: a.name(),
                    description: a.description(),
                    kind: "built-in".into(),
                });
            }
        }
        if let Some(a) = agents.get("orchestrator") {
            out.push(AgentInfo {
                agent_type: "orchestrator".into(),
                name: a.name(),
                description: a.description(),
                kind: "built-in".into(),
            });
        }
        let mut rest: Vec<&Arc<dyn Agent>> = agents
            .values()
            .filter(|a| !self.builtin_order.contains(&a.agent_type()) && a.agent_type() != "orchestrator")
            .collect();
        rest.sort_by_key(|a| a.agent_type().to_string());
        for a in rest {
            out.push(AgentInfo {
                agent_type: a.agent_type().to_string(),
                name: a.name(),
                description: a.description(),
                kind: "skill".into(),
            });
        }
        out
    }

    /// Execute one task: build the 3-layer context, inject upstream results,
    /// dispatch to the registered agent.
    pub async fn execute(
        &self,
        task: &Task,
        parent_id: &str,
        project_id: &str,
    ) -> Result<TaskResult, String> {
        let agent = self
            .get(&task.agent_type)
            .ok_or_else(|| format!("unknown agent type: {}", task.agent_type))?;
        let mut ctx = AgentContext::new(
            self.core.db.clone(),
            &self.core.souls_dir,
            &task.agent_type,
            parent_id,
            project_id,
        );
        for (label, raw) in &task.upstream_results {
            ctx.working.set_result(label, raw.clone());
        }
        agent.execute(&mut ctx, task).await
    }

    /// system prompt = soul override or the agent's in-code default.
    pub fn system_prompt(&self, agent_type: &str) -> String {
        let soul = crate::souls::load(&self.core.souls_dir, agent_type);
        let fallback = self.get(agent_type).map(|a| a.default_system()).unwrap_or_default();
        crate::souls::or_default(&soul, &fallback)
    }
}
