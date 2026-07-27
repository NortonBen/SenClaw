//! Compiling a stored graph into the routing table the scheduler runs on, and
//! checking it before anyone deploys it.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use super::registry::Registry;
use super::spec::PortArity;
use super::types::*;
use crate::model::{JoinPolicy, Node, NodeOpts};

pub struct CompiledNode {
    pub id: NodeId,
    pub rule: String,
    pub name: String,
    pub config: Value,
    pub opts: NodeOpts,
    pub debug: bool,
    /// output port -> where its messages go
    pub out: HashMap<PortId, Vec<PortRef>>,
    /// input ports that actually have an edge into them (join `All` waits on
    /// exactly these, not on every declared port)
    pub connected_inputs: Vec<PortId>,
}

impl CompiledNode {
    pub fn targets(&self, port: &str) -> &[PortRef] {
        self.out.get(port).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

pub struct CompiledGraph {
    pub chain_id: ChainId,
    pub debug: bool,
    pub nodes: HashMap<NodeId, Arc<CompiledNode>>,
    /// Node ids whose rule is a `SourceRule`.
    pub sources: Vec<NodeId>,
}

impl CompiledGraph {
    pub fn node(&self, id: &str) -> Option<&Arc<CompiledNode>> {
        self.nodes.get(id)
    }
}

pub fn compile(
    chain_id: ChainId,
    debug: bool,
    nodes: &[Node],
    edges: &[Edge],
    registry: &Registry,
) -> CompiledGraph {
    let mut out_map: HashMap<NodeId, HashMap<PortId, Vec<PortRef>>> = HashMap::new();
    let mut in_map: HashMap<NodeId, Vec<PortId>> = HashMap::new();

    for e in edges {
        out_map
            .entry(e.from.node.clone())
            .or_default()
            .entry(e.from.port.clone())
            .or_default()
            .push(e.to.clone());
        let ports = in_map.entry(e.to.node.clone()).or_default();
        if !ports.contains(&e.to.port) {
            ports.push(e.to.port.clone());
        }
    }

    let mut compiled = HashMap::new();
    let mut sources = Vec::new();
    for n in nodes {
        if registry.is_source(&n.rule) {
            sources.push(n.id.clone());
        }
        compiled.insert(
            n.id.clone(),
            Arc::new(CompiledNode {
                id: n.id.clone(),
                rule: n.rule.clone(),
                name: if n.name.is_empty() {
                    n.id.clone()
                } else {
                    n.name.clone()
                },
                config: n.config.clone(),
                opts: n.opts.clone(),
                debug: n.debug,
                out: out_map.remove(&n.id).unwrap_or_default(),
                connected_inputs: in_map.remove(&n.id).unwrap_or_default(),
            }),
        );
    }
    sources.sort();

    CompiledGraph {
        chain_id,
        debug,
        nodes: compiled,
        sources,
    }
}

// ------------------------------------------------------------- validation

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueLevel {
    /// Blocks activation.
    Error,
    /// Allowed, but probably not what you meant.
    Warning,
}

#[derive(Clone, Debug, Serialize)]
pub struct Issue {
    pub level: IssueLevel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    pub message: String,
}

impl Issue {
    fn error(node: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            level: IssueLevel::Error,
            node: node.map(|s| s.to_string()),
            edge: None,
            message: message.into(),
        }
    }
    fn warn(node: Option<&str>, message: impl Into<String>) -> Self {
        Self {
            level: IssueLevel::Warning,
            node: node.map(|s| s.to_string()),
            edge: None,
            message: message.into(),
        }
    }
    fn edge_error(edge: &str, message: impl Into<String>) -> Self {
        Self {
            level: IssueLevel::Error,
            node: None,
            edge: Some(edge.to_string()),
            message: message.into(),
        }
    }
}

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.level == IssueLevel::Error)
}

