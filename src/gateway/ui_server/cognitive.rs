//! Cognitive memory HTTP API. Mounted under `/api/cognitive/*`.
//!
//! Endpoints:
//!   * `GET    /api/cognitive/stats`
//!   * `GET    /api/cognitive/nodes?kind=&limit=&offset=`
//!   * `GET    /api/cognitive/node/:id`
//!   * `GET    /api/cognitive/edges?node=&limit=`
//!   * `GET    /api/cognitive/decay-log?limit=`
//!   * `POST   /api/cognitive/search       { query, mode, limit, hops }`
//!   * `DELETE /api/cognitive/node/:id`
//!
//! All handlers require the daemon to have booted the cognitive system
//! (i.e. an embedding provider is configured). When dormant, every endpoint
//! returns HTTP 503 with a clear message instead of pretending to work.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::memory::cognitive::{
    self, CognitiveSystem, DataPoint, RelationshipEdge, SearchHit, SearchQuery, SearchType,
};

use super::core::{AppError, UiState};

// =====================================================================
// Helpers
// =====================================================================

fn require_system() -> Result<Arc<CognitiveSystem>, AppError> {
    cognitive::try_get_instance().ok_or_else(|| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Cognitive system is dormant — configure an embedding provider \
             (SENCLAW_MEMORY_EMBEDDING_PROVIDER) and restart the daemon."
                .to_owned(),
        )
    })
}

fn parse_uuid(raw: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("invalid uuid: {e}")))
}

// =====================================================================
// Wire shapes — kept distinct from the storage types so we can tighten the
// API without affecting the storage layer.
// =====================================================================

#[derive(Debug, Clone, Serialize)]
pub struct NodeView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub summary: String,
    pub salience: f32,
    pub mention_count: u32,
    pub created_at: i64,
    pub last_seen_at: i64,
}

impl From<DataPoint> for NodeView {
    fn from(n: DataPoint) -> Self {
        Self {
            id: n.id.to_string(),
            kind: n.kind.as_str().to_owned(),
            name: n.name,
            summary: n.summary,
            salience: n.salience,
            mention_count: n.mention_count,
            created_at: n.created_at,
            last_seen_at: n.last_seen_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EdgeView {
    pub src: String,
    pub dst: String,
    pub predicate: String,
    pub strength: f32,
    pub tier: u8,
    pub ltp_status: u8,
    pub activation_count: u32,
    pub last_activated: i64,
}

impl From<RelationshipEdge> for EdgeView {
    fn from(e: RelationshipEdge) -> Self {
        Self {
            src: e.src.to_string(),
            dst: e.dst.to_string(),
            predicate: e.predicate,
            strength: e.strength,
            tier: e.tier as u8,
            ltp_status: e.ltp_status as u8,
            activation_count: e.activation_count,
            last_activated: e.last_activated,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HitView {
    pub node: NodeView,
    pub score: f32,
    pub path_len: usize,
}

impl From<SearchHit> for HitView {
    fn from(h: SearchHit) -> Self {
        Self {
            path_len: h.path.len(),
            node: h.node.into(),
            score: h.score,
        }
    }
}

// =====================================================================
// GET /api/cognitive/stats
// =====================================================================

#[derive(Debug, Serialize)]
pub struct StatsResponse {
    pub edges: usize,
    pub nodes_total: usize,
    pub nodes_by_kind: Vec<(String, usize)>,
}

pub(crate) async fn cognitive_stats(
    State(_s): State<Arc<UiState>>,
) -> Result<Json<StatsResponse>, AppError> {
    let sys = require_system()?;
    let edges = sys
        .stats()
        .map(|s| s.edges)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let nodes_total = sys
        .graph
        .count_nodes(None)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let kinds = ["entity", "chunk", "summary", "custom"];
    let mut by_kind = Vec::with_capacity(kinds.len());
    for k in &kinds {
        let n = sys
            .graph
            .count_nodes(Some(k))
            .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        if n > 0 {
            by_kind.push((k.to_string(), n));
        }
    }
    Ok(Json(StatsResponse {
        edges,
        nodes_total,
        nodes_by_kind: by_kind,
    }))
}

// =====================================================================
// GET /api/cognitive/nodes
// =====================================================================

#[derive(Debug, Deserialize)]
pub struct ListNodesQuery {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    50
}

#[derive(Debug, Serialize)]
pub struct ListNodesResponse {
    pub total: usize,
    pub nodes: Vec<NodeView>,
}

pub(crate) async fn cognitive_list_nodes(
    State(_s): State<Arc<UiState>>,
    Query(q): Query<ListNodesQuery>,
) -> Result<Json<ListNodesResponse>, AppError> {
    let sys = require_system()?;
    let kind = q.kind.as_deref();
    let limit = q.limit.clamp(1, 500);
    let total = sys
        .graph
        .count_nodes(kind)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let nodes = sys
        .graph
        .list_nodes(kind, limit, q.offset)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(ListNodesResponse {
        total,
        nodes: nodes.into_iter().map(NodeView::from).collect(),
    }))
}

// =====================================================================
// GET /api/cognitive/node/:id
// =====================================================================

#[derive(Debug, Serialize)]
pub struct NodeDetailResponse {
    pub node: NodeView,
    pub edges: Vec<EdgeView>,
}

pub(crate) async fn cognitive_get_node(
    State(_s): State<Arc<UiState>>,
    Path(id): Path<String>,
) -> Result<Json<NodeDetailResponse>, AppError> {
    let sys = require_system()?;
    let uuid = parse_uuid(&id)?;
    let node = sys
        .graph
        .get_node(uuid)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "node not found".into()))?;
    let edges = sys
        .graph
        .neighbors(uuid, 64)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(NodeDetailResponse {
        node: node.into(),
        edges: edges.into_iter().map(EdgeView::from).collect(),
    }))
}

// =====================================================================
// GET /api/cognitive/decay-log
// =====================================================================

#[derive(Debug, Deserialize)]
pub struct DecayLogQuery {
    #[serde(default = "default_decay_limit")]
    pub limit: usize,
}

fn default_decay_limit() -> usize {
    20
}

pub(crate) async fn cognitive_decay_log(
    State(_s): State<Arc<UiState>>,
    Query(q): Query<DecayLogQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let sys = require_system()?;
    let limit = q.limit.clamp(1, 200);
    let rows = sys
        .graph
        .recent_decay_runs(limit)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "runs": rows })))
}

