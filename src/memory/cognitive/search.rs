//! Search surface — port of cognee `SearchType` (subset for P4).
//!
//! Four modes implemented now; the rest of cognee's 17-variant enum
//! (TEMPORAL, CYPHER, NATURAL_LANGUAGE, AGENTIC_COMPLETION, …) land in
//! later phases. The `SearchType` enum stays open so callers and MCP tools
//! see one stable surface.

use serde::{Deserialize, Serialize};

use super::data_point::DataPoint;
use super::node_set::NodeSet;
use super::triplet::RelationshipEdge;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchType {
    /// Dense-vector recall over chunk nodes.
    Chunks,
    /// Dense-vector recall on entities, returning their outgoing edges.
    Triplet,
    /// k-hop subgraph rooted at vector-seed nodes (no write-back).
    GraphCompletion,
    /// BFS with Hebbian write-back — strengthens activated edges so
    /// frequently-recalled paths become more salient over time.
    SpreadingActivation,
}

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub query_text: String,
    pub query_type: SearchType,
    /// Top-K results.
    pub limit: usize,
    /// Hops for graph-style searches (ignored for `Chunks`).
    pub hops: u8,
    /// Multiplicative decay per hop in spreading activation.
    pub decay_per_hop: f32,
    /// Restrict seed nodes to these node_sets (any-of). Empty = no scope.
    pub node_sets: Vec<NodeSet>,
    /// Run the configured `GraphScorer` (e.g. LightGCN) to re-rank the
    /// top-K candidates before returning. Off by default — opt in per call
    /// when the extra IO is worth it (typically: GraphCompletion).
    pub rerank: bool,
    /// Weight for blending base score with rerank score in [0, 1].
    /// `final = (1 - α) * base + α * rerank`. 0.5 = even mix.
    pub rerank_alpha: f32,
}

impl SearchQuery {
    pub fn chunks(text: impl Into<String>, limit: usize) -> Self {
        Self {
            query_text: text.into(),
            query_type: SearchType::Chunks,
            limit,
            hops: 0,
            decay_per_hop: 1.0,
            node_sets: Vec::new(),
            rerank: false,
            rerank_alpha: 0.5,
        }
    }
    pub fn graph_completion(text: impl Into<String>, limit: usize, hops: u8) -> Self {
        Self {
            query_text: text.into(),
            query_type: SearchType::GraphCompletion,
            limit,
            hops,
            decay_per_hop: 0.6,
            node_sets: Vec::new(),
            rerank: false,
            rerank_alpha: 0.5,
        }
    }
    pub fn spreading(text: impl Into<String>, limit: usize, hops: u8) -> Self {
        Self {
            query_text: text.into(),
            query_type: SearchType::SpreadingActivation,
            limit,
            hops,
            decay_per_hop: 0.6,
            node_sets: Vec::new(),
            rerank: false,
            rerank_alpha: 0.5,
        }
    }
    pub fn triplet(text: impl Into<String>, limit: usize) -> Self {
        Self {
            query_text: text.into(),
            query_type: SearchType::Triplet,
            limit,
            hops: 1,
            decay_per_hop: 1.0,
            node_sets: Vec::new(),
            rerank: false,
            rerank_alpha: 0.5,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub node: DataPoint,
    /// Accumulated relevance / activation score. Higher = better.
    pub score: f32,
    /// Path of edges leading to this node from a seed (empty for direct hits).
    pub path: Vec<RelationshipEdge>,
}
