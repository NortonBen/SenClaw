//! The catalogue of node types.
//!
//! `GET /api/registry` serves this straight to the UI, which draws handles and
//! config forms from it. Adding a rule is a backend-only change.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use super::spec::{PortSpec, Rule, RuleSpec, SourceRule};

pub enum Entry {
    Rule(Arc<dyn Rule>),
    Source(Arc<dyn SourceRule>),
}

impl Entry {
    pub fn spec(&self) -> &RuleSpec {
        match self {
            Entry::Rule(r) => r.spec(),
            Entry::Source(s) => s.spec(),
        }
    }
    pub fn is_source(&self) -> bool {
        matches!(self, Entry::Source(_))
    }
    pub fn validate(&self, config: &Value) -> Vec<String> {
        match self {
            Entry::Rule(r) => r.validate(config),
            Entry::Source(s) => s.validate(config),
        }
    }
    /// Declared outputs plus any the config adds (switch cases).
    pub fn outputs(&self, config: &Value) -> Vec<PortSpec> {
        let mut out = self.spec().outputs.clone();
        if let Entry::Rule(r) = self {
            for p in r.dynamic_outputs(config) {
                if !out.iter().any(|x| x.id == p.id) {
                    out.push(p);
                }
            }
        }
        out
    }
    /// Declared inputs plus any the config adds (join sources).
    pub fn inputs(&self, config: &Value) -> Vec<PortSpec> {
        let mut inp = self.spec().inputs.clone();
        if let Entry::Rule(r) = self {
            let dynamic = r.dynamic_inputs(config);
            if !dynamic.is_empty() {
                // Dynamic inputs replace the default single `in`.
                inp = dynamic;
            }
        }
        inp
    }
}

#[derive(Default)]
pub struct Registry {
    entries: HashMap<String, Entry>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_rule(&mut self, rule: Arc<dyn Rule>) {
        self.entries
            .insert(rule.spec().id.clone(), Entry::Rule(rule));
    }

    pub fn add_source(&mut self, src: Arc<dyn SourceRule>) {
        self.entries
            .insert(src.spec().id.clone(), Entry::Source(src));
    }

    pub fn get(&self, id: &str) -> Option<&Entry> {
        self.entries.get(id)
    }

    pub fn rule(&self, id: &str) -> Option<Arc<dyn Rule>> {
        match self.entries.get(id) {
            Some(Entry::Rule(r)) => Some(r.clone()),
            _ => None,
        }
    }

    pub fn source(&self, id: &str) -> Option<Arc<dyn SourceRule>> {
        match self.entries.get(id) {
            Some(Entry::Source(s)) => Some(s.clone()),
            _ => None,
        }
    }

    pub fn is_source(&self, id: &str) -> bool {
        self.entries.get(id).map(|e| e.is_source()).unwrap_or(false)
    }

    /// Sorted by category then id so the palette is stable across reloads.
    pub fn specs(&self) -> Vec<&RuleSpec> {
        let mut v: Vec<&RuleSpec> = self.entries.values().map(|e| e.spec()).collect();
        v.sort_by(|a, b| {
            format!("{:?}", a.category)
                .cmp(&format!("{:?}", b.category))
                .then(a.id.cmp(&b.id))
        });
        v
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::spec::{Category, RunCtx};
    use crate::engine::types::{Message, Outcome};
    use async_trait::async_trait;
    use serde_json::json;

    struct Dummy(RuleSpec);

    #[async_trait]
    impl Rule for Dummy {
        fn spec(&self) -> &RuleSpec {
            &self.0
        }
        fn dynamic_outputs(&self, config: &Value) -> Vec<PortSpec> {
            config
                .get("cases")
                .and_then(|c| c.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| PortSpec::new(s, s))
                        .collect()
                })
                .unwrap_or_default()
        }
        async fn handle(&self, _ctx: &RunCtx, _msg: Message) -> Outcome {
            Outcome::Terminal
        }
    }

    fn dummy(id: &str) -> Arc<dyn Rule> {
        Arc::new(Dummy(
            RuleSpec::builder(id, id, Category::Transform).build(),
        ))
    }

    #[test]
    fn dynamic_outputs_extend_the_declared_ones() {
        let mut reg = Registry::new();
        reg.add_rule(dummy("sw"));
        let entry = reg.get("sw").unwrap();
        let ports = entry.outputs(&json!({ "cases": ["hot", "cold"] }));
        let ids: Vec<&str> = ports.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"out"));
        assert!(ids.contains(&"error"));
        assert!(ids.contains(&"hot"));
        assert!(ids.contains(&"cold"));
    }

    #[test]
    fn specs_are_sorted_stably() {
        let mut reg = Registry::new();
        reg.add_rule(dummy("zeta"));
        reg.add_rule(dummy("alpha"));
        let ids: Vec<&str> = reg.specs().iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["alpha", "zeta"]);
    }
}
