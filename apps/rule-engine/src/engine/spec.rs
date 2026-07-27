//! The rule contract: what a node declares, and what it does.
//!
//! Ports are declared data, not hard-coded HTML. The UI reads `RuleSpec` from
//! `GET /api/registry` and draws handles + a config form from it, so adding a
//! rule never means touching the frontend.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::services::Services;
use super::types::*;

/// May a port carry more than one edge?
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PortArity {
    /// At most one edge (a decision branch).
    One,
    /// Any number of edges — every one gets a deep copy (fan-out).
    Many,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PortSpec {
    pub id: PortId,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub arity: PortArity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl PortSpec {
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            color: None,
            arity: PortArity::Many,
            description: None,
        }
    }
    pub fn one(mut self) -> Self {
        self.arity = PortArity::One;
        self
    }
    pub fn color(mut self, c: &str) -> Self {
        self.color = Some(c.to_string());
        self
    }
    pub fn desc(mut self, d: &str) -> Self {
        self.description = Some(d.to_string());
        self
    }

    /// The conventional single input.
    pub fn input() -> Self {
        PortSpec::new(PORT_IN, "in").color("#8c8c8c")
    }
    /// The conventional single output.
    pub fn output() -> Self {
        PortSpec::new(PORT_OUT, "out").color("#52c41a")
    }
    /// Every node gets this whether it declares it or not.
    pub fn error() -> Self {
        PortSpec::new(PORT_ERROR, "error")
            .color("#f5222d")
            .desc("Message lỗi đi ra cổng này. Không nối = nhánh dừng và ghi log.")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Starts runs. No input ports.
    Source,
    Transform,
    Logic,
    Filter,
    Sink,
    Ai,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuleSpec {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Category,
    pub icon: String,
    pub color: String,
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
    /// JSON Schema; the UI generates the config form from it.
    pub config_schema: Value,
    /// Markdown shown in the node's "Document" tab.
    #[serde(default)]
    pub doc: String,
}

impl RuleSpec {
    pub fn builder(id: &str, name: &str, category: Category) -> RuleSpecBuilder {
        RuleSpecBuilder {
            spec: RuleSpec {
                id: id.to_string(),
                name: name.to_string(),
                description: String::new(),
                category,
                icon: "⚙️".to_string(),
                color: "#1890ff".to_string(),
                inputs: match category {
                    Category::Source => vec![],
                    _ => vec![PortSpec::input()],
                },
                outputs: vec![PortSpec::output(), PortSpec::error()],
                config_schema: json!({ "type": "object", "properties": {} }),
                doc: String::new(),
            },
        }
    }

    pub fn has_output(&self, port: &str) -> bool {
        port == PORT_ERROR || self.outputs.iter().any(|p| p.id == port)
    }
    pub fn has_input(&self, port: &str) -> bool {
        self.inputs.iter().any(|p| p.id == port)
    }
    pub fn output(&self, port: &str) -> Option<&PortSpec> {
        self.outputs.iter().find(|p| p.id == port)
    }
}

pub struct RuleSpecBuilder {
    spec: RuleSpec,
}

impl RuleSpecBuilder {
    pub fn desc(mut self, d: &str) -> Self {
        self.spec.description = d.to_string();
        self
    }
    pub fn icon(mut self, i: &str) -> Self {
        self.spec.icon = i.to_string();
        self
    }
    pub fn color(mut self, c: &str) -> Self {
        self.spec.color = c.to_string();
        self
    }
    pub fn inputs(mut self, p: Vec<PortSpec>) -> Self {
        self.spec.inputs = p;
        self
    }
    /// Replaces the default `out`; `error` is appended automatically.
    pub fn outputs(mut self, mut p: Vec<PortSpec>) -> Self {
        if !p.iter().any(|x| x.id == PORT_ERROR) {
            p.push(PortSpec::error());
        }
        self.spec.outputs = p;
        self
    }
    pub fn schema(mut self, s: Value) -> Self {
        self.spec.config_schema = s;
        self
    }
    pub fn doc(mut self, d: &str) -> Self {
        self.spec.doc = d.to_string();
        self
    }
    pub fn build(self) -> RuleSpec {
        self.spec
    }
}

/// Everything a rule sees while handling one message.
pub struct RunCtx {
    pub chain_id: ChainId,
    pub run_id: RunId,
    pub node: NodeId,
    pub rule: String,
    pub config: Value,
    pub svc: Arc<Services>,
}

impl RunCtx {
    pub fn cfg<'a>(&'a self, key: &str) -> Option<&'a Value> {
        self.config.get(key).filter(|v| !v.is_null())
    }
    pub fn cfg_str(&self, key: &str) -> Option<String> {
        self.cfg(key)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.trim().is_empty())
    }
    pub fn cfg_str_or(&self, key: &str, default: &str) -> String {
        self.cfg_str(key).unwrap_or_else(|| default.to_string())
    }
    pub fn cfg_bool(&self, key: &str, default: bool) -> bool {
        self.cfg(key).and_then(|v| v.as_bool()).unwrap_or(default)
    }
    /// Accepts a number or a numeric string — the UI stores both.
    pub fn cfg_f64(&self, key: &str) -> Option<f64> {
        match self.cfg(key)? {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
    pub fn cfg_f64_or(&self, key: &str, default: f64) -> f64 {
        self.cfg_f64(key).unwrap_or(default)
    }
    pub fn cfg_u64_or(&self, key: &str, default: u64) -> u64 {
        self.cfg_f64(key).map(|f| f as u64).unwrap_or(default)
    }

    pub fn err(&self, e: EngineError) -> Outcome {
        Outcome::Fail(e.at(&self.node, &self.rule))
    }
    pub fn fail_config(&self, msg: impl Into<String>) -> Outcome {
        self.err(EngineError::config(msg))
    }
    pub fn fail_runtime(&self, msg: impl Into<String>) -> Outcome {
        self.err(EngineError::runtime(msg))
    }

    /// Per-node persistent state. Replaces the Redis keys the Go rules used
    /// (`rule:ma:{chan}:{node}:{branch}:{field}`).
    pub fn state_get(&self, scope: &str) -> Option<Value> {
        self.svc.state.get(self.chain_id, &self.node, scope)
    }
    pub fn state_set(&self, scope: &str, v: &Value) {
        self.svc.state.set(self.chain_id, &self.node, scope, v);
    }

    pub fn log(&self, level: &str, msg: impl Into<String>) {
        self.svc.log.write(
            self.chain_id,
            Some(self.run_id),
            level,
            Some(&self.node),
            msg.into(),
        );
    }
}

/// A processing node.
///
/// Implementations are stateless and shared across every node and chain that
/// uses them — per-node state goes through `RunCtx::state_*`.
#[async_trait]
pub trait Rule: Send + Sync {
    fn spec(&self) -> &RuleSpec;

    /// Extra output ports derived from config — `switch` grows one per case.
    fn dynamic_outputs(&self, _config: &Value) -> Vec<PortSpec> {
        vec![]
    }
    /// Extra input ports derived from config — `join`/`merge` grow one per source.
    fn dynamic_inputs(&self, _config: &Value) -> Vec<PortSpec> {
        vec![]
    }

    /// Reject bad config at save time instead of at 3am. Return human-readable
    /// Vietnamese messages; they surface in the UI next to the node.
    fn validate(&self, _config: &Value) -> Vec<String> {
        vec![]
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome;
}

/// A message a source pushes into the engine from the outside world.
#[derive(Debug)]
pub struct Ingress {
    pub chain_id: ChainId,
    pub node: NodeId,
    pub port: PortId,
    pub data: Value,
    pub meta: Value,
}

/// Handle a source uses to start a run.
#[derive(Clone)]
pub struct Emitter {
    pub(crate) tx: tokio::sync::mpsc::Sender<Ingress>,
    pub(crate) chain_id: ChainId,
    pub(crate) node: NodeId,
}

impl Emitter {
    /// Each call starts a NEW run. The Go original reused one long-lived
    /// session id, which is why nothing ever completed.
    pub async fn emit(&self, port: &str, data: Value, meta: Value) {
        let _ = self
            .tx
            .send(Ingress {
                chain_id: self.chain_id,
                node: self.node.clone(),
                port: port.to_string(),
                data,
                meta,
            })
            .await;
    }
    pub async fn emit_out(&self, data: Value) {
        self.emit(PORT_OUT, data, json!({})).await;
    }
    pub fn node(&self) -> &str {
        &self.node
    }
    pub fn chain_id(&self) -> ChainId {
        self.chain_id
    }
}

pub struct SourceCtx {
    pub chain_id: ChainId,
    pub node: NodeId,
    pub config: Value,
    pub svc: Arc<Services>,
    pub emitter: Emitter,
}

impl SourceCtx {
    pub fn cfg_str(&self, key: &str) -> Option<String> {
        self.config
            .get(key)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .filter(|s| !s.trim().is_empty())
    }
    pub fn cfg_u64_or(&self, key: &str, default: u64) -> u64 {
        match self.config.get(key) {
            Some(Value::Number(n)) => n.as_u64().unwrap_or(default),
            Some(Value::String(s)) => s.trim().parse().unwrap_or(default),
            _ => default,
        }
    }
    pub fn log(&self, level: &str, msg: impl Into<String>) {
        self.svc
            .log
            .write(self.chain_id, None, level, Some(&self.node), msg.into());
    }
}

/// A node that starts runs instead of processing them.
#[async_trait]
pub trait SourceRule: Send + Sync {
    fn spec(&self) -> &RuleSpec;

    fn validate(&self, _config: &Value) -> Vec<String> {
        vec![]
    }

    /// Register listeners and return. Long-lived work belongs in a spawned task
    /// keyed by `(chain_id, node)` so `stop` can cancel it.
    async fn start(&self, ctx: SourceCtx) -> Result<(), String>;

    /// Must actually release resources — the Go engine never called `Stop()`.
    async fn stop(&self, chain_id: ChainId, node: &str);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_port_is_appended_automatically() {
        let spec = RuleSpec::builder("x", "X", Category::Transform)
            .outputs(vec![PortSpec::new("yes", "yes"), PortSpec::new("no", "no")])
            .build();
        assert!(spec.has_output("yes"));
        assert!(spec.has_output(PORT_ERROR));
        assert_eq!(spec.outputs.len(), 3);
    }

    #[test]
    fn source_specs_have_no_inputs_by_default() {
        let spec = RuleSpec::builder("s", "S", Category::Source).build();
        assert!(spec.inputs.is_empty());
    }

    #[test]
    fn has_output_accepts_error_even_when_undeclared() {
        let spec = RuleSpec::builder("x", "X", Category::Sink)
            .outputs(vec![PortSpec::output()])
            .build();
        assert!(spec.has_output(PORT_ERROR));
    }
}
