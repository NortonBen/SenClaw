//! Helpers for rule unit tests. Compiled only under `cfg(test)`.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::db::Db;
use crate::engine::services::{EventBus, Services};
use crate::engine::spec::RunCtx;
use crate::engine::types::{Message, Outcome, PortRef, PORT_IN};

/// A `RunCtx` backed by an in-memory database, so a rule test never touches
/// `~/.senclaw` and never needs a running daemon.
pub fn ctx(rule: &str, config: Value) -> RunCtx {
    let db = Arc::new(Db::open(":memory:").expect("in-memory db"));
    let _ = db.create_chain(1, "test", "");
    let svc = Arc::new(Services::new(db, EventBus::new()));
    RunCtx {
        chain_id: 1,
        run_id: 1,
        node: "n1".to_string(),
        rule: rule.to_string(),
        config,
        svc,
    }
}

pub fn msg(data: Value) -> Message {
    Message::seed(1, 1, PortRef::new("n1", PORT_IN), data, json!({}))
}

pub fn msg_with_meta(data: Value, meta: Value) -> Message {
    Message::seed(1, 1, PortRef::new("n1", PORT_IN), data, meta)
}

/// `(port, data)` pairs of an `Outcome::Emit`; panics on any other outcome so
/// the failure message points at the real problem.
pub fn emitted(outcome: Outcome) -> Vec<(String, Value)> {
    match outcome {
        Outcome::Emit(v) => v.into_iter().map(|e| (e.port, e.data)).collect(),
        Outcome::Terminal => panic!("mong đợi Emit, nhận Terminal"),
        Outcome::Fail(e) => panic!("mong đợi Emit, nhận Fail: {e}"),
    }
}

/// The single emission of a rule that fans out to exactly one port.
pub fn one(outcome: Outcome) -> (String, Value) {
    let mut v = emitted(outcome);
    assert_eq!(v.len(), 1, "mong đợi đúng 1 emission");
    v.remove(0)
}

pub fn failure(outcome: Outcome) -> String {
    match outcome {
        Outcome::Fail(e) => e.message,
        Outcome::Terminal => panic!("mong đợi Fail, nhận Terminal"),
        Outcome::Emit(v) => panic!(
            "mong đợi Fail, nhận Emit trên cổng {:?}",
            v.iter().map(|e| e.port.clone()).collect::<Vec<_>>()
        ),
    }
}

pub fn is_terminal(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Terminal)
}
