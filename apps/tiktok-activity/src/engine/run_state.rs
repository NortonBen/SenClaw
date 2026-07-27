//! Per-run state + `{{template}}` substitution — ported from
//! internal/engine/playwrightexec/run_state.go.
//!
//! One `RunState` is shared for the lifetime of a run. Executors read the
//! previous step's output via `{{prev.key}}`, any step by id via
//! `{{step.<id>.key}}`, and run params via `{{param.key}}`/`{{params.key}}`.
//! The Go code guarded it with a RWMutex because the Dispatcher and handlers
//! touched it concurrently; here a `Mutex` behind an `Arc` plays the same role.

use crate::domain::{FlowAction, StrMap};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

static CFG_TPL: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{\{([^{}]+)\}\}").unwrap());

#[derive(Default)]
struct Inner {
    last: StrMap,
    by_step: BTreeMap<String, StrMap>,
    params: StrMap,
    step_extra: StrMap,
}

#[derive(Clone, Default)]
pub struct RunState {
    inner: Arc<Mutex<Inner>>,
}

impl RunState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_params(&self, p: Option<&StrMap>) {
        let mut g = self.inner.lock().unwrap();
        g.params = p.cloned().unwrap_or_default();
    }

    pub fn merge_params(&self, patch: &StrMap) {
        if patch.is_empty() {
            return;
        }
        let mut g = self.inner.lock().unwrap();
        for (k, v) in patch {
            if k.trim().is_empty() {
                continue;
            }
            g.params.insert(k.clone(), v.clone());
        }
    }

    pub fn get_param(&self, key: &str) -> Option<String> {
        let g = self.inner.lock().unwrap();
        g.params.get(key).cloned()
    }

    pub fn save_step_output(&self, step_id: &str, out: StrMap) {
        let mut g = self.inner.lock().unwrap();
        g.last = out.clone();
        if !step_id.trim().is_empty() {
            g.by_step.insert(step_id.to_string(), out);
        }
    }

    pub fn reset_step_extras(&self) {
        let mut g = self.inner.lock().unwrap();
        g.step_extra = StrMap::new();
    }

    pub fn add_step_extra(&self, key: &str, value: &str) {
        let key = key.trim();
        if key.is_empty() {
            return;
        }
        let mut g = self.inner.lock().unwrap();
        g.step_extra.insert(key.to_string(), value.to_string());
    }

    pub fn take_step_extras(&self) -> StrMap {
        let mut g = self.inner.lock().unwrap();
        std::mem::take(&mut g.step_extra)
    }

    /// Latest value for `key`, checking last step → other steps → params.
    pub fn lookup_extract(&self, key: &str) -> Option<String> {
        let key = key.trim();
        if key.is_empty() {
            return None;
        }
        let g = self.inner.lock().unwrap();
        if let Some(v) = g.last.get(key) {
            if !v.trim().is_empty() {
                return Some(v.clone());
            }
        }
        for out in g.by_step.values() {
            if let Some(v) = out.get(key) {
                if !v.trim().is_empty() {
                    return Some(v.clone());
                }
            }
        }
        if let Some(v) = g.params.get(key) {
            if !v.trim().is_empty() {
                return Some(v.clone());
            }
        }
        None
    }

    fn resolve_value(&self, raw: &str) -> String {
        if raw.is_empty() {
            return raw.to_string();
        }
        let g = self.inner.lock().unwrap();
        CFG_TPL
            .replace_all(raw, |caps: &regex::Captures| {
                let whole = &caps[0];
                let inner = caps[1].trim();
                if inner.is_empty() {
                    return whole.to_string();
                }
                if let Some(key) = inner.strip_prefix("prev.") {
                    return g.last.get(key.trim()).cloned().unwrap_or_else(|| whole.to_string());
                }
                if let Some(rest) = inner.strip_prefix("step.") {
                    let rest = rest.trim();
                    if let Some(dot) = rest.find('.') {
                        if dot > 0 && dot < rest.len() - 1 {
                            let step_id = rest[..dot].trim();
                            let key = rest[dot + 1..].trim();
                            return g
                                .by_step
                                .get(step_id)
                                .and_then(|m| m.get(key))
                                .cloned()
                                .unwrap_or_else(|| whole.to_string());
                        }
                    }
                    return whole.to_string();
                }
                if let Some(key) = inner.strip_prefix("param.") {
                    return g.params.get(key.trim()).cloned().unwrap_or_else(|| whole.to_string());
                }
                if let Some(key) = inner.strip_prefix("params.") {
                    return g.params.get(key.trim()).cloned().unwrap_or_else(|| whole.to_string());
                }
                whole.to_string()
            })
            .into_owned()
    }

    pub fn resolve_config(&self, cfg: &StrMap) -> StrMap {
        cfg.iter().map(|(k, v)| (k.clone(), self.resolve_value(v))).collect()
    }

    pub fn resolve_config_opt(&self, cfg: &Option<StrMap>) -> Option<StrMap> {
        cfg.as_ref().map(|c| self.resolve_config(c))
    }

    /// Resolve templates across config, params and atomic params of one action.
    pub fn resolve_action(&self, mut action: FlowAction) -> FlowAction {
        action.config = self.resolve_config(&action.config);
        action.params = self.resolve_config_opt(&action.params);
        if !action.atomics.is_empty() {
            for at in action.atomics.iter_mut() {
                at.params = self.resolve_config_opt(&at.params);
            }
        }
        action
    }

    pub fn render_for_log(&self, v: &str) -> String {
        self.resolve_value(v)
    }
}

/// The default per-step output map the Dispatcher seeds before merging extras.
pub fn step_default_output(action_type: &str, action_name: &str, page_url: &str) -> StrMap {
    let mut m = StrMap::new();
    m.insert("type".into(), action_type.into());
    m.insert("name".into(), action_name.into());
    m.insert("url".into(), page_url.into());
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn param_and_prev_and_step_templates() {
        let rs = RunState::new();
        let mut p = StrMap::new();
        p.insert("who".into(), "world".into());
        rs.set_params(Some(&p));

        let mut out = StrMap::new();
        out.insert("vid".into(), "123".into());
        rs.save_step_output("s1", out);

        let mut cfg = StrMap::new();
        cfg.insert("a".into(), "hi {{param.who}}".into());
        cfg.insert("b".into(), "prev={{prev.vid}}".into());
        cfg.insert("c".into(), "by-id={{step.s1.vid}}".into());
        cfg.insert("d".into(), "miss={{param.none}}".into());
        let r = rs.resolve_config(&cfg);
        assert_eq!(r["a"], "hi world");
        assert_eq!(r["b"], "prev=123");
        assert_eq!(r["c"], "by-id=123");
        // Unknown keys are left verbatim (matches the Go behaviour).
        assert_eq!(r["d"], "miss={{param.none}}");
    }

    #[test]
    fn merge_params_and_lookup() {
        let rs = RunState::new();
        let mut patch = StrMap::new();
        patch.insert("k".into(), "v".into());
        rs.merge_params(&patch);
        assert_eq!(rs.get_param("k").as_deref(), Some("v"));
        assert_eq!(rs.lookup_extract("k").as_deref(), Some("v"));
    }
}
