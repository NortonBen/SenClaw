//! Cognify pipeline — port of cognee `cognify(text)`.
//!
//! ```text
//!   text  ──chunk──▶  Chunk DataPoints
//!                          │
//!                          ▼
//!                   LLM triplet extraction
//!                          │
//!                          ▼
//!                  entity resolution
//!                  (exact → fuzzy → vector)
//!                          │
//!                          ▼
//!              upsert nodes + Hebbian edges
//!              (chunk -[MENTIONS]→ entity,
//!               entity -[pred]→ entity)
//! ```
//!
//! Idempotent: re-running on the same text **strengthens** existing edges
//! (Hebbian) and dedupes chunks via content-hash, instead of duplicating.

use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::memory::chunker::{chunk_text, ChunkerOptions};

use super::data_point::DataPoint;
use super::embed::CognitiveEmbedder;
use super::llm::{parse_triplets, LlmClient, RawTriplet};
use super::node_set::NodeSet;
use super::triplet::RelationshipEdge;

// System prompt for the triplet-extraction LLM call.
//
// Design notes:
//   * Multilingual on purpose — chats arrive in Vietnamese, English, etc.
//     Earlier prompt only said "extract triplets" and small models would
//     return `{"triplets":[]}` on Vietnamese first-person statements like
//     "tôi tên là Sen" because no English noun was present. We now spell
//     out that everyday self-introductions count as facts and that entity
//     names MUST stay in the source script (no transliteration).
//   * One-shot example anchors the schema for instruct models that drift
//     on JSON formatting (Qwen, smaller Llamas, etc.).
//   * `response_format=json_object` is already set by OpenAiCompatLlm, so
//     we don't need defensive "JSON only" reminders past the example.
const SYSTEM_PROMPT: &str = "\
You are a knowledge-graph builder. Read the text and extract every (subject, predicate, object) \
factual relationship — including everyday claims like names, locations, preferences, ownerships, \
roles, and identities, even when the sentence is short or first-person. Treat first-person pronouns \
(I/tôi/我/etc.) as a concrete subject when an identity statement is being made. Keep entity names \
in the SAME script as the source (don't translate or transliterate). Use compact, lowercase, \
English predicates (e.g. `name`, `lives_in`, `likes`, `is_a`, `works_at`, `owns`). Skip filler \
words and questions. Return JSON only.\n\n\
Schema: {\"triplets\":[{\"subject\":\"...\",\"predicate\":\"...\",\"object\":\"...\"}]}\n\n\
Example input: \"tôi tên là Sen, sống ở Hà Nội và thích cà phê đen.\"\n\
Example output: {\"triplets\":[\
{\"subject\":\"tôi\",\"predicate\":\"name\",\"object\":\"Sen\"},\
{\"subject\":\"tôi\",\"predicate\":\"lives_in\",\"object\":\"Hà Nội\"},\
{\"subject\":\"tôi\",\"predicate\":\"likes\",\"object\":\"cà phê đen\"}\
]}";

fn content_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

fn build_user_prompt(text: &str, known_entities: &[String]) -> String {
    let entity_hint = if known_entities.is_empty() {
        String::new()
    } else {
        format!(
            "\nPotential entities already known (reuse when applicable): {}\n",
            known_entities.join(", ")
        )
    };
    format!("Extract triplets from the following text.{entity_hint}\nText:\n{text}\n")
}

/// Options controlling the cognify run.
#[derive(Debug, Clone)]
pub struct CognifyOptions {
    pub chunker: ChunkerOptions,
    /// Node-set tags applied to every node produced by this run.
    pub node_sets: Vec<NodeSet>,
    /// Cap on triplets per chunk — defensive against runaway LLM output.
    pub max_triplets_per_chunk: usize,
    /// Importance signal forwarded to `RelationshipEdge::strengthen` for
    /// every new edge produced during this run. Values in (0, 1].
    pub importance: f32,
}

