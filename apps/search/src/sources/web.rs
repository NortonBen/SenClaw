//! Public-web source, over the browser bridge.
//!
//! Honest limits (all surfaced through [`health`], never as an empty result):
//!   * needs the SenClaw Chrome extension attached to the bridge;
//!   * the SERP is scraped from a real tab, so Google CAPTCHA / "unusual
//!     traffic" is a hard error (`SearchEngine.ts:25-35`) — we fail over to
//!     Bing once per run and then stop asking;
//!   * `country` / `safe_search` exist on `browser_search`'s params but are
//!     dropped before the wire (`browser_server.rs:1262`), so we don't offer
//!     them.

use crate::model::{Budget, Evidence, SourceHealth, SourceKind, SubQuery};
use crate::sources::SearchSource;
use crate::transport::BrowserWs;
use async_trait::async_trait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub struct WebSource {
    browser: BrowserWs,
    /// Set once Google has rate-limited us; every later query goes to Bing.
    /// Sticky for the process lifetime — retrying Google after a block just
    /// deepens it.
    google_blocked: AtomicBool,
}

impl WebSource {
    pub fn new(browser: BrowserWs) -> Self {
        Self {
            browser,
            google_blocked: AtomicBool::new(false),
        }
    }

    fn engine(&self) -> &'static str {
        if self.google_blocked.load(Ordering::Relaxed) {
            "bing"
        } else {
            "google"
        }
    }
}

/// The extension throws on CAPTCHA / rate-limit rather than returning results.
fn looks_rate_limited(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("captcha")
        || e.contains("unusual traffic")
        || e.contains("rate limit")
        || e.contains("/sorry/")
}

#[async_trait]
impl SearchSource for WebSource {
    fn id(&self) -> &str {
        "web"
    }
    fn label(&self) -> &str {
        "Web"
    }
    fn kind(&self) -> SourceKind {
        SourceKind::Web
    }

    async fn health(&self) -> SourceHealth {
        match self.browser.is_connected().await {
            Ok(_) if self.google_blocked.load(Ordering::Relaxed) => SourceHealth::degraded(
                "Google đã chặn (CAPTCHA) — đang dùng Bing cho phiên này",
            ),
            Ok(_) => SourceHealth::Ready,
            Err(e) => SourceHealth::unavailable(format!(
                "không kết nối được browser bridge ({e}) — cần Chrome extension của SenClaw"
            )),
        }
    }

    async fn search(&self, q: &SubQuery, budget: Budget) -> anyhow::Result<Vec<Evidence>> {
        let timeout = Duration::from_millis(budget.timeout_ms);
        let n = budget.max_results.clamp(1, 50) as u8;

        let mut results = self
            .browser
            .search(&q.text, self.engine(), n, q.lang.as_deref(), timeout)
            .await;

        // One failover: Google blocked → Bing, then remember it.
        if let Err(e) = &results {
            if looks_rate_limited(&e.to_string()) && !self.google_blocked.load(Ordering::Relaxed) {
                self.google_blocked.store(true, Ordering::Relaxed);
                results = self
                    .browser
                    .search(&q.text, "bing", n, q.lang.as_deref(), timeout)
                    .await;
            }
        }

        let serp = results?;
        Ok(serp
            .results
            .into_iter()
            .enumerate()
            .filter(|(_, r)| !r.url.trim().is_empty())
            .map(|(i, r)| {
                // SERP position is 1-based and occasionally missing; the
                // enumeration index is the reliable rank.
                let rank = if r.position > 0 {
                    r.position as u32 - 1
                } else {
                    i as u32
                };
                let mut ev = Evidence::new(
                    self.id(),
                    self.kind(),
                    rank,
                    1.0 / (1.0 + rank as f32),
                    r.title,
                    r.snippet,
                    Some(r.url),
                );
                ev.lang = q.lang.clone();
                ev
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit_detection_matches_the_extension_error_strings() {
        assert!(looks_rate_limited("Search blocked: CAPTCHA detected"));
        assert!(looks_rate_limited(
            "google.com/sorry/index — unusual traffic"
        ));
        assert!(!looks_rate_limited("browser bridge connect failed"));
    }

    #[test]
    fn engine_switches_to_bing_once_google_is_blocked() {
        let s = WebSource::new(BrowserWs::new("ws://127.0.0.1:1/browser-mcp", "t"));
        assert_eq!(s.engine(), "google");
        s.google_blocked.store(true, Ordering::Relaxed);
        assert_eq!(s.engine(), "bing");
    }
}
