//! Multi-input barriers.
//!
//! The Go engine had nothing here: two edges into one node meant that node ran
//! twice. A node with `join: all` instead waits for one message on each of its
//! *connected* input ports and fires once.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Map, Value};

use super::graph::CompiledNode;
use super::types::*;
use crate::model::JoinPolicy;

type JoinKey = (RunId, NodeId, String);

struct Slot {
    parts: HashMap<PortId, Message>,
    deadline_ms: i64,
}

pub enum Gate {
    /// Run the node now with these parts (one element for `join: any`).
    Fire(Vec<Message>),
    /// Held; the message stays counted as in-flight so the run does not look
    /// finished while a barrier is still waiting.
    Park,
}

#[derive(Debug)]
pub struct Expired {
    pub run_id: RunId,
    pub node: NodeId,
    pub parts: Vec<Message>,
}

#[derive(Default)]
pub struct JoinTable {
    slots: Mutex<HashMap<JoinKey, Vec<Slot>>>,
}

impl JoinTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn gate(&self, node: &CompiledNode, msg: Message, default_timeout_ms: u64) -> Gate {
        if node.opts.join == JoinPolicy::Any {
            return Gate::Fire(vec![msg]);
        }
        // Wait on exactly the ports someone wired up.
        let expected: Vec<PortId> = if node.connected_inputs.is_empty() {
            vec![msg.target.port.clone()]
        } else {
            node.connected_inputs.clone()
        };
        if expected.len() < 2 {
            return Gate::Fire(vec![msg]);
        }

        let corr = node
            .opts
            .corr_key
            .as_ref()
            .and_then(|k| crate::daq::get(&msg.data, k))
            .map(|v| value_key(&v))
            .unwrap_or_default();
        let key: JoinKey = (msg.run_id, node.id.clone(), corr);
        let timeout = node.opts.join_timeout_ms.unwrap_or(default_timeout_ms) as i64;

        let mut guard = match self.slots.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let list = guard.entry(key.clone()).or_default();
        let port = msg.target.port.clone();

        // Fill the oldest generation that is still missing this port; a later
        // arrival on an already-filled port starts a new generation.
        let idx = list.iter().position(|s| !s.parts.contains_key(&port));
        let idx = match idx {
            Some(i) => i,
            None => {
                list.push(Slot {
                    parts: HashMap::new(),
                    deadline_ms: now_ms() + timeout,
                });
                list.len() - 1
            }
        };
        list[idx].parts.insert(port, msg);

        if expected.iter().all(|p| list[idx].parts.contains_key(p)) {
            let slot = list.remove(idx);
            if list.is_empty() {
                guard.remove(&key);
            }
            let mut parts: Vec<Message> = slot.parts.into_values().collect();
            parts.sort_by(|a, b| a.target.port.cmp(&b.target.port));
            Gate::Fire(parts)
        } else {
            Gate::Park
        }
    }

    /// Slots past their deadline. Callers must release the in-flight count for
    /// every returned message.
    pub fn take_expired(&self) -> Vec<Expired> {
        let now = now_ms();
        let mut out = Vec::new();
        let mut guard = match self.slots.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.retain(|key, list| {
            let mut keep = Vec::new();
            for slot in list.drain(..) {
                if slot.deadline_ms <= now {
                    out.push(Expired {
                        run_id: key.0,
                        node: key.1.clone(),
                        parts: slot.parts.into_values().collect(),
                    });
                } else {
                    keep.push(slot);
                }
            }
            *list = keep;
            !list.is_empty()
        });
        out
    }

    /// Drop everything belonging to a run (it failed or was reaped).
    pub fn drop_run(&self, run_id: RunId) -> Vec<Message> {
        let mut out = Vec::new();
        let mut guard = match self.slots.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.retain(|key, list| {
            if key.0 != run_id {
                return true;
            }
            for slot in list.drain(..) {
                out.extend(slot.parts.into_values());
            }
            false
        });
        out
    }

    pub fn drop_chain(&self, chain_id: ChainId) {
        let mut guard = match self.slots.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.retain(|_, list| {
            list.retain(|s| {
                s.parts
                    .values()
                    .next()
                    .map(|m| m.chain_id != chain_id)
                    .unwrap_or(false)
            });
            !list.is_empty()
        });
    }

    pub fn pending(&self) -> usize {
        self.slots
            .lock()
            .map(|g| g.values().map(|v| v.len()).sum())
            .unwrap_or(0)
    }
}

/// Fold the parts of a fired barrier into the single message the rule sees.
pub fn combine(policy: JoinPolicy, mut parts: Vec<Message>) -> Message {
    if parts.len() == 1 {
        return parts.remove(0);
    }
    // The lowest seq wins as the carrier so the trace stays ordered.
    parts.sort_by_key(|m| m.seq);
    let mut base = parts[0].clone();
    let any_error = parts.iter().any(|m| m.is_error());

    match policy {
        JoinPolicy::Merge => {
            let mut acc = Value::Object(Map::new());
            for m in &parts {
                deep_merge(&mut acc, &m.data);
            }
            base.data = acc;
        }
        _ => {
            let mut obj = Map::new();
            for m in &parts {
                obj.insert(m.target.port.clone(), m.data.clone());
            }
            base.data = Value::Object(obj);
        }
    }

    let mut meta = base.meta.take_object();
    meta.insert(
        "_join".to_string(),
        json!({
            "ports": parts.iter().map(|m| m.target.port.clone()).collect::<Vec<_>>(),
            "count": parts.len(),
        }),
    );
    base.meta = Value::Object(meta);

    if any_error {
        base.kind = MsgKind::Error;
        base.error = parts.iter().find_map(|m| m.error.clone());
    }
    base
}

