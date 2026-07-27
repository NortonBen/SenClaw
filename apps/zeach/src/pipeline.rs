//! Fan-out → normalize → dedupe → fuse → (deepen).
//!
//! P0 stops here — this is `search_query`, the cheap LLM-free path other
//! components call. Claim extraction, corroboration and reports (P2/P3) will
//! consume the same [`SearchOutcome`].
//!
//! The invariant that matters: **a failing source degrades the run, it never
//! fails it**, and every degradation is recorded in [`SourceOutcome`] so a thin
//! result is legible instead of looking like "there was nothing out there".

use crate::fusion;
use crate::model::{Budget, Evidence, SourceHealth, SourceKind, SourceOutcome, SubQuery};
use crate::sources::Registry;
use crate::transport::Transports;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    /// Explicit source selection. `None` = every enabled source.
    #[serde(default)]
    pub sources: Option<Vec<String>>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub lang: Option<String>,
    /// 1 = snippets only. 2 = also fetch full page text for the top web hits.
    #[serde(default = "default_depth")]
    pub depth: u8,
}

fn default_limit() -> usize {
    20
}
fn default_depth() -> u8 {
    1
}

impl SearchRequest {
    #[allow(dead_code)] // used by tests today; by the P2 planner/monitors next
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            sources: None,
            limit: default_limit(),
            lang: None,
            depth: default_depth(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchOutcome {
    pub query: String,
    pub evidence: Vec<Evidence>,
    /// Per (source × sub-query) result — including the ones that failed.
    pub sources: Vec<SourceOutcome>,
    /// Source ids the caller asked for that do not exist. A typo must be an
    /// error, not a silently narrower search.
    pub unknown_sources: Vec<String>,
    pub total_before_dedupe: usize,
    pub deepened: usize,
    pub ms: u64,
}

/// How many top web results get their full text fetched at depth >= 2.
const DEEPEN_TOP_N: usize = 5;

pub async fn run(
    registry: &Registry,
    transports: &Arc<Transports>,
    req: &SearchRequest,
) -> SearchOutcome {
    let started = Instant::now();
    let unknown_sources = req
        .sources
        .as_deref()
        .map(|w| registry.unknown(w))
        .unwrap_or_default();
    let selected = registry.select(req.sources.as_deref());

    // P0 uses the query verbatim. Sub-query planning is a P2 stage; the
    // fan-out below is already shaped for N sub-queries.
    let sub_queries = vec![SubQuery {
        text: req.query.clone(),
        narrow: None,
        lang: req.lang.clone(),
    }];

    let sem = Arc::new(tokio::sync::Semaphore::new(crate::config::fanout_concurrency()));
    let mut tasks = Vec::new();

    for rs in selected {
        for sq in &sub_queries {
            let sem = sem.clone();
            let source = rs.source.clone();
            let sq = sq.clone();
            let budget = Budget {
                max_results: rs.max_results.min(req.limit.max(1)),
                timeout_ms: rs.timeout_ms,
            };
            tasks.push(tokio::spawn(async move {
                let _permit = sem.acquire_owned().await;
                run_one(source, sq, budget).await
            }));
        }
    }

    let mut outcomes = Vec::with_capacity(tasks.len());
    let mut all: Vec<Evidence> = Vec::new();
    for t in tasks {
        match t.await {
            Ok((outcome, items)) => {
                outcomes.push(outcome);
                all.extend(items);
            }
            Err(e) => outcomes.push(SourceOutcome {
                source_id: "?".into(),
                sub_query: req.query.clone(),
                status: "error".into(),
                item_count: 0,
                dropped_count: 0,
                ms: 0,
                error: Some(format!("fan-out task panicked: {e}")),
            }),
        }
    }

    let total_before_dedupe = all.len();
    let mut merged = fusion::dedupe(all);
    fusion::fuse(&mut merged, &registry.weights());
    // Fair-share truncation, not a plain `truncate` — see `select_diverse`.
    fusion::select_diverse(&mut merged, req.limit);

    let deepened = if req.depth >= 2 {
        deepen(transports, &mut merged, DEEPEN_TOP_N).await
    } else {
        0
    };

    SearchOutcome {
        query: req.query.clone(),
        evidence: merged,
        sources: outcomes,
        unknown_sources,
        total_before_dedupe,
        deepened,
        ms: started.elapsed().as_millis() as u64,
    }
}

/// One (source × sub-query) task, with its own error boundary.
async fn run_one(
    source: Arc<dyn crate::sources::SearchSource>,
    sq: SubQuery,
    budget: Budget,
) -> (SourceOutcome, Vec<Evidence>) {
    let started = Instant::now();
    let mut outcome = SourceOutcome {
        source_id: source.id().to_string(),
        sub_query: sq.text.clone(),
        status: "ok".into(),
        item_count: 0,
        dropped_count: 0,
        ms: 0,
        error: None,
    };

    // Probing first means an unavailable source is reported as such rather
    // than as a timeout twenty seconds later.
    if let SourceHealth::Unavailable { reason } = source.health().await {
        outcome.status = "skipped".into();
        outcome.error = Some(reason);
        outcome.ms = started.elapsed().as_millis() as u64;
        return (outcome, vec![]);
    }

    let timeout = Duration::from_millis(budget.timeout_ms);
    let result = tokio::time::timeout(timeout, source.search(&sq, budget)).await;
    outcome.ms = started.elapsed().as_millis() as u64;

    match result {
        Err(_) => {
            outcome.status = "timeout".into();
            outcome.error = Some(format!("quá {} ms", budget.timeout_ms));
            (outcome, vec![])
        }
        Ok(Err(e)) => {
            outcome.status = "error".into();
            outcome.error = Some(e.to_string());
            (outcome, vec![])
        }
        Ok(Ok(mut items)) => {
            // A cap that silently drops results reads as "the source had
            // nothing more". Record what was dropped.
            if items.len() > budget.max_results {
                outcome.dropped_count = items.len() - budget.max_results;
                items.truncate(budget.max_results);
            }
            outcome.item_count = items.len();
            (outcome, items)
        }
    }
}

/// Fetch full page text for the top web results so downstream stages ground on
/// the page rather than a SERP snippet.
///
/// Each fetch runs in its own browser lane — `NewTab` reuses the *calling
/// agent's* tab, so sharing one identity would make concurrent fetches
/// overwrite each other.
pub(crate) async fn deepen(
    transports: &Arc<Transports>,
    evidence: &mut [Evidence],
    top_n: usize,
) -> usize {
    let targets: Vec<usize> = evidence
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.full_text.is_none()
                && e.url.is_some()
                && e.hits.iter().any(|h| h.kind == SourceKind::Web)
        })
        .map(|(i, _)| i)
        .take(top_n)
        .collect();

