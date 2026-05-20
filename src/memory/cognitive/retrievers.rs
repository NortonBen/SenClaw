//! Retrievers — port of cognee's `modules/retrieval/*`.
//!
//! Every retriever takes the same `SearchQuery` and returns `Vec<SearchHit>`.
//! All four share a common **seed step**: embed the query text, run vector
//! search, return the top-K nodes. The graph-style modes then walk the graph
//! starting from those seeds.
//!
//! `SpreadingActivation` is the only one that **mutates state**: it calls
//! [`RelationshipEdge::strengthen`] on every edge it traverses, persisting
//! Hebbian write-back. Read-only retrievers use `effective_strength` so they
//! never alter the graph.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use uuid::Uuid;

use super::data_point::{DataPoint, NodeKind};
use super::embed::CognitiveEmbedder;
use super::gnn::GraphScorer;
use super::search::{SearchHit, SearchQuery, SearchType};
use super::triplet::RelationshipEdge;

pub struct CognitiveRetriever {
    pub embedder: Arc<CognitiveEmbedder>,
    scorer: Option<Arc<dyn GraphScorer>>,
}

impl CognitiveRetriever {
    pub fn new(embedder: Arc<CognitiveEmbedder>) -> Self {
        Self { embedder, scorer: None }
    }

    /// Attach a re-ranker (e.g. `LightGcnScorer`). Activated per call by
    /// setting `SearchQuery::rerank = true`.
    pub fn with_scorer(mut self, scorer: Arc<dyn GraphScorer>) -> Self {
        self.scorer = Some(scorer);
        self
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let hits = match query.query_type {
            SearchType::Chunks => self.search_chunks(query).await?,
            SearchType::Triplet => self.search_triplet(query).await?,
            SearchType::GraphCompletion => self.search_graph_completion(query).await?,
            SearchType::SpreadingActivation => self.search_spreading(query).await?,
        };
        if query.rerank {
            self.apply_rerank(query, hits).await
        } else {
            Ok(hits)
        }
    }

    /// Re-rank the candidate set using the configured [`GraphScorer`]. If
    /// no scorer is attached, this is a no-op — caller-set `rerank=true`
    /// without a scorer is treated as "off" rather than an error so the
    /// pipeline stays forgiving.
    async fn apply_rerank(
        &self,
        query: &SearchQuery,
        hits: Vec<SearchHit>,
    ) -> Result<Vec<SearchHit>> {
        let Some(scorer) = self.scorer.as_ref() else {
            return Ok(hits);
        };
        if hits.is_empty() {
            return Ok(hits);
        }

        // Re-embed the query so the scorer compares like-for-like with the
        // stored node embeddings.
        let mut emb = self
            .embedder
            .provider
            .embed(&[query.query_text.clone()])
            .await
            .context("embed query for rerank")?;
        let q = emb.pop().unwrap_or_default();

        // Gather candidate node embeddings from `cog_nodes.embedding` via
        // a single SQL fetch each — caching here would help only when the
        // scorer is hot.
        let candidates: Vec<DataPoint> = hits.iter().map(|h| h.node.clone()).collect();
        let candidate_embs = self.fetch_embeddings(&candidates).await?;
        let new_scores = scorer.score(&q, &candidates, &candidate_embs)?;

        let alpha = query.rerank_alpha.clamp(0.0, 1.0);
        let mut out: Vec<SearchHit> = hits
            .into_iter()
            .zip(new_scores.into_iter())
            .map(|(mut h, s)| {
                h.score = (1.0 - alpha) * h.score + alpha * s;
                h
            })
            .collect();
        out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(out)
    }

    /// Pull the stored embedding BLOBs for the candidate nodes. Falls back
    /// to a zero vector when a candidate has no embedding (shouldn't happen
    /// after add+embed, but keeps the scorer honest).
    async fn fetch_embeddings(&self, candidates: &[DataPoint]) -> Result<Vec<Vec<f32>>> {
        let dims = self.embedder.provider.dimensions() as usize;
        let mut out = Vec::with_capacity(candidates.len());
        for c in candidates {
            // We don't have a direct "get_embedding(id)" yet — synthesise via
            // a fresh embed of the node's own text. This guarantees correct
            // dim + provider model alignment, at the cost of one extra call
            // per candidate during rerank. The candidate set is intentionally
            // small (<= query.limit) so this is acceptable.
            let text = super::embed::text_for_embedding(c);
            if text.trim().is_empty() {
                out.push(vec![0.0f32; dims]);
                continue;
            }
            let mut v = self.embedder.provider.embed(&[text]).await?;
            out.push(v.pop().unwrap_or_else(|| vec![0.0f32; dims]));
        }
        Ok(out)
    }

