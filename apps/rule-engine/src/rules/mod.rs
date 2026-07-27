//! Node catalogue.
//!
//! Every module here exposes exactly one constructor:
//!   - processing node: `pub fn rule() -> Arc<dyn Rule>`
//!   - source node:     `pub fn source() -> Arc<dyn SourceRule>`
//!
//! `register` is the single place a node becomes visible to both the engine and
//! the UI palette.

use crate::engine::registry::Registry;

// -- logic / transform (pure, no I/O) -------------------------------------
pub mod arithmetic;
pub mod conditional;
pub mod delay;
pub mod fork;
pub mod format;
pub mod join_rule;
pub mod log_rule;
pub mod merge;
pub mod project;
pub mod split;
pub mod switch_rule;
pub mod trigger_time;

// -- filters with state ----------------------------------------------------
pub mod kalman;
pub mod moving_average;

// -- I/O -------------------------------------------------------------------
pub mod http_request;
pub mod notification;
pub mod telegram_send;

// -- SenClaw-native --------------------------------------------------------
pub mod ai_agent;
pub mod knowledge;
pub mod mcp_call;
pub mod senclaw_send;

// -- sources ---------------------------------------------------------------
pub mod manual;
pub mod schedule;
pub mod telegram_hook;
pub mod webhook;

pub fn register(reg: &mut Registry) {
    // logic / transform
    reg.add_rule(arithmetic::rule());
    reg.add_rule(conditional::rule());
    reg.add_rule(switch_rule::rule());
    reg.add_rule(fork::rule());
    reg.add_rule(split::rule());
    reg.add_rule(join_rule::rule());
    reg.add_rule(merge::rule());
    reg.add_rule(format::rule());
    reg.add_rule(project::rule());
    reg.add_rule(trigger_time::rule());
    reg.add_rule(delay::rule());
    reg.add_rule(log_rule::rule());

    // filters
    reg.add_rule(moving_average::rule());
    reg.add_rule(kalman::rule());

    // I/O
    reg.add_rule(http_request::rule());
    reg.add_rule(telegram_send::rule());
    reg.add_rule(notification::rule());

    // SenClaw-native
    reg.add_rule(ai_agent::rule());
    reg.add_rule(knowledge::rule());
    reg.add_rule(mcp_call::rule());
    reg.add_rule(senclaw_send::rule());

    // sources
    reg.add_source(manual::source());
    reg.add_source(webhook::source());
    reg.add_source(schedule::source());
    reg.add_source(telegram_hook::source());
}

/// Shared by the sources that keep a task per deployed node.
pub struct TaskMap {
    inner: std::sync::Mutex<std::collections::HashMap<(i64, String), tokio::task::JoinHandle<()>>>,
}

impl TaskMap {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
    pub fn insert(&self, chain_id: i64, node: &str, handle: tokio::task::JoinHandle<()>) {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(old) = g.insert((chain_id, node.to_string()), handle) {
            old.abort();
        }
    }
    pub fn remove(&self, chain_id: i64, node: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(h) = g.remove(&(chain_id, node.to_string())) {
            h.abort();
        }
    }
    pub fn len(&self) -> usize {
        self.inner.lock().map(|g| g.len()).unwrap_or(0)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for TaskMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry of live source subscriptions keyed by an external id (webhook id,
/// telegram token). Lets an HTTP handler find which node an inbound request
/// belongs to.
pub struct RouteMap {
    inner: std::sync::Mutex<std::collections::HashMap<String, Vec<crate::engine::spec::Emitter>>>,
}

impl RouteMap {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
    pub fn add(&self, key: &str, emitter: crate::engine::spec::Emitter) {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let list = g.entry(key.to_string()).or_default();
        list.retain(|e| !(e.chain_id() == emitter.chain_id() && e.node() == emitter.node()));
        list.push(emitter);
    }
    pub fn remove(&self, chain_id: i64, node: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        g.retain(|_, list| {
            list.retain(|e| !(e.chain_id() == chain_id && e.node() == node));
            !list.is_empty()
        });
    }
    pub fn get(&self, key: &str) -> Vec<crate::engine::spec::Emitter> {
        self.inner
            .lock()
            .map(|g| g.get(key).cloned().unwrap_or_default())
            .unwrap_or_default()
    }
    pub fn keys(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|g| g.keys().cloned().collect())
            .unwrap_or_default()
    }
}

impl Default for RouteMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_installs_every_node_type() {
        let mut reg = Registry::new();
        register(&mut reg);
        // 21 processing nodes + 4 sources
        assert_eq!(reg.len(), 25, "registry size drifted from rules::register");
        assert!(reg.rule("conditional").is_some());
        assert!(reg.source("webhook").is_some());
        assert!(reg.is_source("schedule"));
        assert!(!reg.is_source("conditional"));
    }

    #[test]
    fn every_spec_has_a_unique_id_and_an_error_port() {
        let mut reg = Registry::new();
        register(&mut reg);
        let mut seen = std::collections::HashSet::new();
        for spec in reg.specs() {
            assert!(seen.insert(spec.id.clone()), "trùng id rule `{}`", spec.id);
            assert!(
                spec.outputs.iter().any(|p| p.id == "error"),
                "`{}` thiếu cổng error",
                spec.id
            );
            assert!(!spec.description.is_empty(), "`{}` thiếu mô tả", spec.id);
        }
    }
}
