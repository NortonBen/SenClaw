//! Sources: one uniform interface over every searchable surface.
//!
//! A source knows how to turn a [`SubQuery`] into [`Evidence`]. It does NOT
//! know about ranking, dedupe, claims or reports — those are pipeline stages
//! that operate on the uniform output. Adding a surface means adding a source,
//! never touching the pipeline.

pub mod corpus;
pub mod knowledge;
pub mod mcp_source;
pub mod presets;
pub mod web;
pub mod wiki;

use crate::model::{Budget, Evidence, SourceHealth, SourceKind, SubQuery};
use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

#[async_trait]
pub trait SearchSource: Send + Sync {
    /// Stable id, e.g. `web`, `wiki`, `knowledge`, `social:threads`, `mcp:acme`.
    fn id(&self) -> &str;

    /// Human label for the UI.
    fn label(&self) -> &str;

    fn kind(&self) -> SourceKind;

    /// Prior trust weight used by rank fusion. 1.0 is neutral.
    fn weight(&self) -> f32 {
        1.0
    }

    /// Can this source run right now, and if not, why not?
    ///
    /// Must be honest: a source whose backend is missing reports
    /// `Unavailable`, never an empty result set. A run that quietly drops a
    /// source reads as "nothing out there" when the truth is "we didn't look".
    async fn health(&self) -> SourceHealth;

    async fn search(&self, q: &SubQuery, budget: Budget) -> anyhow::Result<Vec<Evidence>>;
}

/// A source plus its user-tunable configuration.
#[derive(Clone)]
pub struct RegisteredSource {
    pub source: Arc<dyn SearchSource>,
    pub enabled: bool,
    pub weight: f32,
    pub max_results: usize,
    pub timeout_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct SourceInfo {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub enabled: bool,
    pub weight: f32,
    pub max_results: usize,
    pub timeout_ms: u64,
    pub health: SourceHealth,
}

/// The set of sources a run may fan out to.
#[derive(Clone, Default)]
pub struct Registry {
    order: Vec<String>,
    sources: HashMap<String, RegisteredSource>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, source: Arc<dyn SearchSource>) {
        let id = source.id().to_string();
        let weight = source.weight();
        if !self.sources.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.sources.insert(
            id,
            RegisteredSource {
                source,
                enabled: true,
                weight,
                max_results: 10,
                timeout_ms: crate::config::source_timeout_ms(),
            },
        );
    }

    #[allow(dead_code)] // used by the P1 source-detail endpoint
    pub fn get(&self, id: &str) -> Option<&RegisteredSource> {
        self.sources.get(id)
    }

    /// Drop a source. Returns false if it was not registered.
    pub fn remove(&mut self, id: &str) -> bool {
        self.order.retain(|x| x != id);
        self.sources.remove(id).is_some()
    }

    /// Sources in registration order, honoring an explicit selection.
    ///
    /// `wanted = None` means "every enabled source". An explicit selection may
    /// name a disabled source — asking for it counts as enabling it for that
    /// run.
    pub fn select(&self, wanted: Option<&[String]>) -> Vec<RegisteredSource> {
        self.order
            .iter()
            .filter_map(|id| self.sources.get(id))
            .filter(|rs| match wanted {
                Some(w) => w.iter().any(|x| x == rs.source.id()),
                None => rs.enabled,
            })
            .cloned()
            .collect()
    }

    /// Ids named in `wanted` that this registry does not know about. Surfaced
    /// so a typo'd source name is an error, not a silently narrower search.
    pub fn unknown(&self, wanted: &[String]) -> Vec<String> {
        wanted
            .iter()
            .filter(|id| !self.sources.contains_key(*id))
            .cloned()
            .collect()
    }

    pub fn weights(&self) -> HashMap<String, f32> {
        self.sources
            .iter()
            .map(|(id, rs)| (id.clone(), rs.weight))
            .collect()
    }

    pub fn set_config(
        &mut self,
        id: &str,
        enabled: Option<bool>,
        weight: Option<f32>,
        max_results: Option<usize>,
        timeout_ms: Option<u64>,
    ) -> bool {
        match self.sources.get_mut(id) {
            Some(rs) => {
                if let Some(v) = enabled {
                    rs.enabled = v;
                }
                if let Some(v) = weight {
                    rs.weight = v.clamp(0.0, 10.0);
                }
                if let Some(v) = max_results {
                    rs.max_results = v.clamp(1, 100);
                }
                if let Some(v) = timeout_ms {
                    rs.timeout_ms = v.clamp(1_000, 120_000);
                }
                true
            }
            None => false,
        }
    }

    /// Probe every source concurrently. Used by `search_sources` and the UI.
    pub async fn describe(&self) -> Vec<SourceInfo> {
        let entries: Vec<RegisteredSource> = self
            .order
            .iter()
            .filter_map(|id| self.sources.get(id))
            .cloned()
            .collect();

        let probes = entries.iter().map(|rs| {
            let src = rs.source.clone();
            async move { src.health().await }
        });
        let healths = futures_util::future::join_all(probes).await;

        entries
            .into_iter()
            .zip(healths)
            .map(|(rs, health)| SourceInfo {
                id: rs.source.id().to_string(),
                label: rs.source.label().to_string(),
                kind: rs.source.kind().as_str().to_string(),
                enabled: rs.enabled,
                weight: rs.weight,
                max_results: rs.max_results,
                timeout_ms: rs.timeout_ms,
                health,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Evidence;

    struct Stub(&'static str, SourceKind);

    #[async_trait]
    impl SearchSource for Stub {
        fn id(&self) -> &str {
            self.0
        }
        fn label(&self) -> &str {
            self.0
        }
        fn kind(&self) -> SourceKind {
            self.1
        }
        async fn health(&self) -> SourceHealth {
            SourceHealth::Ready
        }
        async fn search(&self, _q: &SubQuery, _b: Budget) -> anyhow::Result<Vec<Evidence>> {
            Ok(vec![])
        }
    }

    fn registry() -> Registry {
        let mut r = Registry::new();
        r.register(Arc::new(Stub("web", SourceKind::Web)));
        r.register(Arc::new(Stub("wiki", SourceKind::Internal)));
        r
    }

    #[test]
    fn select_none_returns_every_enabled_source() {
        let r = registry();
        assert_eq!(r.select(None).len(), 2);
    }

    #[test]
    fn disabled_sources_drop_out_of_the_default_selection() {
        let mut r = registry();
        assert!(r.set_config("wiki", Some(false), None, None, None));
        let ids: Vec<_> = r.select(None).iter().map(|s| s.source.id().to_string()).collect();
        assert_eq!(ids, vec!["web"]);
    }

    #[test]
    fn naming_a_disabled_source_explicitly_runs_it_anyway() {
        let mut r = registry();
        r.set_config("wiki", Some(false), None, None, None);
        let wanted = vec!["wiki".to_string()];
        assert_eq!(r.select(Some(&wanted)).len(), 1);
    }

    #[test]
    fn unknown_source_ids_are_reported_rather_than_ignored() {
        let r = registry();
        let wanted = vec!["web".into(), "typo".into()];
        assert_eq!(r.unknown(&wanted), vec!["typo".to_string()]);
    }

    #[test]
    fn set_config_clamps_out_of_range_values() {
        let mut r = registry();
        r.set_config("web", None, Some(99.0), Some(9999), Some(1));
        let rs = r.get("web").unwrap();
        assert_eq!(rs.weight, 10.0);
        assert_eq!(rs.max_results, 100);
        assert_eq!(rs.timeout_ms, 1_000);
    }
}