impl Default for CognifyOptions {
    fn default() -> Self {
        Self {
            chunker: ChunkerOptions::default(),
            node_sets: Vec::new(),
            max_triplets_per_chunk: 32,
            importance: 0.8,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct CognifyReport {
    pub chunks_added: usize,
    pub chunks_deduped: usize,
    pub entities_added: usize,
    pub entities_reused: usize,
    pub edges_added: usize,
    pub edges_strengthened: usize,
}

pub struct CognifyPipeline {
    embedder: CognitiveEmbedder,
    llm: Arc<dyn LlmClient>,
}

impl CognifyPipeline {
    pub fn new(embedder: CognitiveEmbedder, llm: Arc<dyn LlmClient>) -> Self {
        Self { embedder, llm }
    }

    /// Run the pipeline on a single document. The same `source` string ties
    /// repeated runs together (chunks dedupe by content hash regardless,
    /// `source` is just for provenance / future filtering).
    pub async fn cognify(
        &self,
        text: &str,
        _source: &str,
        opts: &CognifyOptions,
    ) -> Result<CognifyReport> {
        let now = Utc::now().timestamp();
        let mut report = CognifyReport::default();
        let chunks = chunk_text(text, opts.chunker);

        for ch in chunks {
            let hash = content_hash(&ch.text);
            // Dedupe by content hash.
            let chunk_node = match self
                .embedder
                .graph
                .find_node_by_content_hash(&hash)
                .context("find_node_by_content_hash")?
            {
                Some(existing) => {
                    report.chunks_deduped += 1;
                    existing
                }
                None => {
                    let node = DataPoint::chunk(ch.text.clone(), Some(hash), now);
                    // Persist + embed in one shot.
                    self.embedder
                        .add_node(&node)
                        .await
                        .context("embed chunk node")?;
                    for set in &opts.node_sets {
                        let _ = self.embedder.graph.tag_node(node.id, set);
                    }
                    report.chunks_added += 1;
                    node
                }
            };

            let triplets = self
                .extract_triplets(&ch.text)
                .await
                .context("extract triplets")?;

            for raw in triplets.into_iter().take(opts.max_triplets_per_chunk) {
                self.upsert_triplet(&chunk_node, &raw, opts, &mut report, now)
                    .await?;
            }
        }

        Ok(report)
    }

    async fn extract_triplets(&self, text: &str) -> Result<Vec<RawTriplet>> {
        let user = build_user_prompt(text, &[]);
        // Treat both LLM-call failures (no client configured, network
        // error, rate-limit) AND JSON-parse failures as soft errors. The
        // chunk has still been embedded by the caller; missing triplets
        // just means no edges this round — Hebbian dynamics will rebuild
        // them on the next pass when an LLM is available.
        let raw = match self.llm.complete(SYSTEM_PROMPT, &user).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "[cognify] LLM call failed; skipping triplet extraction");
                return Ok(Vec::new());
            }
        };
        match parse_triplets(&raw) {
            Ok(t) => Ok(t),
            Err(e) => {
                // Defensive: a single chunk failing to parse should not abort
                // the whole document. Log + return empty.
                tracing::warn!(error = %e, "[cognify] triplet parse failed; skipping chunk");
                Ok(Vec::new())
            }
        }
    }

    async fn upsert_triplet(
        &self,
        chunk_node: &DataPoint,
        raw: &RawTriplet,
        opts: &CognifyOptions,
        report: &mut CognifyReport,
        now: i64,
    ) -> Result<()> {
        if raw.subject.trim().is_empty()
            || raw.object.trim().is_empty()
            || raw.predicate.trim().is_empty()
        {
            return Ok(());
        }

        let subj = self.resolve_or_create_entity(&raw.subject, opts, report, now).await?;
        let obj = self.resolve_or_create_entity(&raw.object, opts, report, now).await?;

        // chunk -[MENTIONS]→ subject / object (provenance edges).
        for ent in [&subj, &obj] {
            let mention = RelationshipEdge::new(chunk_node.id, ent.id, "MENTIONS", now);
            let existed = self
                .embedder
                .graph
                .neighbors(chunk_node.id, 256)?
                .into_iter()
                .find(|e| e.dst == ent.id && e.predicate == "MENTIONS");
            match existed {
                Some(mut e) => {
                    e.strengthen(opts.importance, now);
                    self.embedder.graph.upsert_edge(&e)?;
                    report.edges_strengthened += 1;
                }
                None => {
                    let mut m = mention;
                    m.strengthen(opts.importance, now);
                    self.embedder.graph.upsert_edge(&m)?;
                    report.edges_added += 1;
                }
            }
        }

        // subject -[predicate]→ object (semantic edge).
        let existing = self
            .embedder
            .graph
            .neighbors(subj.id, 256)?
            .into_iter()
            .find(|e| e.dst == obj.id && e.predicate.eq_ignore_ascii_case(&raw.predicate));
        match existing {
            Some(mut e) => {
                e.strengthen(opts.importance, now);
                self.embedder.graph.upsert_edge(&e)?;
                report.edges_strengthened += 1;
            }
            None => {
                let mut e = RelationshipEdge::new(subj.id, obj.id, raw.predicate.clone(), now);
                e.context = chunk_node.id.to_string();
                e.source_episode_id = Some(chunk_node.id);
                e.strengthen(opts.importance, now);
                self.embedder.graph.upsert_edge(&e)?;
                report.edges_added += 1;
            }
        }

        Ok(())
    }

