//! Cognitive knowledge-graph source.
//!
//! Goes through the daemon's REST endpoint rather than the `knowledge.search`
//! bridge action on purpose: the bridge defaults `space` to the calling app's
//! id (`src/gateway/ui_server/space.rs:1612`), which would confine every search
//! to the `search` space. REST treats `space: None` as global
//! (`cognitive.rs:727`), which is what a federated search needs.
//!
//! `mode: "hybrid"` fuses the vector and FTS seeds; it is load-bearing, the
//! same way `apps/crm/src/senclaw.rs:84` calls it out.

use crate::model::{Budget, Evidence, SourceHealth, SourceKind, SubQuery};
use crate::sources::SearchSource;
use crate::transport::CoreRest;
use async_trait::async_trait;
use std::time::Duration;

pub struct KnowledgeSource {
    core: CoreRest,
    /// `None` = search every space.
    space: Option<String>,
    hops: u8,
}

impl KnowledgeSource {
    pub fn new(core: CoreRest) -> Self {
        Self {
            core,
            space: None,
            hops: 2,
        }
    }

    #[allow(dead_code)] // P1: per-space knowledge sources
    pub fn with_space(mut self, space: Option<String>) -> Self {
        self.space = space.filter(|s| !s.trim().is_empty());
        self
    }
}

#[async_trait]
impl SearchSource for KnowledgeSource {
    fn id(&self) -> &str {
        "knowledge"
    }
    fn label(&self) -> &str {
        "Knowledge"
    }
    fn kind(&self) -> SourceKind {
        SourceKind::Internal
    }
    fn weight(&self) -> f32 {
        1.2
    }

    async fn health(&self) -> SourceHealth {
        match self.core.cognitive_stats(Duration::from_secs(5)).await {
            Ok(v) => {
                // A graph with no edges answers every query with nothing; say so
                // rather than letting the source look merely unlucky.
                let edges = v.get("edges").and_then(serde_json::Value::as_u64);
                let nodes = v.get("nodes").and_then(serde_json::Value::as_u64);
                if nodes == Some(0) {
                    SourceHealth::degraded("knowledge graph đang rỗng — chưa có gì để tìm")
                } else if edges == Some(0) {
                    SourceHealth::degraded(
                        "graph chưa có quan hệ (edges=0) — chỉ tìm được theo chunk/FTS",
                    )
                } else {
                    SourceHealth::Ready
                }
            }
            Err(e) => SourceHealth::unavailable(format!("cognitive không phản hồi: {e}")),
        }
    }

    async fn search(&self, q: &SubQuery, budget: Budget) -> anyhow::Result<Vec<Evidence>> {
        let hits = self
            .core
            .cognitive_search(
                &q.text,
                "hybrid",
                budget.max_results,
                self.hops,
                self.space.as_deref(),
                Duration::from_millis(budget.timeout_ms),
            )
            .await?;

        let mut evs: Vec<Evidence> = hits
            .into_iter()
            .enumerate()
            .map(|(i, h)| {
                let title = if h.node.name.trim().is_empty() {
                    format!("{} #{}", h.node.kind, &h.node.id[..8.min(h.node.id.len())])
                } else {
                    h.node.name.clone()
                };
                let mut ev = Evidence::new(
                    self.id(),
                    self.kind(),
                    i as u32,
                    h.score,
                    title,
                    h.node.summary,
                    None,
                );
                ev.meta = serde_json::json!({
                    "node_id": h.node.id,
                    "node_kind": h.node.kind,
                    "path_len": h.path_len,
                    "space": self.space,
                });
                ev
            })
            .collect();

        self.fill_missing_text(&mut evs, Duration::from_millis(budget.timeout_ms))
            .await;
        Ok(evs)
    }
}

/// How many text-less hits get the two-hop lookup. Bounded: this is a nicety on
/// top of a result set, never a reason for the source to blow its budget.
const ENRICH_MAX: usize = 12;

impl KnowledgeSource {
    /// Pull the sentence behind each text-less entity hit.
    ///
    /// `/api/cognitive/search` answers with graph nodes: an entity node's
    /// `summary` is almost always empty, so a hit arrives as a bare phrase like
    /// "năng suất của một đội ngũ nhỏ". The text is one hop away — on the
    /// `chunk` node joined by a `MENTIONS` edge. Without it a knowledge citation
    /// shows the reader nothing, and the relevance gate has to judge a fragment.
    ///
    /// Best-effort throughout: any failed lookup leaves the item as it was.
    async fn fill_missing_text(&self, evidence: &mut [Evidence], timeout: Duration) {
        let targets: Vec<usize> = evidence
            .iter()
            .enumerate()
            .filter(|(_, e)| e.snippet.trim().is_empty() && node_id(e).is_some())
            .map(|(i, _)| i)
            .take(ENRICH_MAX)
            .collect();
        if targets.is_empty() {
            return;
        }

        let fetches = targets.iter().map(|&i| {
            let id = node_id(&evidence[i]).unwrap_or_default();
            async move { (i, self.chunk_text(&id, timeout).await) }
        });
        for (i, text) in futures_util::future::join_all(fetches).await {
            if let Some(t) = text {
                evidence[i].snippet = crate::util::truncate_chars(&t, 1_200);
            }
        }
    }

    /// entity id → text of a chunk that mentions it.
    async fn chunk_text(&self, node_id: &str, timeout: Duration) -> Option<String> {
        let detail = self.core.cognitive_node(node_id, timeout).await.ok()?;
        let edges = detail.get("edges")?.as_array()?;
        // `MENTIONS` points chunk → entity, so the neighbour we want is `src`.
        let chunk_id = edges.iter().find_map(|e| {
            let pred = e.get("predicate").and_then(serde_json::Value::as_str)?;
            if !pred.eq_ignore_ascii_case("mentions") {
                return None;
            }
            let src = e.get("src").and_then(serde_json::Value::as_str)?;
            (src != node_id).then(|| src.to_string())
        })?;

        let chunk = self.core.cognitive_node(&chunk_id, timeout).await.ok()?;
        let text = chunk
            .get("node")
            .and_then(|n| n.get("summary"))
            .and_then(serde_json::Value::as_str)?
            .trim();
        (!text.is_empty()).then(|| text.to_string())
    }
}

fn node_id(e: &Evidence) -> Option<String> {
    e.meta
        .get("node_id")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
}
