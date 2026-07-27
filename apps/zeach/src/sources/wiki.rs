//! Git-backed wiki source, over the daemon's REST surface.
//!
//! One subtlety that silently breaks this source if ignored: the wiki builds an
//! **AND**-joined prefix match (`src/wiki/search.rs:130`), while memory and
//! cognitive OR-join their expanded tokens. Feeding the wiki a broad expanded
//! query therefore returns nothing at all — so we send the narrow variant.

use crate::model::{Budget, Evidence, SourceHealth, SourceKind, SubQuery};
use crate::sources::SearchSource;
use crate::transport::CoreRest;
use async_trait::async_trait;
use std::time::Duration;

pub struct WikiSource {
    core: CoreRest,
}

impl WikiSource {
    pub fn new(core: CoreRest) -> Self {
        Self { core }
    }
}

#[async_trait]
impl SearchSource for WikiSource {
    fn id(&self) -> &str {
        "wiki"
    }
    fn label(&self) -> &str {
        "Wiki"
    }
    fn kind(&self) -> SourceKind {
        SourceKind::Internal
    }
    /// Curated, human-written knowledge — worth more per hit than a SERP row.
    fn weight(&self) -> f32 {
        1.3
    }

    async fn health(&self) -> SourceHealth {
        match self.core.wiki_stats(Duration::from_secs(5)).await {
            Ok(_) => SourceHealth::Ready,
            Err(e) => SourceHealth::unavailable(format!("wiki không phản hồi: {e}")),
        }
    }

    async fn search(&self, q: &SubQuery, budget: Budget) -> anyhow::Result<Vec<Evidence>> {
        let hits = self
            .core
            .wiki_search(
                q.for_and_backend(),
                None,
                budget.max_results,
                Duration::from_millis(budget.timeout_ms),
            )
            .await?;

        Ok(hits
            .into_iter()
            .enumerate()
            .map(|(i, h)| {
                let title = if h.title.trim().is_empty() {
                    h.path.clone()
                } else {
                    h.title.clone()
                };
                let mut ev = Evidence::new(
                    self.id(),
                    self.kind(),
                    i as u32,
                    1.0 / (1.0 + i as f32),
                    title,
                    h.snippet,
                    None,
                );
                // Wiki pages have no URL; the path is the citation target.
                ev.meta = serde_json::json!({ "path": h.path, "tags": h.tags });
                ev.published_at = parse_rfc3339_ms(&h.updated);
                ev
            })
            .collect())
    }
}

fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_variant_is_preferred_for_the_and_joined_backend() {
        let mut q = SubQuery::new("lãi suất điều hành ngân hàng nhà nước 2026");
        assert_eq!(q.for_and_backend(), q.text);
        q.narrow = Some("lãi suất điều hành".into());
        assert_eq!(q.for_and_backend(), "lãi suất điều hành");
    }

    #[test]
    fn timestamps_parse_and_bad_ones_are_dropped() {
        assert!(parse_rfc3339_ms("2026-07-20T10:00:00Z").is_some());
        assert_eq!(parse_rfc3339_ms("not a date"), None);
    }
}
