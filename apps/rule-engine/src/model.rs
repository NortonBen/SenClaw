//! Persisted shapes. These are also the wire shapes the UI sees.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::engine::types::{ChainId, Edge, NodeId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ChainStatus {
    Active,
    Inactive,
    Error,
}

impl ChainStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ChainStatus::Active => "ACTIVE",
            ChainStatus::Inactive => "INACTIVE",
            ChainStatus::Error => "ERROR",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_uppercase().as_str() {
            "ACTIVE" => ChainStatus::Active,
            "ERROR" => ChainStatus::Error,
            _ => ChainStatus::Inactive,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chain {
    pub id: ChainId,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub status: ChainStatus,
    /// Chain-wide debug: trace every hop, not just nodes flagged individually.
    #[serde(default)]
    pub debug: bool,
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// How a node combines messages arriving on its input ports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JoinPolicy {
    /// Every message fires the node on its own. Matches the Go engine.
    #[default]
    Any,
    /// Wait for one message on each *connected* input port, then fire once with
    /// `{ "<port>": <data>, ... }`.
    All,
    /// Like `All`, but the parts are deep-merged into one object.
    Merge,
}

/// Per-node runtime knobs, stored in the `opts` column.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeOpts {
    pub join: JoinPolicy,
    /// Group joins by a value inside the message instead of by run.
    pub corr_key: Option<String>,
    pub join_timeout_ms: Option<u64>,
    /// Workers reading this node's mailbox. 1 keeps per-node ordering.
    pub concurrency: u32,
    pub retries: u32,
    pub retry_backoff_ms: u64,
}

/// Upper bound on workers spawned per node. `concurrency` comes straight from a
/// user/agent-supplied number, and the engine spawns exactly that many tasks —
/// without a ceiling a typo (`1000000`) would try to spawn a million.
pub const MAX_CONCURRENCY: u32 = 64;

impl Default for NodeOpts {
    fn default() -> Self {
        Self {
            join: JoinPolicy::Any,
            corr_key: None,
            join_timeout_ms: None,
            concurrency: 1,
            retries: 0,
            retry_backoff_ms: 500,
        }
    }
}

impl NodeOpts {
    /// Number of workers to actually spawn: at least 1, at most [`MAX_CONCURRENCY`].
    pub fn workers(&self) -> u32 {
        self.concurrency.clamp(1, MAX_CONCURRENCY)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    #[serde(default)]
    pub chain_id: ChainId,
    /// `RuleSpec.id`
    pub rule: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "empty_obj")]
    pub config: Value,
    #[serde(default)]
    pub opts: NodeOpts,
    #[serde(default)]
    pub x: f64,
    #[serde(default)]
    pub y: f64,
    #[serde(default)]
    pub debug: bool,
}

fn empty_obj() -> Value {
    json!({})
}

/// What `GET /api/chains/:id` returns and `PUT .../graph` accepts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphDto {
    pub chain: Chain,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Running,
    Done,
    Failed,
    Timeout,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Done => "done",
            RunStatus::Failed => "failed",
            RunStatus::Timeout => "timeout",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRow {
    pub id: i64,
    pub chain_id: ChainId,
    pub status: String,
    pub trigger_node: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub hops: i64,
    pub error: Option<String>,
}

/// One step of a run's trace. Written only when the chain or node is in debug.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HopRow {
    #[serde(default)]
    pub id: i64,
    pub run_id: i64,
    pub chain_id: ChainId,
    pub seq: i64,
    pub node: String,
    pub rule: String,
    pub in_port: String,
    #[serde(default)]
    pub out_port: String,
    pub kind: String,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub error: String,
    pub ts: i64,
    pub dur_ms: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogRow {
    #[serde(default)]
    pub id: i64,
    pub chain_id: ChainId,
    pub run_id: Option<i64>,
    pub level: String,
    pub node: Option<String>,
    pub message: String,
    pub ts: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_opts_default_is_any_join_single_worker() {
        let o = NodeOpts::default();
        assert_eq!(o.join, JoinPolicy::Any);
        assert_eq!(o.concurrency, 1);
    }

    #[test]
    fn node_opts_tolerates_partial_json() {
        let o: NodeOpts = serde_json::from_str(r#"{"join":"all"}"#).unwrap();
        assert_eq!(o.join, JoinPolicy::All);
        assert_eq!(o.concurrency, 1);
    }

    #[test]
    fn chain_status_roundtrips_unknown_as_inactive() {
        assert_eq!(ChainStatus::parse("weird"), ChainStatus::Inactive);
        assert_eq!(ChainStatus::parse("active"), ChainStatus::Active);
    }
}