    /// Entity resolution: exact-name match first, then fall through to
    /// creating a new entity. Vector-based fuzzy matching is left to P4
    /// (needs a similarity threshold + benchmark before we trust it).
    async fn resolve_or_create_entity(
        &self,
        name: &str,
        opts: &CognifyOptions,
        report: &mut CognifyReport,
        now: i64,
    ) -> Result<DataPoint> {
        let canonical = name.trim();
        if let Some(existing) = self.embedder.graph.find_entity_by_name(canonical)? {
            report.entities_reused += 1;
            return Ok(existing);
        }
        let node = DataPoint::entity(canonical, now);
        self.embedder.add_node(&node).await?;
        for set in &opts.node_sets {
            let _ = self.embedder.graph.tag_node(node.id, set);
        }
        report.entities_added += 1;
        Ok(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Db;
    use crate::memory::cognitive::graph_store::SqliteGraphStore;
    use crate::memory::cognitive::llm::test_support::StubLlm;
    use crate::memory::cognitive::vector_store::SqliteVectorStore;
    use crate::memory::embedding::EmbeddingProvider;
    use async_trait::async_trait;
    use std::sync::Arc;

    // Re-import the traits that were removed from the main module
    use crate::memory::cognitive::graph_store::GraphStore;
    use crate::memory::cognitive::vector_store::VectorStore;

    /// Deterministic fake embedder — hashes input text into f32s so we get
    /// stable but distinct vectors without touching the network or MLX.
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

    fn build_pipeline(replies: Vec<String>) -> CognifyPipeline {
        let cfg = Config::from_env();
        let db = Arc::new(Db::open_in_memory(&cfg).unwrap());
        let graph: Arc<dyn GraphStore> = Arc::new(SqliteGraphStore::new(Arc::clone(&db)));
        let vector: Arc<dyn VectorStore> = Arc::new(SqliteVectorStore::new(Arc::clone(&db)));
        let provider: Arc<dyn EmbeddingProvider> = Arc::new(FakeEmbedder);
        let embedder = CognitiveEmbedder::new(graph, vector, provider);
        let llm = Arc::new(StubLlm::new(replies));
        CognifyPipeline::new(embedder, llm)
    }

    #[tokio::test]
    async fn cognify_creates_nodes_and_edges() {
        let canned = r#"{"triplets":[{"subject":"Ada","predicate":"invented","object":"compiler"}]}"#.to_string();
        let pipe = build_pipeline(vec![canned]);
        let report = pipe
            .cognify("Ada invented the compiler.", "doc1", &CognifyOptions::default())
            .await
            .unwrap();
        assert_eq!(report.chunks_added, 1);
        assert_eq!(report.entities_added, 2);
        // 1 semantic edge + 2 MENTIONS provenance edges
        assert_eq!(report.edges_added, 3);
    }

    #[tokio::test]
    async fn cognify_is_idempotent_and_strengthens() {
        let r1 = r#"{"triplets":[{"subject":"Ada","predicate":"invented","object":"compiler"}]}"#.to_string();
        let r2 = r1.clone();
        let pipe = build_pipeline(vec![r1, r2]);

        let opts = CognifyOptions::default();
        let first = pipe.cognify("Ada invented the compiler.", "doc1", &opts).await.unwrap();
        let second = pipe.cognify("Ada invented the compiler.", "doc1", &opts).await.unwrap();

        // Second pass dedupes the chunk + reuses entities + strengthens edges.
        assert_eq!(first.chunks_added, 1);
        assert_eq!(second.chunks_added, 0);
        assert_eq!(second.chunks_deduped, 1);
        assert_eq!(second.entities_reused, 2);
        assert_eq!(second.entities_added, 0);
        assert!(second.edges_strengthened >= 3);
        assert_eq!(second.edges_added, 0);
    }

    #[tokio::test]
    async fn cognify_skips_empty_triplets() {
        let canned = r#"{"triplets":[{"subject":"","predicate":"x","object":"y"}]}"#.to_string();
        let pipe = build_pipeline(vec![canned]);
        let r = pipe.cognify("noise", "doc", &CognifyOptions::default()).await.unwrap();
        assert_eq!(r.edges_added, 0);
        assert_eq!(r.entities_added, 0);
    }
}