// =====================================================================
// GET /api/cognitive/top-nodes
// =====================================================================
//
// Used by the Graph Explorer to surface "interesting" seed candidates —
// the user picks a name from this list (or accepts the default selection)
// and the UI calls /sample to actually render the subgraph.
//
// Cheap query: degree aggregate over `cog_edges`, no embeddings needed.

#[derive(Debug, Deserialize)]
pub struct TopNodesQuery {
    #[serde(default = "default_top_limit")]
    pub limit: usize,
}
fn default_top_limit() -> usize {
    20
}

#[derive(Debug, Serialize)]
pub struct TopNodeView {
    pub node: NodeView,
    pub degree: usize,
}

pub(crate) async fn cognitive_top_nodes(
    State(_s): State<Arc<UiState>>,
    Query(q): Query<TopNodesQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let sys = require_system()?;
    let limit = q.limit.clamp(1, 200);
    let rows = sys
        .graph
        .top_nodes_by_degree(limit)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let views: Vec<TopNodeView> = rows
        .into_iter()
        .map(|r| TopNodeView { node: r.node.into(), degree: r.degree })
        .collect();
    Ok(Json(serde_json::json!({ "nodes": views })))
}

// =====================================================================
// GET /api/cognitive/sample
// =====================================================================
//
// Returns a merged subgraph reachable from the top-K most-connected
// nodes. Use this as the Graph Explorer's "default sample" on mount.
// `seed_count` chooses how many top-degree nodes to use as BFS seeds;
// `hops` and `limit` clamp the resulting size like /subgraph does.
//
// Multi-seed merge happens server-side so the UI gets a single payload
// with deduplicated nodes/edges — saves the client from N round-trips +
// client-side union logic.

#[derive(Debug, Deserialize)]
pub struct SampleQuery {
    #[serde(default = "default_seed_count")]
    pub seed_count: usize,
    #[serde(default = "default_sample_hops")]
    pub hops: u8,
    #[serde(default = "default_sample_limit")]
    pub limit: usize,
}
fn default_seed_count() -> usize {
    5
}
fn default_sample_hops() -> u8 {
    2
}
fn default_sample_limit() -> usize {
    150
}

