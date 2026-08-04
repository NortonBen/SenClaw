//! What the page did while nobody was looking.
//!
//! The browser was previously write-only: you could tell it to click, but if the
//! click fired a request that 500'd, or the page threw, or a `confirm()` box
//! went up, none of that reached the agent. It saw a snapshot that had not
//! changed and had no way to find out why. Every serious browser-automation MCP
//! server (Playwright's, Chrome DevTools') exposes console and network for
//! exactly this reason — it is the difference between "the click did nothing"
//! and "the click POSTed /api/login and got a 401".
//!
//! So each page gets a recorder: bounded ring buffers for console lines and
//! network requests, plus the two pieces of state that will otherwise wedge the
//! browser outright — a JavaScript dialog (blocks the renderer, including
//! screenshots, until answered) and a file chooser (blocks the click that opened
//! it).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

/// Keep the last N of each. Enough to explain the action that just happened,
/// small enough that a long-lived tab cannot grow without bound.
const MAX_CONSOLE: usize = 200;
const MAX_REQUESTS: usize = 300;

#[derive(Debug, Clone)]
pub struct ConsoleLine {
    pub level: String,
    pub text: String,
    pub at: i64,
}

#[derive(Debug, Clone)]
pub struct NetRequest {
    pub id: String,
    pub method: String,
    pub url: String,
    pub resource_type: String,
    pub status: Option<i64>,
    pub status_text: String,
    pub mime: String,
    pub failed: Option<String>,
}

impl NetRequest {
    /// Images, fonts, stylesheets and scripts are almost always noise when an
    /// agent asks "what did that button do?". `browser_network_requests`
    /// filters them out unless asked.
    pub fn is_static(&self) -> bool {
        matches!(
            self.resource_type.as_str(),
            "Image" | "Font" | "Stylesheet" | "Script" | "Media" | "Manifest" | "Other"
        )
    }
}

#[derive(Debug, Clone)]
pub struct PendingDialog {
    pub kind: String,
    pub message: String,
    pub default_prompt: String,
    pub at: i64,
}

#[derive(Default)]
struct Inner {
    console: VecDeque<ConsoleLine>,
    requests: VecDeque<NetRequest>,
    dialog: Option<PendingDialog>,
    /// Set while a file chooser is open; the click that opened it is blocked
    /// until we answer with `DOM.setFileInputFiles`.
    file_chooser: Option<i64>,
    downloads: Vec<Value>,
}