/// Everything that can be checked without running the graph.
pub fn validate(nodes: &[Node], edges: &[Edge], registry: &Registry) -> Vec<Issue> {
    let mut issues = Vec::new();
    let by_id: HashMap<&str, &Node> = nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    if nodes.is_empty() {
        issues.push(Issue::warn(None, "Luồng chưa có node nào."));
    }

    // Duplicate ids would silently shadow each other in the compiled map.
    let mut seen = HashSet::new();
    for n in nodes {
        if !seen.insert(n.id.as_str()) {
            issues.push(Issue::error(
                Some(&n.id),
                format!("Trùng node id `{}`.", n.id),
            ));
        }
    }

    let mut source_count = 0usize;
    for n in nodes {
        let Some(entry) = registry.get(&n.rule) else {
            issues.push(Issue::error(
                Some(&n.id),
                format!("Không có loại node `{}` trong registry.", n.rule),
            ));
            continue;
        };
        if entry.is_source() {
            source_count += 1;
        }
        for msg in entry.validate(&n.config) {
            issues.push(Issue::error(Some(&n.id), msg));
        }
        // `join` / `merge` are inert unless the node carries the matching join
        // policy: with the default `any`, the barrier never engages and the
        // node just fires once per incoming message. Catch the mismatch here so
        // a decorative join can't be activated.
        let required_join = match n.rule.as_str() {
            "join" => Some(JoinPolicy::All),
            "merge" => Some(JoinPolicy::Merge),
            _ => None,
        };
        if let Some(req) = required_join {
            if n.opts.join != req {
                issues.push(Issue::error(
                    Some(&n.id),
                    format!(
                        "Node `{}` cần đặt opts.join = `{:?}` (đang là `{:?}`), nếu không nó sẽ không gộp mà chạy nhiều lần.",
                        n.rule, req, n.opts.join
                    ),
                ));
            }
        }
        if n.opts.join != JoinPolicy::Any {
            let inbound: Vec<&Edge> = edges.iter().filter(|e| e.to.node == n.id).collect();
            if inbound.len() < 2 {
                issues.push(Issue::warn(
                    Some(&n.id),
                    format!(
                        "Node đặt join `{:?}` nhưng chỉ có {} cổng vào được nối — nó sẽ chờ mãi.",
                        n.opts.join,
                        inbound.len()
                    ),
                ));
            } else {
                // A barrier fed entirely by the decision ports of one upstream
                // (a conditional's true/false, a switch's cases — all `arity:
                // One`, and mutually exclusive) can never fill: only one branch
                // ever fires. That run parks until the TTL reaper fails it. Key
                // off port arity, not the rule name, so any decision-style node
                // is covered.
                let sources: std::collections::HashSet<&str> =
                    inbound.iter().map(|e| e.from.node.as_str()).collect();
                if sources.len() == 1 {
                    let up = sources.into_iter().next().unwrap_or("");
                    let all_decision_ports = by_id
                        .get(up)
                        .and_then(|un| registry.get(&un.rule))
                        .map(|entry| {
                            let outs = entry.outputs(&by_id.get(up).unwrap().config);
                            inbound.iter().all(|e| {
                                outs.iter()
                                    .any(|p| p.id == e.from.port && p.arity == PortArity::One)
                            })
                        })
                        .unwrap_or(false);
                    if all_decision_ports {
                        issues.push(Issue::warn(
                            Some(&n.id),
                            format!(
                                "Mọi cổng vào của join đều đến từ các nhánh loại trừ nhau của `{up}` — chỉ một nhánh chạy nên barrier không bao giờ đủ."
                            ),
                        ));
                    }
                }
            }
        }
        if n.opts.concurrency == 0 {
            issues.push(Issue::error(
                Some(&n.id),
                "concurrency phải >= 1.".to_string(),
            ));
        } else if n.opts.concurrency > crate::model::MAX_CONCURRENCY {
            issues.push(Issue::warn(
                Some(&n.id),
                format!(
                    "concurrency {} vượt trần — sẽ chỉ chạy {} worker.",
                    n.opts.concurrency,
                    crate::model::MAX_CONCURRENCY
                ),
            ));
        }
    }

    if source_count == 0 && !nodes.is_empty() {
        issues.push(Issue::warn(
            None,
            "Luồng không có node nguồn nào — sẽ không bao giờ tự chạy. Thêm `manual`, `webhook` hoặc `schedule`.",
        ));
    }

    // Edges: endpoints exist, ports exist, arity respected.
    let mut used_one_ports: HashMap<(String, String), usize> = HashMap::new();
    for e in edges {
        let Some(src) = by_id.get(e.from.node.as_str()) else {
            issues.push(Issue::edge_error(
                &e.id,
                format!("Cạnh trỏ từ node không tồn tại `{}`.", e.from.node),
            ));
            continue;
        };
        let Some(dst) = by_id.get(e.to.node.as_str()) else {
            issues.push(Issue::edge_error(
                &e.id,
                format!("Cạnh trỏ tới node không tồn tại `{}`.", e.to.node),
            ));
            continue;
        };
        let (Some(src_entry), Some(dst_entry)) = (registry.get(&src.rule), registry.get(&dst.rule))
        else {
            continue; // already reported as an unknown rule
        };

        let outs = src_entry.outputs(&src.config);
        match outs.iter().find(|p| p.id == e.from.port) {
            None => issues.push(Issue::edge_error(
                &e.id,
                format!(
                    "Node `{}` ({}) không có cổng ra `{}`.",
                    src.id, src.rule, e.from.port
                ),
            )),
            Some(p) if p.arity == PortArity::One => {
                let key = (src.id.clone(), e.from.port.clone());
                let n = used_one_ports.entry(key).or_insert(0);
                *n += 1;
                if *n > 1 {
                    issues.push(Issue::edge_error(
                        &e.id,
                        format!(
                            "Cổng `{}` của node `{}` chỉ nhận 1 cạnh.",
                            e.from.port, src.id
                        ),
                    ));
                }
            }
            _ => {}
        }

        let ins = dst_entry.inputs(&dst.config);
        if ins.is_empty() {
            issues.push(Issue::edge_error(
                &e.id,
                format!("Node nguồn `{}` không nhận cổng vào.", dst.id),
            ));
        } else if !ins.iter().any(|p| p.id == e.to.port) {
            issues.push(Issue::edge_error(
                &e.id,
                format!(
                    "Node `{}` ({}) không có cổng vào `{}`.",
                    dst.id, dst.rule, e.to.port
                ),
            ));
        }
    }

    // Reachability: a non-source node with no inbound edge can never fire.
    for n in nodes {
        let is_source = registry.is_source(&n.rule);
        let has_in = edges.iter().any(|e| e.to.node == n.id);
        let has_out = edges.iter().any(|e| e.from.node == n.id);
        if !is_source && !has_in {
            issues.push(Issue::warn(
                Some(&n.id),
                "Node không có cổng vào nào được nối — sẽ không bao giờ chạy.",
            ));
        }
        if is_source && !has_out {
            issues.push(Issue::warn(
                Some(&n.id),
                "Node nguồn chưa nối đi đâu — sự kiện sẽ rơi vào hư vô.",
            ));
        }
    }

    if let Some(cycle) = find_cycle(nodes, edges) {
        // Not an error: a loop with a delay or an exit condition is a valid
        // design, and the per-run hop budget stops runaways.
        issues.push(Issue::warn(
            None,
            format!(
                "Có vòng lặp trong đồ thị: {}. Đảm bảo có điều kiện thoát.",
                cycle.join(" → ")
            ),
        ));
    }

    issues
}