pub(crate) async fn cognitive_sample(
    State(_s): State<Arc<UiState>>,
    Query(q): Query<SampleQuery>,
) -> Result<Json<SubgraphResponse>, AppError> {
    let sys = require_system()?;
    let seed_count = q.seed_count.clamp(1, 20);
    let hops = q.hops.clamp(1, 5);
    let limit = q.limit.clamp(2, 500);

    // Pick top-degree seeds.
    let seeds = sys
        .graph
        .top_nodes_by_degree(seed_count)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if seeds.is_empty() {
        return Ok(Json(SubgraphResponse {
            nodes: Vec::new(),
            edges: Vec::new(),
            truncated: false,
        }));
    }

    // Multi-seed BFS, dedup as we go. Same skeleton as `cognitive_subgraph`
    // but with N starting points.
    use std::collections::{HashMap, HashSet, VecDeque};
    let mut visited: HashMap<Uuid, DataPoint> = HashMap::new();
    let mut frontier: VecDeque<Uuid> = VecDeque::new();
    let mut next_frontier: VecDeque<Uuid> = VecDeque::new();
    let mut edges: Vec<RelationshipEdge> = Vec::new();
    let mut seen_edges: HashSet<(Uuid, Uuid, String)> = HashSet::new();

    for s in seeds {
        if visited.len() >= limit {
            break;
        }
        visited.insert(s.node.id, s.node.clone());
        frontier.push_back(s.node.id);
    }

    let mut truncated = false;
    for _ in 0..hops {
        while let Some(nid) = frontier.pop_front() {
            if visited.len() >= limit {
                truncated = true;
                break;
            }
            let nbrs = sys
                .graph
                .neighbors(nid, 32)
                .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            for edge in nbrs {
                let other = if edge.src == nid { edge.dst } else { edge.src };
                if !visited.contains_key(&other) {
                    if visited.len() >= limit {
                        truncated = true;
                        continue;
                    }
                    if let Ok(Some(n)) = sys.graph.get_node(other) {
                        visited.insert(n.id, n);
                        next_frontier.push_back(other);
                    }
                }
                let key = (edge.src, edge.dst, edge.predicate.clone());
                if !seen_edges.contains(&key) {
                    seen_edges.insert(key);
                    edges.push(edge);
                }
            }
        }
        std::mem::swap(&mut frontier, &mut next_frontier);
        next_frontier.clear();
        if visited.len() >= limit || frontier.is_empty() {
            break;
        }
    }

    // Drop edges with endpoints outside the visited set (defensive).
    let edges: Vec<EdgeView> = edges
        .into_iter()
        .filter(|e| visited.contains_key(&e.src) && visited.contains_key(&e.dst))
        .map(EdgeView::from)
        .collect();
    let nodes: Vec<NodeView> = visited.into_values().map(NodeView::from).collect();

    Ok(Json(SubgraphResponse { nodes, edges, truncated }))
}

// =====================================================================
// GET /api/cognitive/subgraph
// =====================================================================
//
// Extracts a BFS subgraph rooted at `seed`. Used by the force-directed
// graph visualization in the UI — cap on size keeps the client render
// cheap. Edges only between *visited* nodes so the response is a
// self-contained, layout-ready graph.

#[derive(Debug, Deserialize)]
pub struct SubgraphQuery {
    pub seed: String,
    #[serde(default = "default_subgraph_hops")]
    pub hops: u8,
    #[serde(default = "default_subgraph_limit")]
    pub limit: usize,
}

fn default_subgraph_hops() -> u8 {
    2
}
fn default_subgraph_limit() -> usize {
    100
}

#[derive(Debug, Serialize)]
pub struct SubgraphResponse {
    pub nodes: Vec<NodeView>,
    pub edges: Vec<EdgeView>,
    /// True if BFS hit `limit` before exhausting reachable nodes — UI can
    /// show a "results truncated" hint.
    pub truncated: bool,
}