/// One recorder per page.
#[derive(Clone, Default)]
pub struct Recorder(Arc<Mutex<Inner>>);

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Recorder {
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        // A poisoned lock here would mean a panic inside a tiny critical
        // section; recovering is strictly better than taking the browser down.
        let mut g = match self.0.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        f(&mut g)
    }

    pub fn push_console(&self, level: &str, text: String) {
        self.with(|i| {
            if i.console.len() >= MAX_CONSOLE {
                i.console.pop_front();
            }
            i.console.push_back(ConsoleLine {
                level: level.to_string(),
                text,
                at: now_ms(),
            });
        });
    }

    pub fn start_request(&self, id: String, method: String, url: String, resource_type: String) {
        self.with(|i| {
            if i.requests.len() >= MAX_REQUESTS {
                i.requests.pop_front();
            }
            i.requests.push_back(NetRequest {
                id,
                method,
                url,
                resource_type,
                status: None,
                status_text: String::new(),
                mime: String::new(),
                failed: None,
            });
        });
    }

    pub fn finish_request(&self, id: &str, status: i64, status_text: String, mime: String) {
        self.with(|i| {
            // Search from the back: a redirect chain reuses the request id, and
            // the most recent leg is the one being completed.
            if let Some(r) = i.requests.iter_mut().rev().find(|r| r.id == id) {
                r.status = Some(status);
                r.status_text = status_text;
                r.mime = mime;
            }
        });
    }

    pub fn fail_request(&self, id: &str, reason: String) {
        self.with(|i| {
            if let Some(r) = i.requests.iter_mut().rev().find(|r| r.id == id) {
                r.failed = Some(reason);
            }
        });
    }

    /// Requests started in the last `ms` milliseconds that have not finished.
    /// Used by the settle heuristic to tell "still loading" from "quiet".
    pub fn in_flight(&self) -> usize {
        self.with(|i| {
            i.requests
                .iter()
                .filter(|r| r.status.is_none() && r.failed.is_none() && !r.is_static())
                .count()
        })
    }

    pub fn console(&self, only_errors: bool, limit: usize) -> Vec<ConsoleLine> {
        self.with(|i| {
            i.console
                .iter()
                .filter(|c| !only_errors || c.level == "error" || c.level == "exception")
                .rev()
                .take(limit)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        })
    }

    pub fn requests(
        &self,
        include_static: bool,
        filter: Option<&str>,
        limit: usize,
    ) -> Vec<NetRequest> {
        let f = filter.map(|s| s.to_lowercase());
        self.with(|i| {
            i.requests
                .iter()
                .filter(|r| include_static || !r.is_static())
                .filter(|r| {
                    f.as_ref()
                        .map(|f| r.url.to_lowercase().contains(f))
                        .unwrap_or(true)
                })
                .rev()
                .take(limit)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect()
        })
    }

    pub fn set_dialog(&self, d: Option<PendingDialog>) {
        self.with(|i| i.dialog = d);
    }
    pub fn dialog(&self) -> Option<PendingDialog> {
        self.with(|i| i.dialog.clone())
    }

    pub fn set_file_chooser(&self, backend_node_id: Option<i64>) {
        self.with(|i| i.file_chooser = backend_node_id);
    }
    pub fn file_chooser(&self) -> Option<i64> {
        self.with(|i| i.file_chooser)
    }

    pub fn push_download(&self, v: Value) {
        self.with(|i| {
            if i.downloads.len() >= 50 {
                i.downloads.remove(0);
            }
            i.downloads.push(v);
        });
    }
    pub fn downloads(&self) -> Vec<Value> {
        self.with(|i| i.downloads.clone())
    }

    /// Called on navigation: the previous page's console and requests describe a
    /// document that no longer exists.
    pub fn reset_for_navigation(&self) {
        self.with(|i| {
            i.console.clear();
            i.requests.clear();
            i.file_chooser = None;
        });
    }

    pub fn console_json(&self, only_errors: bool, limit: usize) -> Value {
        json!(self
            .console(only_errors, limit)
            .into_iter()
            .map(|c| json!({ "level": c.level, "text": c.text, "at": c.at }))
            .collect::<Vec<_>>())
    }

    pub fn requests_json(&self, include_static: bool, filter: Option<&str>, limit: usize) -> Value {
        json!(self
            .requests(include_static, filter, limit)
            .into_iter()
            .map(|r| {
                let mut o = json!({ "method": r.method, "url": r.url, "type": r.resource_type });
                if let Some(s) = r.status {
                    o["status"] = json!(s);
                    if !r.status_text.is_empty() {
                        o["statusText"] = json!(r.status_text);
                    }
                    if !r.mime.is_empty() {
                        o["mime"] = json!(r.mime);
                    }
                } else if r.failed.is_none() {
                    o["status"] = json!("pending");
                }
                if let Some(f) = r.failed {
                    o["failed"] = json!(f);
                }
                o
            })
            .collect::<Vec<_>>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_is_bounded_and_keeps_the_newest() {
        let r = Recorder::new();
        for i in 0..MAX_CONSOLE + 20 {
            r.push_console("log", format!("line {i}"));
        }
        let all = r.console(false, 10_000);
        assert_eq!(all.len(), MAX_CONSOLE);
        assert_eq!(
            all.last().unwrap().text,
            format!("line {}", MAX_CONSOLE + 19)
        );
    }

    #[test]
    fn console_returns_chronological_order_after_limiting() {
        let r = Recorder::new();
        for i in 0..10 {
            r.push_console("log", format!("{i}"));
        }
        let last3 = r.console(false, 3);
        let texts: Vec<&str> = last3.iter().map(|c| c.text.as_str()).collect();
        assert_eq!(
            texts,
            vec!["7", "8", "9"],
            "limit must keep the newest, in order"
        );
    }

    #[test]
    fn error_filter_selects_errors_and_exceptions() {
        let r = Recorder::new();
        r.push_console("log", "chatty".into());
        r.push_console("error", "boom".into());
        r.push_console("exception", "threw".into());
        let errs = r.console(true, 100);
        assert_eq!(errs.len(), 2);
        assert!(errs.iter().all(|c| c.text != "chatty"));
    }

    #[test]
    fn responses_attach_to_their_request() {
        let r = Recorder::new();
        r.start_request(
            "1".into(),
            "POST".into(),
            "https://x/api/login".into(),
            "XHR".into(),
        );
        r.finish_request("1", 401, "Unauthorized".into(), "application/json".into());
        let v = r.requests_json(false, None, 10);
        assert_eq!(v[0]["status"], 401);
        assert_eq!(v[0]["statusText"], "Unauthorized");
    }

    #[test]
    fn static_assets_are_hidden_by_default() {
        let r = Recorder::new();
        r.start_request(
            "1".into(),
            "GET".into(),
            "https://x/logo.png".into(),
            "Image".into(),
        );
        r.start_request(
            "2".into(),
            "GET".into(),
            "https://x/api/me".into(),
            "XHR".into(),
        );
        assert_eq!(r.requests(false, None, 10).len(), 1);
        assert_eq!(r.requests(true, None, 10).len(), 2);
    }

    #[test]
    fn url_filter_is_case_insensitive_substring() {
        let r = Recorder::new();
        r.start_request(
            "1".into(),
            "GET".into(),
            "https://x/API/Users".into(),
            "XHR".into(),
        );
        r.start_request(
            "2".into(),
            "GET".into(),
            "https://x/health".into(),
            "XHR".into(),
        );
        assert_eq!(r.requests(false, Some("api/users"), 10).len(), 1);
    }

    #[test]
    fn in_flight_ignores_static_and_finished() {
        let r = Recorder::new();
        r.start_request(
            "1".into(),
            "GET".into(),
            "https://x/a.png".into(),
            "Image".into(),
        );
        r.start_request(
            "2".into(),
            "GET".into(),
            "https://x/api".into(),
            "XHR".into(),
        );
        assert_eq!(r.in_flight(), 1);
        r.finish_request("2", 200, "OK".into(), "application/json".into());
        assert_eq!(r.in_flight(), 0);
    }

    #[test]
    fn a_failed_request_is_not_in_flight_forever() {
        let r = Recorder::new();
        r.start_request(
            "1".into(),
            "GET".into(),
            "https://x/api".into(),
            "XHR".into(),
        );
        r.fail_request("1", "net::ERR_ABORTED".into());
        assert_eq!(r.in_flight(), 0);
        assert_eq!(
            r.requests_json(false, None, 5)[0]["failed"],
            "net::ERR_ABORTED"
        );
    }

    #[test]
    fn navigation_clears_the_previous_document() {
        let r = Recorder::new();
        r.push_console("log", "old".into());
        r.start_request(
            "1".into(),
            "GET".into(),
            "https://x/".into(),
            "Document".into(),
        );
        r.set_dialog(Some(PendingDialog {
            kind: "alert".into(),
            message: "hi".into(),
            default_prompt: String::new(),
            at: 0,
        }));
        r.reset_for_navigation();
        assert!(r.console(false, 10).is_empty());
        assert!(r.requests(true, None, 10).is_empty());
        // A dialog outlives navigation on purpose: `beforeunload` fires during
        // it, and dropping it would leave the renderer blocked with nobody
        // holding the record that it needs answering.
        assert!(r.dialog().is_some());
    }
}