fn find_cycle(nodes: &[Node], edges: &[Edge]) -> Option<Vec<NodeId>> {
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in edges {
        adj.entry(e.from.node.as_str())
            .or_default()
            .push(e.to.node.as_str());
    }
    let mut state: HashMap<&str, u8> = HashMap::new(); // 0 new, 1 open, 2 done
    let mut stack: Vec<&str> = Vec::new();

    fn dfs<'a>(
        n: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        state: &mut HashMap<&'a str, u8>,
        stack: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        state.insert(n, 1);
        stack.push(n);
        for next in adj.get(n).map(|v| v.as_slice()).unwrap_or(&[]) {
            match state.get(next).copied().unwrap_or(0) {
                0 => {
                    if let Some(c) = dfs(next, adj, state, stack) {
                        return Some(c);
                    }
                }
                1 => {
                    let start = stack.iter().position(|x| x == next).unwrap_or(0);
                    let mut cycle: Vec<String> =
                        stack[start..].iter().map(|s| s.to_string()).collect();
                    cycle.push(next.to_string());
                    return Some(cycle);
                }
                _ => {}
            }
        }
        stack.pop();
        state.insert(n, 2);
        None
    }

    for n in nodes {
        if state.get(n.id.as_str()).copied().unwrap_or(0) == 0 {
            if let Some(c) = dfs(n.id.as_str(), &adj, &mut state, &mut stack) {
                return Some(c);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx, SourceCtx, SourceRule};
    use async_trait::async_trait;
    use serde_json::json;

    struct Pass(RuleSpec);
    #[async_trait]
    impl Rule for Pass {
        fn spec(&self) -> &RuleSpec {
            &self.0
        }
        async fn handle(&self, _c: &RunCtx, _m: Message) -> Outcome {
            Outcome::Terminal
        }
    }

    struct Src(RuleSpec);
    #[async_trait]
    impl SourceRule for Src {
        fn spec(&self) -> &RuleSpec {
            &self.0
        }
        async fn start(&self, _ctx: SourceCtx) -> Result<(), String> {
            Ok(())
        }
        async fn stop(&self, _c: ChainId, _n: &str) {}
    }

    fn registry() -> Registry {
        let mut r = Registry::new();
        r.add_rule(Arc::new(Pass(
            RuleSpec::builder("pass", "Pass", Category::Transform).build(),
        )));
        r.add_rule(Arc::new(Pass(
            RuleSpec::builder("cond", "Cond", Category::Logic)
                .outputs(vec![
                    PortSpec::new("true", "true").one(),
                    PortSpec::new("false", "false").one(),
                ])
                .build(),
        )));
        r.add_source(Arc::new(Src(RuleSpec::builder(
            "manual",
            "Manual",
            Category::Source,
        )
        .build())));
        r.add_rule(Arc::new(Pass(
            RuleSpec::builder("join", "Join", Category::Logic)
                .inputs(vec![PortSpec::new("a", "a"), PortSpec::new("b", "b")])
                .build(),
        )));
        r
    }

    fn node(id: &str, rule: &str) -> Node {
        Node {
            id: id.into(),
            chain_id: 1,
            rule: rule.into(),
            name: id.into(),
            config: json!({}),
            opts: NodeOpts::default(),
            x: 0.0,
            y: 0.0,
            debug: false,
        }
    }

    fn edge(id: &str, fnode: &str, fport: &str, tnode: &str, tport: &str) -> Edge {
        Edge {
            id: id.into(),
            from: PortRef::new(fnode, fport),
            to: PortRef::new(tnode, tport),
        }
    }

    #[test]
    fn a_join_node_left_on_the_default_policy_is_an_error() {
        let reg = registry();
        let mut j = node("j", "join");
        // opts.join defaults to Any — the inert case.
        let nodes = vec![
            node("s", "manual"),
            node("a", "pass"),
            node("b", "pass"),
            j.clone(),
        ];
        let edges = vec![
            edge("e0", "s", "out", "a", "in"),
            edge("e1", "s", "out", "b", "in"),
            edge("e2", "a", "out", "j", "a"),
            edge("e3", "b", "out", "j", "b"),
        ];
        let issues = validate(&nodes, &edges, &reg);
        assert!(
            issues
                .iter()
                .any(|i| i.level == IssueLevel::Error && i.message.contains("opts.join")),
            "join để `any` phải là lỗi: {issues:?}"
        );

        // With the matching policy the error is gone.
        j.opts.join = JoinPolicy::All;
        let nodes = vec![node("s", "manual"), node("a", "pass"), node("b", "pass"), j];
        let ok = validate(&nodes, &edges, &reg);
        assert!(!ok.iter().any(|i| i.message.contains("opts.join")));
    }

    #[test]
    fn a_join_fed_only_by_mutually_exclusive_branches_is_warned() {
        let reg = registry();
        let mut j = node("j", "join");
        j.opts.join = JoinPolicy::All;
        let nodes = vec![node("s", "manual"), node("c", "cond"), j];
        let edges = vec![
            edge("e0", "s", "out", "c", "in"),
            edge("e1", "c", "true", "j", "a"),
            edge("e2", "c", "false", "j", "b"),
        ];
        let issues = validate(&nodes, &edges, &reg);
        assert!(
            issues.iter().any(|i| i.message.contains("loại trừ nhau")),
            "phải cảnh báo barrier không bao giờ đủ: {issues:?}"
        );
    }

    #[test]
    fn compile_builds_the_out_table_and_finds_sources() {
        let reg = registry();
        let nodes = vec![node("s", "manual"), node("p", "pass")];
        let edges = vec![edge("e", "s", "out", "p", "in")];
        let g = compile(1, false, &nodes, &edges, &reg);
        assert_eq!(g.sources, vec!["s".to_string()]);
        assert_eq!(g.node("s").unwrap().targets("out").len(), 1);
        assert_eq!(g.node("s").unwrap().targets("error").len(), 0);
        assert_eq!(
            g.node("p").unwrap().connected_inputs,
            vec!["in".to_string()]
        );
    }

    #[test]
    fn fan_out_keeps_every_target_on_one_port() {
        let reg = registry();
        let nodes = vec![node("s", "manual"), node("a", "pass"), node("b", "pass")];
        let edges = vec![
            edge("e1", "s", "out", "a", "in"),
            edge("e2", "s", "out", "b", "in"),
        ];
        let g = compile(1, false, &nodes, &edges, &reg);
        assert_eq!(g.node("s").unwrap().targets("out").len(), 2);
    }

    #[test]
    fn unknown_port_is_an_error() {
        let reg = registry();
        let nodes = vec![node("s", "manual"), node("p", "pass")];
        let edges = vec![edge("e", "s", "nope", "p", "in")];
        let issues = validate(&nodes, &edges, &reg);
        assert!(has_errors(&issues));
        assert!(issues.iter().any(|i| i.message.contains("cổng ra `nope`")));
    }

    #[test]
    fn arity_one_rejects_a_second_edge() {
        let reg = registry();
        let nodes = vec![
            node("s", "manual"),
            node("c", "cond"),
            node("a", "pass"),
            node("b", "pass"),
        ];
        let edges = vec![
            edge("e0", "s", "out", "c", "in"),
            edge("e1", "c", "true", "a", "in"),
            edge("e2", "c", "true", "b", "in"),
        ];
        let issues = validate(&nodes, &edges, &reg);
        assert!(issues
            .iter()
            .any(|i| i.level == IssueLevel::Error && i.message.contains("chỉ nhận 1 cạnh")));
    }

    #[test]
    fn unknown_rule_is_an_error_and_does_not_panic() {
        let reg = registry();
        let nodes = vec![node("x", "does-not-exist")];
        let issues = validate(&nodes, &[], &reg);
        assert!(has_errors(&issues));
    }

    #[test]
    fn cycle_is_reported_as_a_warning_only() {
        let reg = registry();
        let nodes = vec![node("s", "manual"), node("a", "pass"), node("b", "pass")];
        let edges = vec![
            edge("e0", "s", "out", "a", "in"),
            edge("e1", "a", "out", "b", "in"),
            edge("e2", "b", "out", "a", "in"),
        ];
        let issues = validate(&nodes, &edges, &reg);
        assert!(!has_errors(&issues));
        assert!(issues.iter().any(|i| i.message.contains("vòng lặp")));
    }

    #[test]
    fn orphan_node_is_warned_about() {
        let reg = registry();
        let nodes = vec![node("s", "manual"), node("lonely", "pass")];
        let issues = validate(&nodes, &[], &reg);
        assert!(issues
            .iter()
            .any(|i| i.node.as_deref() == Some("lonely") && i.level == IssueLevel::Warning));
    }
}