fn deep_merge(dst: &mut Value, src: &Value) {
    match (dst, src) {
        (Value::Object(d), Value::Object(s)) => {
            for (k, v) in s {
                match d.get_mut(k) {
                    Some(existing) => deep_merge(existing, v),
                    None => {
                        d.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (d, s) => *d = s.clone(),
    }
}

fn value_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

trait TakeObject {
    fn take_object(&mut self) -> Map<String, Value>;
}

impl TakeObject for Value {
    fn take_object(&mut self) -> Map<String, Value> {
        match std::mem::replace(self, Value::Null) {
            Value::Object(m) => m,
            _ => Map::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::NodeOpts;
    use std::collections::HashMap as Map2;
    use std::sync::Arc;

    fn node(join: JoinPolicy, inputs: &[&str]) -> Arc<CompiledNode> {
        Arc::new(CompiledNode {
            id: "j".into(),
            rule: "join".into(),
            name: "j".into(),
            config: json!({}),
            opts: NodeOpts {
                join,
                ..Default::default()
            },
            debug: false,
            out: Map2::new(),
            connected_inputs: inputs.iter().map(|s| s.to_string()).collect(),
        })
    }

    fn msg(port: &str, data: Value) -> Message {
        let mut m = Message::seed(1, 1, PortRef::new("j", port), data, json!({}));
        m.seq = 1;
        m
    }

    #[test]
    fn any_fires_immediately() {
        let t = JoinTable::new();
        let n = node(JoinPolicy::Any, &["a", "b"]);
        match t.gate(&n, msg("a", json!({"x":1})), 1000) {
            Gate::Fire(v) => assert_eq!(v.len(), 1),
            Gate::Park => panic!("should fire"),
        }
    }

    #[test]
    fn all_waits_for_every_connected_port() {
        let t = JoinTable::new();
        let n = node(JoinPolicy::All, &["a", "b"]);
        assert!(matches!(
            t.gate(&n, msg("a", json!({"x":1})), 1000),
            Gate::Park
        ));
        assert_eq!(t.pending(), 1);
        match t.gate(&n, msg("b", json!({"y":2})), 1000) {
            Gate::Fire(parts) => {
                assert_eq!(parts.len(), 2);
                let c = combine(JoinPolicy::All, parts);
                assert_eq!(c.data["a"]["x"], 1);
                assert_eq!(c.data["b"]["y"], 2);
                assert_eq!(c.meta["_join"]["count"], 2);
            }
            Gate::Park => panic!("should fire"),
        }
        assert_eq!(t.pending(), 0);
    }

    #[test]
    fn a_repeat_on_the_same_port_opens_a_new_generation() {
        let t = JoinTable::new();
        let n = node(JoinPolicy::All, &["a", "b"]);
        assert!(matches!(t.gate(&n, msg("a", json!(1)), 1000), Gate::Park));
        assert!(matches!(t.gate(&n, msg("a", json!(2)), 1000), Gate::Park));
        assert_eq!(t.pending(), 2);
        // First `b` completes the OLDEST generation.
        match t.gate(&n, msg("b", json!(3)), 1000) {
            Gate::Fire(parts) => {
                let c = combine(JoinPolicy::All, parts);
                assert_eq!(c.data["a"], 1);
            }
            Gate::Park => panic!("should fire"),
        }
        assert_eq!(t.pending(), 1);
    }

    #[test]
    fn merge_deep_merges_the_parts() {
        let parts = vec![
            msg("a", json!({"user": {"id": 1}})),
            msg("b", json!({"user": {"name": "x"}, "extra": true})),
        ];
        let c = combine(JoinPolicy::Merge, parts);
        assert_eq!(c.data["user"]["id"], 1);
        assert_eq!(c.data["user"]["name"], "x");
        assert_eq!(c.data["extra"], true);
    }

    #[test]
    fn an_error_part_poisons_the_combined_message() {
        let mut bad = msg("b", json!({}));
        bad.kind = MsgKind::Error;
        bad.error = Some(EngineError::runtime("boom"));
        let c = combine(JoinPolicy::All, vec![msg("a", json!({})), bad]);
        assert!(c.is_error());
        assert_eq!(c.error.unwrap().message, "boom");
    }

    #[test]
    fn expired_slots_come_back_with_their_parts() {
        let t = JoinTable::new();
        let n = node(JoinPolicy::All, &["a", "b"]);
        assert!(matches!(t.gate(&n, msg("a", json!(1)), 0), Gate::Park));
        let exp = t.take_expired();
        assert_eq!(exp.len(), 1);
        assert_eq!(exp[0].parts.len(), 1);
        assert_eq!(t.pending(), 0);
    }

    #[test]
    fn corr_key_separates_unrelated_items() {
        let t = JoinTable::new();
        let mut n = node(JoinPolicy::All, &["a", "b"]);
        Arc::get_mut(&mut n).unwrap().opts.corr_key = Some("id".into());
        assert!(matches!(
            t.gate(&n, msg("a", json!({"id": "x"})), 1000),
            Gate::Park
        ));
        // Different correlation id: must NOT complete the first barrier.
        assert!(matches!(
            t.gate(&n, msg("b", json!({"id": "y"})), 1000),
            Gate::Park
        ));
        assert_eq!(t.pending(), 2);
        assert!(matches!(
            t.gate(&n, msg("b", json!({"id": "x"})), 1000),
            Gate::Fire(_)
        ));
    }
}