    let fetches = targets.iter().enumerate().map(|(lane, &idx)| {
        let browser = transports.browser.lane(lane);
        let url = evidence[idx].url.clone().unwrap_or_default();
        async move {
            let text = browser
                .fetch_text(&url, Duration::from_secs(30))
                .await
                .ok()
                .filter(|t| !t.trim().is_empty());
            (idx, text)
        }
    });

    let mut n = 0;
    for (idx, text) in futures_util::future::join_all(fetches).await {
        if let Some(t) = text {
            // Cap stored page text; the LLM stages have a hard input budget and
            // a single long page must not crowd out every other source.
            evidence[idx].full_text = Some(crate::util::truncate_chars(&t, 20_000));
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::SearchSource;
    use async_trait::async_trait;

    struct Fake {
        id: &'static str,
        kind: SourceKind,
        behavior: Behavior,
    }
    enum Behavior {
        Items(usize),
        Fail(&'static str),
        Hang,
        Down(&'static str),
    }

    #[async_trait]
    impl SearchSource for Fake {
        fn id(&self) -> &str {
            self.id
        }
        fn label(&self) -> &str {
            self.id
        }
        fn kind(&self) -> SourceKind {
            self.kind
        }
        async fn health(&self) -> SourceHealth {
            match &self.behavior {
                Behavior::Down(r) => SourceHealth::unavailable(*r),
                _ => SourceHealth::Ready,
            }
        }
        async fn search(&self, _q: &SubQuery, b: Budget) -> anyhow::Result<Vec<Evidence>> {
            match &self.behavior {
                Behavior::Fail(m) => anyhow::bail!("{m}"),
                Behavior::Hang => {
                    tokio::time::sleep(Duration::from_millis(b.timeout_ms * 10)).await;
                    Ok(vec![])
                }
                Behavior::Down(_) => Ok(vec![]),
                Behavior::Items(n) => Ok((0..*n)
                    .map(|i| {
                        Evidence::new(
                            self.id,
                            self.kind,
                            i as u32,
                            1.0,
                            format!("{} {i}", self.id),
                            "body",
                            Some(format!("https://{}.example/{i}", self.id)),
                        )
                    })
                    .collect()),
            }
        }
    }

    fn reg(sources: Vec<Fake>) -> Registry {
        let mut r = Registry::new();
        for s in sources {
            r.register(Arc::new(s));
        }
        r
    }

    #[tokio::test]
    async fn a_failing_source_degrades_the_run_instead_of_failing_it() {
        let r = reg(vec![
            Fake {
                id: "good",
                kind: SourceKind::Web,
                behavior: Behavior::Items(3),
            },
            Fake {
                id: "bad",
                kind: SourceKind::Internal,
                behavior: Behavior::Fail("backend exploded"),
            },
        ]);
        let out = run(&r, &Transports::from_config(), &SearchRequest::new("q")).await;
        assert_eq!(out.evidence.len(), 3, "good source still contributes");
        let bad = out.sources.iter().find(|s| s.source_id == "bad").unwrap();
        assert_eq!(bad.status, "error");
        assert!(bad.error.as_ref().unwrap().contains("backend exploded"));
    }

    #[tokio::test]
    async fn an_unavailable_source_is_skipped_with_its_reason_not_silently_empty() {
        let r = reg(vec![Fake {
            id: "web",
            kind: SourceKind::Web,
            behavior: Behavior::Down("extension chưa kết nối"),
        }]);
        let out = run(&r, &Transports::from_config(), &SearchRequest::new("q")).await;
        assert!(out.evidence.is_empty());
        assert_eq!(out.sources[0].status, "skipped");
        assert_eq!(
            out.sources[0].error.as_deref(),
            Some("extension chưa kết nối")
        );
    }

    #[tokio::test]
    async fn a_hanging_source_times_out_without_blocking_the_others() {
        let mut r = reg(vec![
            Fake {
                id: "slow",
                kind: SourceKind::Web,
                behavior: Behavior::Hang,
            },
            Fake {
                id: "fast",
                kind: SourceKind::Internal,
                behavior: Behavior::Items(2),
            },
        ]);
        r.set_config("slow", None, None, None, Some(1_000));
        let out = run(&r, &Transports::from_config(), &SearchRequest::new("q")).await;
        assert_eq!(out.evidence.len(), 2);
        let slow = out.sources.iter().find(|s| s.source_id == "slow").unwrap();
        assert_eq!(slow.status, "timeout");
    }

    #[tokio::test]
    async fn unknown_source_ids_surface_on_the_outcome() {
        let r = reg(vec![Fake {
            id: "web",
            kind: SourceKind::Web,
            behavior: Behavior::Items(1),
        }]);
        let mut req = SearchRequest::new("q");
        req.sources = Some(vec!["web".into(), "nope".into()]);
        let out = run(&r, &Transports::from_config(), &req).await;
        assert_eq!(out.unknown_sources, vec!["nope".to_string()]);
    }

    #[tokio::test]
    async fn a_cap_that_drops_results_says_so() {
        let mut r = reg(vec![Fake {
            id: "web",
            kind: SourceKind::Web,
            behavior: Behavior::Items(10),
        }]);
        r.set_config("web", None, None, Some(3), None);
        let out = run(&r, &Transports::from_config(), &SearchRequest::new("q")).await;
        // The fake honors `max_results` only via the pipeline's truncation.
        assert_eq!(out.sources[0].item_count, 3);
        assert_eq!(out.sources[0].dropped_count, 7);
    }
}
