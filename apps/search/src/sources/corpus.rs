//! The app's own uploaded documents, over SQLite FTS5.
//!
//! The only in-process source: no network, no daemon, no peer app. That makes
//! it the source that still works when everything else is down — and the one
//! whose failures are entirely our own fault.

use crate::db::Db;
use crate::model::{Budget, Evidence, SourceHealth, SourceKind, SubQuery};
use crate::sources::SearchSource;
use async_trait::async_trait;

pub struct CorpusSource {
    db: Db,
}

impl CorpusSource {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

#[async_trait]
impl SearchSource for CorpusSource {
    fn id(&self) -> &str {
        "corpus"
    }
    fn label(&self) -> &str {
        "Tài liệu"
    }
    fn kind(&self) -> SourceKind {
        SourceKind::Docs
    }
    /// The user uploaded these on purpose; they outrank a random SERP row.
    fn weight(&self) -> f32 {
        1.4
    }

    async fn health(&self) -> SourceHealth {
        match self.db.list_documents() {
            Err(e) => SourceHealth::unavailable(format!("không đọc được bảng tài liệu: {e}")),
            // An empty corpus is not broken, but it *is* the reason a search
            // over it returns nothing — say which one it is.
            Ok(docs) if docs.is_empty() => {
                SourceHealth::degraded("chưa có tài liệu nào được tải lên")
            }
            Ok(_) => SourceHealth::Ready,
        }
    }

    async fn search(&self, q: &SubQuery, budget: Budget) -> anyhow::Result<Vec<Evidence>> {
        let Some(expr) = crate::corpus::fts_query(&q.text) else {
            // Not an error: the query genuinely contains no searchable token.
            return Ok(vec![]);
        };
        let hits = self.db.search_corpus(&expr, budget.max_results)?;

        Ok(hits
            .into_iter()
            .enumerate()
            .map(|(i, h)| {
                let mut ev = Evidence::new(
                    self.id(),
                    self.kind(),
                    i as u32,
                    h.score,
                    format!("{} · đoạn {}", h.doc_name, h.ord + 1),
                    crate::util::truncate_chars(&h.text, 600),
                    None,
                );
                // No URL, so the citation target is the document + chunk.
                ev.meta = serde_json::json!({
                    "doc_id": h.doc_id,
                    "doc_name": h.doc_name,
                    "chunk_id": h.chunk_id,
                    "chunk_ord": h.ord,
                });
                ev
            })
            .collect())
    }
}