pub(crate) async fn cognitive_subgraph(
    State(_s): State<Arc<UiState>>,
    Query(q): Query<SubgraphQuery>,
) -> Result<Json<SubgraphResponse>, AppError> {
    let sys = require_system()?;
    let seed = parse_uuid(&q.seed)?;
    let hops = q.hops.clamp(1, 5);
    let limit = q.limit.clamp(2, 200);

    // BFS from `seed`. Visit nodes layer-by-layer, stopping when we hit
    // `limit` *visited* (so layer boundaries are respected even when we
    // truncate). Edges are kept only when both endpoints are visited so
    // the response is self-contained.
    use std::collections::{HashMap, HashSet, VecDeque};

    let mut visited: HashMap<Uuid, DataPoint> = HashMap::new();
    let mut frontier: VecDeque<Uuid> = VecDeque::new();
    let mut next_frontier: VecDeque<Uuid> = VecDeque::new();
    let mut edges: Vec<RelationshipEdge> = Vec::new();
    let mut seen_edges: HashSet<(Uuid, Uuid, String)> = HashSet::new();

    match sys.graph.get_node(seed) {
        Ok(Some(node)) => {
            visited.insert(node.id, node);
            frontier.push_back(seed);
        }
        Ok(None) => return Err(AppError(StatusCode::NOT_FOUND, "seed not found".into())),
        Err(e) => return Err(AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }

    let mut truncated = false;
    for _ in 0..hops {
        while let Some(nid) = frontier.pop_front() {
            if visited.len() >= limit {
                truncated = true;
                break;
            }
            let nbrs = sys
                .graph
                .neighbors(nid, 32)
                .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            for edge in nbrs {
                let other = if edge.src == nid { edge.dst } else { edge.src };
                if !visited.contains_key(&other) {
                    if visited.len() >= limit {
                        truncated = true;
                        continue;
                    }
                    if let Ok(Some(n)) = sys.graph.get_node(other) {
                        visited.insert(n.id, n);
                        next_frontier.push_back(other);
                    }
                }
                let key = (edge.src, edge.dst, edge.predicate.clone());
                if !seen_edges.contains(&key) {
                    seen_edges.insert(key);
                    edges.push(edge);
                }
            }
        }
        std::mem::swap(&mut frontier, &mut next_frontier);
        next_frontier.clear();
        if visited.len() >= limit || frontier.is_empty() {
            break;
        }
    }

    // Drop edges with endpoints outside the visited set (defensive).
    let edges: Vec<EdgeView> = edges
        .into_iter()
        .filter(|e| visited.contains_key(&e.src) && visited.contains_key(&e.dst))
        .map(EdgeView::from)
        .collect();
    let nodes: Vec<NodeView> = visited.into_values().map(NodeView::from).collect();

    Ok(Json(SubgraphResponse { nodes, edges, truncated }))
}

// =====================================================================
// POST /api/cognitive/search
// =====================================================================

#[derive(Debug, Deserialize)]
pub struct SearchBody {
    pub query: String,
    /// chunks | triplet | graph | spreading. Default: graph.
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    #[serde(default = "default_hops")]
    pub hops: u8,
    #[serde(default)]
    pub rerank: bool,
}

fn default_search_limit() -> usize {
    10
}
fn default_hops() -> u8 {
    2
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub hits: Vec<HitView>,
}

pub(crate) async fn cognitive_search(
    State(_s): State<Arc<UiState>>,
    Json(body): Json<SearchBody>,
) -> Result<Json<SearchResponse>, AppError> {
    let sys = require_system()?;
    let query_type = match body.mode.as_deref().unwrap_or("graph") {
        "chunks" => SearchType::Chunks,
        "triplet" => SearchType::Triplet,
        "spreading" => SearchType::SpreadingActivation,
        _ => SearchType::GraphCompletion,
    };
    let limit = body.limit.clamp(1, 50);
    let mut q = SearchQuery::chunks(body.query, limit);
    q.query_type = query_type;
    q.hops = body.hops.clamp(1, 6);
    q.rerank = body.rerank;
    q.decay_per_hop = 0.6;

    let hits = sys
        .search(&q)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(SearchResponse {
        hits: hits.into_iter().map(HitView::from).collect(),
    }))
}

// =====================================================================
// DELETE /api/cognitive/node/:id
// =====================================================================

pub(crate) async fn cognitive_forget(
    State(_s): State<Arc<UiState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let sys = require_system()?;
    let uuid = parse_uuid(&id)?;
    sys.graph
        .delete_node(uuid)
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let _ = sys.vector.delete(uuid); // best effort
    Ok(Json(serde_json::json!({ "forgotten": id })))
}

// =====================================================================
// Tests — direct handler invocation (no axum boot)
// =====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::cognitive::data_point::NodeKind;

    #[test]
    fn node_view_roundtrip() {
        let mut n = DataPoint::entity("Ada", 100);
        n.summary = "pioneer".into();
        let v: NodeView = n.into();
        assert_eq!(v.kind, "entity");
        assert_eq!(v.name, "Ada");
        assert_eq!(v.summary, "pioneer");
    }

    #[test]
    fn edge_view_carries_dynamics() {
        let mut e = RelationshipEdge::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "knows",
            10,
        );
        e.strength = 0.42;
        e.activation_count = 7;
        let v: EdgeView = e.into();
        assert_eq!(v.predicate, "knows");
        assert!((v.strength - 0.42).abs() < 1e-5);
        assert_eq!(v.activation_count, 7);
    }

    #[test]
    fn parse_uuid_rejects_garbage() {
        assert!(parse_uuid("nope").is_err());
        assert!(parse_uuid(&Uuid::new_v4().to_string()).is_ok());
    }

    #[test]
    fn require_system_returns_503_when_dormant() {
        // Default test environment hasn't booted the daemon, so the
        // singleton is empty.
        match require_system() {
            Err(AppError(code, msg)) => {
                assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
                assert!(msg.contains("dormant"), "{msg}");
            }
            Ok(_) => {
                // If another test bootstrapped the singleton, that's fine —
                // we just can't run this assertion in that case.
            }
        }
        // touch NodeKind so the import isn't unused if future tests are pruned
        let _ = NodeKind::Entity;
    }
}