    // ---- shared: seed by vector ----

    /// Embed the query and return top-K nearest nodes, optionally filtered
    /// to a kind. `limit` is the requested K — callers usually want a few
    /// more candidates than the final result count.
    async fn vector_seeds(
        &self,
        query_text: &str,
        limit: usize,
        kind_filter: Option<NodeKind>,
    ) -> Result<Vec<(DataPoint, f32)>> {
        let mut emb = self
            .embedder
            .provider
            .embed(&[query_text.to_string()])
            .await
            .context("embed query")?;
        let q = emb
            .pop()
            .ok_or_else(|| anyhow::anyhow!("embedder returned empty"))?;
        // Over-fetch when filtering by kind so we still hit `limit` after the filter.
        let fetch = if kind_filter.is_some() { limit * 4 } else { limit };
        let hits = self.embedder.vector.search(&q, fetch.max(8))?;

        let mut out = Vec::with_capacity(hits.len().min(limit));
        for h in hits {
            if let Some(node) = self.embedder.graph.get_node(h.node_id)? {
                if let Some(k) = kind_filter {
                    if node.kind != k {
                        continue;
                    }
                }
                // distance → similarity score in [0, 1]
                let score = 1.0 - h.distance.clamp(0.0, 2.0) * 0.5;
                out.push((node, score));
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(out)
    }

    // ---- Chunks ----

    async fn search_chunks(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let seeds = self
            .vector_seeds(&query.query_text, query.limit, Some(NodeKind::Chunk))
            .await?;
        Ok(seeds
            .into_iter()
            .map(|(node, score)| SearchHit { node, score, path: Vec::new() })
            .collect())
    }

    // ---- Triplet ----

    async fn search_triplet(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let seeds = self
            .vector_seeds(&query.query_text, query.limit, Some(NodeKind::Entity))
            .await?;
        let now = Utc::now().timestamp();
        let mut hits = Vec::new();
        for (entity, seed_score) in seeds {
            let edges = self
                .embedder
                .graph
                .neighbors(entity.id, 32)
                .context("neighbors for triplet")?;
            // The seed entity itself first.
            hits.push(SearchHit { node: entity.clone(), score: seed_score, path: Vec::new() });
            for edge in edges {
                if edge.predicate == "MENTIONS" {
                    continue; // skip provenance edges in TRIPLET view
                }
                let neighbor_id = if edge.src == entity.id { edge.dst } else { edge.src };
                if let Some(nbr) = self.embedder.graph.get_node(neighbor_id)? {
                    let strength = edge.effective_strength(now);
                    hits.push(SearchHit {
                        node: nbr,
                        score: seed_score * strength,
                        path: vec![edge],
                    });
                }
            }
            if hits.len() >= query.limit * 4 {
                break;
            }
        }
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(query.limit);
        Ok(hits)
    }

    // ---- GraphCompletion (read-only k-hop) ----

    async fn search_graph_completion(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        // Entities first — they form the backbone of the subgraph. If we
        // don't find enough entity seeds, fall back to chunk seeds and walk
        // their MENTIONS edges.
        let mut seeds = self
            .vector_seeds(&query.query_text, query.limit, Some(NodeKind::Entity))
            .await?;
        if seeds.is_empty() {
            seeds = self
                .vector_seeds(&query.query_text, query.limit, None)
                .await?;
        }
        self.walk(seeds, query, false).await
    }

    // ---- SpreadingActivation (Hebbian write-back) ----

    async fn search_spreading(&self, query: &SearchQuery) -> Result<Vec<SearchHit>> {
        let seeds = self
            .vector_seeds(&query.query_text, query.limit, None)
            .await?;
        self.walk(seeds, query, true).await
    }

    /// Shared k-hop BFS used by GraphCompletion (read_only=true) and
    /// SpreadingActivation (read_only=false → strengthen as we go).
    async fn walk(
        &self,
        seeds: Vec<(DataPoint, f32)>,
        query: &SearchQuery,
        write_back: bool,
    ) -> Result<Vec<SearchHit>> {
        let now = Utc::now().timestamp();
        // node_id → (best activation, best path)
        let mut best: HashMap<Uuid, (f32, Vec<RelationshipEdge>)> = HashMap::new();
        let mut frontier: Vec<(Uuid, f32, Vec<RelationshipEdge>)> = Vec::new();

        for (node, score) in &seeds {
            best.insert(node.id, (*score, Vec::new()));
            frontier.push((node.id, *score, Vec::new()));
        }

        let decay = query.decay_per_hop.clamp(0.05, 1.0);
        for _hop in 0..query.hops {
            let mut next: Vec<(Uuid, f32, Vec<RelationshipEdge>)> = Vec::new();
            for (node_id, activation, path) in frontier.drain(..) {
                let edges = self.embedder.graph.neighbors(node_id, 64)?;
                for mut edge in edges {
                    let neighbor_id = if edge.src == node_id { edge.dst } else { edge.src };
                    let strength = edge.effective_strength(now);
                    let propagated = activation * decay * strength;
                    if propagated < 0.01 {
                        continue;
                    }

                    if write_back {
                        // Hebbian: passing activation through this edge
                        // strengthens it. Importance scaled by activation so
                        // strong signals reinforce more than weak ones.
                        edge.strengthen(activation.clamp(0.1, 1.0), now);
                        self.embedder.graph.upsert_edge(&edge)?;
                    }

                    let mut new_path = path.clone();
                    new_path.push(edge);

                    let entry = best.entry(neighbor_id).or_insert((f32::NEG_INFINITY, Vec::new()));
                    if propagated > entry.0 {
                        *entry = (propagated, new_path.clone());
                        next.push((neighbor_id, propagated, new_path));
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }

        let mut hits: Vec<SearchHit> = Vec::with_capacity(best.len());
        for (id, (score, path)) in best.drain() {
            if let Some(node) = self.embedder.graph.get_node(id)? {
                hits.push(SearchHit { node, score, path });
            }
        }
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        hits.truncate(query.limit);
        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Db;
    use crate::memory::cognitive::cognify::{CognifyOptions, CognifyPipeline};
    use crate::memory::cognitive::embed::CognitiveEmbedder;
    use crate::memory::cognitive::graph_store::SqliteGraphStore;
    use crate::memory::cognitive::llm::test_support::StubLlm;
    use crate::memory::cognitive::vector_store::SqliteVectorStore;
    use crate::memory::embedding::EmbeddingProvider;
    use async_trait::async_trait;

    // Re-import the trait that was removed from the main module
    use crate::memory::cognitive::graph_store::GraphStore;
    use crate::memory::cognitive::vector_store::VectorStore;

    /// Deterministic embedder: bag-of-bytes hash → 8-dim vector. Distinct
    /// inputs land at distinct points, identical inputs collide exactly.
    struct FakeEmbedder;

    #[async_trait]
    impl EmbeddingProvider for FakeEmbedder {
        fn name(&self) -> &str { "fake" }
        fn model(&self) -> &str { "fake-model" }
        fn dimensions(&self) -> u32 { 8 }
        async fn embed(&self, texts: &[String]) -> anyhow::Result<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|t| {
                    let mut v = vec![0.0f32; 8];
                    for (i, b) in t.bytes().enumerate() {
                        v[i % 8] += b as f32;
                    }
                    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-9);
                    v.iter().map(|x| x / norm).collect()
                })
                .collect())
        }
    }

    /// Build a small fixture: cognify a sentence that yields two triplets
    /// (Ada -[invented]→ compiler, compiler -[runs_on]→ machine).
    async fn fixture() -> (Arc<CognitiveEmbedder>, CognifyPipeline) {
        let cfg = Config::from_env();
        let db = Arc::new(Db::open_in_memory(&cfg).unwrap());
        let graph: Arc<dyn GraphStore> = Arc::new(SqliteGraphStore::new(Arc::clone(&db)));
        let vector: Arc<dyn VectorStore> = Arc::new(SqliteVectorStore::new(Arc::clone(&db)));
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FakeEmbedder);
        let embedder = Arc::new(CognitiveEmbedder::new(graph, vector, provider));
        let canned = r#"{"triplets":[
            {"subject":"Ada","predicate":"invented","object":"compiler"},
            {"subject":"compiler","predicate":"runs_on","object":"machine"}
        ]}"#
        .to_string();
        let llm = Arc::new(StubLlm::new(vec![canned]));
        let pipe = CognifyPipeline::new(
            CognitiveEmbedder::new(
                Arc::clone(&embedder.graph),
                Arc::clone(&embedder.vector),
                Arc::clone(&embedder.provider),
            ),
            llm,
        );
        pipe.cognify(
            "Ada invented the compiler. The compiler runs on the machine.",
            "doc",
            &CognifyOptions::default(),
        )
        .await
        .unwrap();
        (embedder, pipe)
    }

    #[tokio::test]
    async fn chunks_retriever_returns_chunk_nodes() {
        let (embedder, _) = fixture().await;
        let r = CognitiveRetriever::new(embedder);
        let hits = r.search(&SearchQuery::chunks("compiler", 5)).await.unwrap();
        assert!(!hits.is_empty(), "expected at least one chunk hit");
        assert!(hits.iter().all(|h| h.node.kind == NodeKind::Chunk));
    }

    #[tokio::test]
    async fn triplet_retriever_returns_entities_and_edges() {
        let (embedder, _) = fixture().await;
        let r = CognitiveRetriever::new(embedder);
        let hits = r.search(&SearchQuery::triplet("compiler", 10)).await.unwrap();
        assert!(!hits.is_empty());
        // At least one hit should carry an outgoing edge.
        assert!(hits.iter().any(|h| !h.path.is_empty()));
    }

    #[tokio::test]
    async fn graph_completion_walks_multiple_hops() {
        let (embedder, _) = fixture().await;
        let r = CognitiveRetriever::new(embedder);
        // 2 hops should reach `machine` from a `Ada` seed (Ada→compiler→machine).
        let hits = r
            .search(&SearchQuery::graph_completion("Ada", 10, 2))
            .await
            .unwrap();
        let names: Vec<String> = hits
            .iter()
            .filter(|h| h.node.kind == NodeKind::Entity)
            .map(|h| h.node.name.clone())
            .collect();
        assert!(names.iter().any(|n| n == "machine"), "expected to reach 'machine'; got {names:?}");
    }

    #[tokio::test]
    async fn rerank_runs_without_scorer_attached_is_noop() {
        // rerank=true + no scorer → results returned untouched (forgiving).
        let (embedder, _) = fixture().await;
        let r = CognitiveRetriever::new(embedder);
        let mut q = SearchQuery::chunks("compiler", 5);
        q.rerank = true;
        let hits = r.search(&q).await.unwrap();
        assert!(!hits.is_empty());
    }

    #[tokio::test]
    async fn rerank_with_lightgcn_changes_scores() {
        use super::super::gnn::LightGcnScorer;

        let (embedder, _) = fixture().await;
        let scorer: Arc<dyn super::super::gnn::GraphScorer> = Arc::new(
            LightGcnScorer::new(Arc::clone(&embedder.graph)).with_layers(2),
        );
        let r = CognitiveRetriever::new(Arc::clone(&embedder)).with_scorer(scorer);

        let mut q = SearchQuery::graph_completion("Ada", 10, 2);
        let baseline = r.search(&q).await.unwrap();
        q.rerank = true;
        q.rerank_alpha = 0.7;
        let reranked = r.search(&q).await.unwrap();

        // The two result sets should overlap heavily but at least one score
        // should differ — proof the LightGCN blend actually ran.
        let baseline_scores: Vec<(uuid::Uuid, f32)> =
            baseline.iter().map(|h| (h.node.id, h.score)).collect();
        let reranked_scores: Vec<(uuid::Uuid, f32)> =
            reranked.iter().map(|h| (h.node.id, h.score)).collect();
        assert!(!baseline_scores.is_empty());
        assert!(!reranked_scores.is_empty());
        let any_diff = reranked_scores.iter().any(|(id, s_new)| {
            baseline_scores
                .iter()
                .find(|(bid, _)| bid == id)
                .map(|(_, s_old)| (s_new - s_old).abs() > 1e-4)
                .unwrap_or(true)
        });
        assert!(any_diff, "expected at least one score to change after rerank");
    }

    #[tokio::test]
    async fn spreading_activation_writes_back() {
        let (embedder, _) = fixture().await;

        // Snapshot a known edge's activation count BEFORE spreading.
        let ada = embedder.graph.find_entity_by_name("Ada").unwrap().unwrap();
        let before = embedder
            .graph
            .neighbors(ada.id, 16)
            .unwrap()
            .into_iter()
            .find(|e| e.predicate == "invented")
            .expect("invented edge");
        let before_count = before.activation_count;

        let r = CognitiveRetriever::new(Arc::clone(&embedder));
        let _ = r
            .search(&SearchQuery::spreading("Ada", 10, 2))
            .await
            .unwrap();

        let after = embedder
            .graph
            .neighbors(ada.id, 16)
            .unwrap()
            .into_iter()
            .find(|e| e.predicate == "invented")
            .expect("invented edge");
        assert!(
            after.activation_count > before_count,
            "spreading activation should strengthen edges (before={before_count}, after={})",
            after.activation_count
        );
    }
}
