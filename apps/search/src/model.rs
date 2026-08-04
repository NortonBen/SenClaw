//! The types every source produces and every downstream stage consumes.
//!
//! The load-bearing idea is [`Evidence`]: one retrieved item plus its full
//! provenance. A single item retrieved by three sources is ONE `Evidence` with
//! three [`SourceHit`]s — not three items. That merge is what makes
//! corroboration counting (§5.4 of docs/search-app-design.md) mean anything.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Coarse family of a source. Independence is counted per KIND (and per
/// domain), never per source id — three social platforms echoing one press
/// release must not read as three independent confirmations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Public web (SERP, crawled pages).
    Web,
    /// The user's own knowledge: cognitive graph, wiki, file memory.
    Internal,
    /// Social platforms.
    Social,
    /// Uploaded documents / corpora.
    Docs,
    /// Code and repo documentation.
    Code,
    /// User-registered MCP sources.
    Custom,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Web => "web",
            SourceKind::Internal => "internal",
            SourceKind::Social => "social",
            SourceKind::Docs => "docs",
            SourceKind::Code => "code",
            SourceKind::Custom => "custom",
        }
    }
}

/// One source's contribution to an `Evidence`. `rank` is the position within
/// THAT source's own result list — the only cross-source-comparable signal we
/// have, and the reason fusion is rank-based (§5.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceHit {
    pub source_id: String,
    pub kind: SourceKind,
    pub rank: u32,
    /// Source-native score (BM25 / cosine / SERP position / graph activation).
    /// Kept for display and debugging. NEVER compared across sources.
    pub raw_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,
    /// Registrable domain of `canonical_url`, used for independence counting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    pub snippet: String,
    /// Filled by the deepen stage (P0: only for `web` when depth >= 2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_at: Option<i64>,
    pub retrieved_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    #[serde(default)]
    pub meta: Value,

    /// Provenance. Length > 1 means several sources independently surfaced it.
    pub hits: Vec<SourceHit>,
    /// Reciprocal-rank-fusion score, filled by `fusion::fuse`.
    #[serde(default)]
    pub fused_score: f32,
    #[serde(default)]
    pub independent_kinds: usize,
    #[serde(default)]
    pub independent_domains: usize,
}

impl Evidence {
    /// Build a single-source `Evidence`. `fusion::finalize` fills the rest.
    pub fn new(
        source_id: impl Into<String>,
        kind: SourceKind,
        rank: u32,
        raw_score: f32,
        title: impl Into<String>,
        snippet: impl Into<String>,
        url: Option<String>,
    ) -> Self {
        let source_id = source_id.into();
        let title = title.into();
        let snippet = snippet.into();
        let (canonical_url, domain) = match url.as_deref() {
            Some(u) => crate::fusion::canonicalize_url(u),
            None => (None, None),
        };
        let id = crate::fusion::evidence_id(canonical_url.as_deref(), &source_id, &title, &snippet);
        Self {
            id,
            title,
            url,
            canonical_url,
            domain,
            snippet,
            full_text: None,
            author: None,
            published_at: None,
            retrieved_at: now_ms(),
            lang: None,
            meta: Value::Null,
            hits: vec![SourceHit {
                source_id,
                kind,
                rank,
                raw_score,
            }],
            fused_score: 0.0,
            independent_kinds: 0,
            independent_domains: 0,
        }
    }

    /// Text used for near-duplicate detection and, later, claim extraction.
    pub fn body(&self) -> &str {
        self.full_text.as_deref().unwrap_or(&self.snippet)
    }
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// One expansion of the user's question, dispatched to sources.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubQuery {
    pub text: String,
    /// Narrow variant for AND-joined backends (the wiki's FTS AND-joins prefix
    /// terms — `src/wiki/search.rs:130` — so the expanded variant returns
    /// nothing there).
    #[serde(default)]
    pub narrow: Option<String>,
    #[serde(default)]
    pub lang: Option<String>,
}

impl SubQuery {
    #[allow(dead_code)] // used by tests today; by the P2 query planner next
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            narrow: None,
            lang: None,
        }
    }
    /// The variant a source should use given how its backend joins terms.
    pub fn for_and_backend(&self) -> &str {
        self.narrow.as_deref().unwrap_or(&self.text)
    }
}

/// Per-call limits handed to a source.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub max_results: usize,
    pub timeout_ms: u64,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_results: 10,
            timeout_ms: 20_000,
        }
    }
}

/// Whether a source can be used right now — and if not, WHY. A source that is
/// unavailable must say so; it must never masquerade as "returned no results".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum SourceHealth {
    Ready,
    /// Usable but with a known caveat (e.g. social replay search is `not_wired`
    /// upstream, so it will return nothing for some platforms).
    Degraded {
        reason: String,
    },
    Unavailable {
        reason: String,
    },
}

impl SourceHealth {
    pub fn degraded(reason: impl Into<String>) -> Self {
        SourceHealth::Degraded {
            reason: reason.into(),
        }
    }
    pub fn unavailable(reason: impl Into<String>) -> Self {
        SourceHealth::Unavailable {
            reason: reason.into(),
        }
    }
    #[allow(dead_code)] // P1: skip unusable sources during planning
    pub fn usable(&self) -> bool {
        !matches!(self, SourceHealth::Unavailable { .. })
    }
}

/// Outcome of one (source × sub-query) fan-out task. Recorded per run so a
/// degraded run is legible instead of silently thin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceOutcome {
    pub source_id: String,
    pub sub_query: String,
    /// ok | timeout | error | skipped
    pub status: String,
    pub item_count: usize,
    /// Results discarded by a cap. Non-zero means the source had more to give.
    pub dropped_count: usize,
    pub ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
