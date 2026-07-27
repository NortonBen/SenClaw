//! Core value types of the engine.
//!
//! The one idea that separates this from the Go original (`dipper-engine`): a
//! rule never names the next node. It emits on a *named output port* and the
//! router looks the port up in the edge table. Node ids live in `edges`, never
//! in `config`.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub type NodeId = String;
pub type PortId = String;
pub type ChainId = i64;
pub type RunId = u64;

/// The implicit error port every node has, whether or not it declares one.
pub const PORT_ERROR: &str = "error";
/// Conventional single input / single output names.
pub const PORT_IN: &str = "in";
pub const PORT_OUT: &str = "out";

/// One end of an edge: a port on a node.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PortRef {
    pub node: NodeId,
    pub port: PortId,
}

impl PortRef {
    pub fn new(node: impl Into<NodeId>, port: impl Into<PortId>) -> Self {
        Self {
            node: node.into(),
            port: port.into(),
        }
    }
}

impl std::fmt::Display for PortRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.node, self.port)
    }
}

/// A directed connection between an output port and an input port.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edge {
    pub id: String,
    pub from: PortRef,
    pub to: PortRef,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MsgKind {
    Data,
    Error,
}

/// A failure that travels *inside* a message rather than aborting the run.
///
/// The Go original stored `ErrorDetail error`, which marshals to `{}` and loses
/// the detail across a queue hop — this is a String for that reason.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EngineError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
}

impl EngineError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            node: None,
            rule: None,
        }
    }
    pub fn config(message: impl Into<String>) -> Self {
        Self::new("config_invalid", message)
    }
    pub fn runtime(message: impl Into<String>) -> Self {
        Self::new("runtime_error", message)
    }
    pub fn at(mut self, node: &str, rule: &str) -> Self {
        self.node = Some(node.to_string());
        self.rule = Some(rule.to_string());
        self
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

/// One message in flight.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub run_id: RunId,
    pub chain_id: ChainId,
    /// Monotonic within a run; used to order the debug trace.
    pub seq: u64,
    pub target: PortRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<PortRef>,
    pub data: serde_json::Value,
    pub meta: serde_json::Value,
    pub kind: MsgKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<EngineError>,
    pub ts: i64,
}

impl Message {
    /// Build the first message of a run, aimed at `target`.
    pub fn seed(
        run_id: RunId,
        chain_id: ChainId,
        target: PortRef,
        data: serde_json::Value,
        meta: serde_json::Value,
    ) -> Self {
        Self {
            run_id,
            chain_id,
            seq: 0,
            target,
            from: None,
            data,
            meta,
            kind: MsgKind::Data,
            error: None,
            ts: now_ms(),
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self.kind, MsgKind::Error)
    }
}

/// One message a rule wants to send out of a named port.
#[derive(Clone, Debug)]
pub struct Emission {
    pub port: PortId,
    pub data: serde_json::Value,
    /// `None` keeps the incoming meta unchanged.
    pub meta: Option<serde_json::Value>,
}

impl Emission {
    pub fn new(port: impl Into<PortId>, data: serde_json::Value) -> Self {
        Self {
            port: port.into(),
            data,
            meta: None,
        }
    }
    pub fn with_meta(mut self, meta: serde_json::Value) -> Self {
        self.meta = Some(meta);
        self
    }
}

/// What a rule returns.
///
/// `Fail` routes to the implicit `error` port. If nothing is wired there the
/// branch ends *and the failure is logged* — the Go original dropped it
/// silently, which is why a bad `option` killed a chain with no trace.
#[derive(Debug)]
pub enum Outcome {
    Emit(Vec<Emission>),
    /// End this branch deliberately (a sink did its job).
    Terminal,
    Fail(EngineError),
}

impl Outcome {
    /// Single message on the conventional `out` port.
    pub fn out(data: serde_json::Value) -> Self {
        Outcome::Emit(vec![Emission::new(PORT_OUT, data)])
    }
    /// Single message on a named port.
    pub fn port(port: &str, data: serde_json::Value) -> Self {
        Outcome::Emit(vec![Emission::new(port, data)])
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Monotonic-ish id: milliseconds since epoch shifted left, plus a counter.
/// Enough for run ids and edge ids; avoids pulling in a snowflake crate.
pub fn next_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) & 0x3FF;
    ((now_ms() as u64) << 10) | n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_increasing_within_a_burst() {
        let a = next_id();
        let b = next_id();
        assert_ne!(a, b);
        assert!(b > a);
    }

    #[test]
    fn port_ref_displays_as_node_colon_port() {
        assert_eq!(PortRef::new("n1", "yes").to_string(), "n1:yes");
    }

    #[test]
    fn engine_error_carries_location() {
        let e = EngineError::config("bad expr").at("n2", "conditional");
        assert_eq!(e.code, "config_invalid");
        assert_eq!(e.node.as_deref(), Some("n2"));
        assert_eq!(e.rule.as_deref(), Some("conditional"));
    }
}
