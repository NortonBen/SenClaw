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

        Ok(hits
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
            .collect())
    }
}
