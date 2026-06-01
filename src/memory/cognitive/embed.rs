//! Bridge between cognee/shodh-style `DataPoint`s and senclaw's existing
//! [`memory::embedding::EmbeddingProvider`] stack.
//!
//! Why reuse the existing provider stack? Because it already handles OpenAI,
//! OpenRouter, Ollama, batching, caching, and (eventually) the MLX local
//! backend. The cognitive layer should not duplicate that — it just needs to
//! know which fields of a `DataPoint` to embed.
//!
//! ## Text-for-embedding policy
//!
//! Different node kinds get different text:
//!   * `Chunk`   → `summary` (the chunk body)
//!   * `Entity`  → `"{name}\n{summary}"` (name dominates, with description)
//!   * `Summary` → `summary`
//!   * `Custom`  → fallback to `summary`, then `name`
//!
//! Matches cognee's behaviour of indexing the `metadata.index_fields` per
//! DataPoint type, but flattened — we let the node's `summary` carry the
//! ready-to-embed text instead of indexing structured fields.

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::memory::embedding::EmbeddingProvider;

use super::data_point::{DataPoint, NodeKind};
use super::graph_store::GraphStore;
use super::vector_store::VectorStore;

/// Pick the embedding-input text for a DataPoint.
pub fn text_for_embedding(node: &DataPoint) -> String {
    match node.kind {
        NodeKind::Chunk | NodeKind::Summary => node.summary.clone(),
        NodeKind::Entity => {
            if node.summary.is_empty() {
                node.name.clone()
            } else {
                format!("{}\n{}", node.name, node.summary)
            }
        }
        NodeKind::Custom => {
            if !node.summary.is_empty() {
                node.summary.clone()
            } else {
                node.name.clone()
            }
        }
    }
}

/// Embed a single node and persist the vector. Returns the embedding for
/// callers that want to feed it into immediate retrieval.
pub async fn embed_node(
    provider: &dyn EmbeddingProvider,
    vector_store: &dyn VectorStore,
    node: &DataPoint,
) -> Result<Vec<f32>> {
    let text = text_for_embedding(node);
    if text.trim().is_empty() {
        anyhow::bail!("node {} has no embeddable text", node.id);
    }
    let mut out = provider
        .embed(&[text])
        .await
        .context("embedding provider call")?;
    let emb = out
        .pop()
        .ok_or_else(|| anyhow::anyhow!("embedding provider returned empty"))?;
    vector_store
        .upsert(node.id, &emb, provider.model())
        .context("vector_store upsert")?;
    Ok(emb)
}

/// Thin wrapper that bundles graph + vector + embedder. Most cognify /
/// retrieval code paths want all three together; this keeps the wiring
/// explicit at call sites instead of passing three `Arc`s around.
pub struct CognitiveEmbedder {
    pub graph: Arc<dyn GraphStore>,
    pub vector: Arc<dyn VectorStore>,
    pub provider: Arc<dyn EmbeddingProvider>,
}

impl CognitiveEmbedder {
    pub fn new(
        graph: Arc<dyn GraphStore>,
        vector: Arc<dyn VectorStore>,
        provider: Arc<dyn EmbeddingProvider>,
    ) -> Self {
        Self {
            graph,
            vector,
            provider,
        }
    }

    /// Upsert the node, then embed + index it. Convenience for `add()`-style
    /// flows (cognee `cognee.add(text)`).
    pub async fn add_node(&self, node: &DataPoint) -> Result<Vec<f32>> {
        self.graph.upsert_node(node)?;
        embed_node(&*self.provider, &*self.vector, node).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_for_chunk_uses_summary() {
        let n = DataPoint::chunk("hello", None, 0);
        assert_eq!(text_for_embedding(&n), "hello");
    }

    #[test]
    fn text_for_entity_joins_name_and_summary() {
        let mut n = DataPoint::entity("Ada", 0);
        n.summary = "computer pioneer".into();
        assert_eq!(text_for_embedding(&n), "Ada\ncomputer pioneer");
    }

    #[test]
    fn text_for_entity_without_summary_uses_name() {
        let n = DataPoint::entity("Ada", 0);
        assert_eq!(text_for_embedding(&n), "Ada");
    }
}
