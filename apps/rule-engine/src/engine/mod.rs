//! The scheduler.
//!
//! Shape: one bounded mailbox per **node** (the Go engine had one queue per
//! *rule type*, so two nodes of the same kind fought over one pool), N workers
//! per node from `opts.concurrency`, and a router that turns
//! `(node, out_port)` into targets through the edge table.

pub mod graph;
pub mod join;
pub mod registry;
pub mod run;
pub mod services;
pub mod spec;
pub mod types;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio::task::JoinHandle;

use graph::{CompiledGraph, CompiledNode, Issue};
use join::{combine, Gate, JoinTable};
use registry::Registry;
use run::{RunState, RunTable};
use services::{EngineEvent, Services};
use spec::{Emitter, Ingress, RunCtx, SourceCtx};
use types::*;

use crate::model::{Chain, HopRow, Node, RunStatus};

/// Bound on the *entry* channel only (sources → ingress). This is where
/// backpressure belongs: it throttles how fast new runs are admitted.
const INGRESS_CAPACITY: usize = 1024;

/// The immutable half of a deployment; workers hold a clone and never take a
/// lock to route a message.
///
/// Per-node mailboxes are **unbounded**. A bounded mailbox deadlocks a cyclic
/// graph: a worker routing into a full downstream blocks, and if that
/// downstream (directly or transitively) routes back, neither drains. Unbounded
/// sends never block, so a runaway cycle is instead bounded by the per-run hop
/// budget, which is checked on dequeue in `execute`. Within one run the queue
/// depth is therefore capped at ~`max_hops` messages; entry backpressure keeps
/// the number of concurrent runs in check.
struct Live {
    graph: Arc<CompiledGraph>,
    node_tx: HashMap<NodeId, mpsc::UnboundedSender<Message>>,
}

struct Deployment {
    tasks: Vec<JoinHandle<()>>,
    sources: Vec<(NodeId, String)>, // (node id, rule id)
}

/// Result of a synchronous run (see [`Engine::start_run_wait`]).
pub struct RunOutcome {
    pub run_id: RunId,
    pub status: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}

pub struct Engine {
    pub registry: Arc<Registry>,
    pub svc: Arc<Services>,
    pub runs: Arc<RunTable>,
    joins: Arc<JoinTable>,
    live: RwLock<HashMap<ChainId, Arc<Live>>>,
    deployments: Mutex<HashMap<ChainId, Deployment>>,
    ingress_tx: mpsc::Sender<Ingress>,
    join_timeout_ms: u64,
    run_ttl_secs: i64,
}

impl Engine {
    pub fn start(registry: Arc<Registry>, svc: Arc<Services>) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<Ingress>(INGRESS_CAPACITY);
        let runs = Arc::new(RunTable::new(
            svc.db.clone(),
            svc.bus.clone(),
            crate::config::max_hops_per_run(),
        ));
        let engine = Arc::new(Engine {
            registry,
            svc,
            runs,
            joins: Arc::new(JoinTable::new()),
            live: RwLock::new(HashMap::new()),
            deployments: Mutex::new(HashMap::new()),
            ingress_tx: tx,
            join_timeout_ms: crate::config::default_join_timeout_ms(),
            run_ttl_secs: crate::config::run_ttl_secs(),
        });

        let e = engine.clone();
        tokio::spawn(async move { e.ingress_loop(rx).await });
        let e = engine.clone();
        tokio::spawn(async move { e.reaper_loop().await });
        engine
    }

    fn live_of(&self, chain_id: ChainId) -> Option<Arc<Live>> {
        self.live
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(&chain_id)
            .cloned()
    }

    pub fn is_deployed(&self, chain_id: ChainId) -> bool {
        self.live_of(chain_id).is_some()
    }

    pub fn deployed_chains(&self) -> Vec<ChainId> {
        self.live
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .copied()
            .collect()
    }

    // ------------------------------------------------------------- deploy

    /// Compile, spawn workers, start sources. Replaces any running deployment
    /// of the same chain — unlike the Go loader, which returned early if the
    /// chain was already running and so never picked up edits.
    pub async fn deploy(
        self: &Arc<Self>,
        chain: &Chain,
        nodes: &[Node],
        edges: &[Edge],
    ) -> Result<Vec<Issue>, String> {
        let issues = graph::validate(nodes, edges, &self.registry);
        if graph::has_errors(&issues) {
            return Err(issues
                .iter()
                .filter(|i| i.level == graph::IssueLevel::Error)
                .map(|i| i.message.clone())
                .collect::<Vec<_>>()
                .join("; "));
        }

        self.undeploy(chain.id).await;

        let compiled = Arc::new(graph::compile(
            chain.id,
            chain.debug,
            nodes,
            edges,
            &self.registry,
        ));

        let mut node_tx = HashMap::new();
        let mut receivers = Vec::new();
        for (id, n) in compiled.nodes.iter() {
            if self.registry.is_source(&n.rule) {
                continue; // sources push into ingress, they have no mailbox
            }
            let (tx, rx) = mpsc::unbounded_channel::<Message>();
            node_tx.insert(id.clone(), tx);
            receivers.push((n.clone(), rx));
        }

        let live = Arc::new(Live {
            graph: compiled.clone(),
            node_tx,
        });
        self.live
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(chain.id, live.clone());

        let mut tasks = Vec::new();
        for (node, rx) in receivers {
            let shared = Arc::new(AsyncMutex::new(rx));
            let workers = node.opts.workers();
            for _ in 0..workers {
                let engine = self.clone();
                let live = live.clone();
                let node = node.clone();
                let rx = shared.clone();
                tasks.push(tokio::spawn(async move {
                    Engine::node_worker(engine, live, node, rx).await;
                }));
            }
        }

        // Sources last: nothing should fire before the mailboxes exist.
        let mut sources = Vec::new();
        for id in &compiled.sources {
            let Some(node) = compiled.node(id) else {
                continue;
            };
            let Some(src) = self.registry.source(&node.rule) else {
                continue;
            };
            let ctx = SourceCtx {
                chain_id: chain.id,
                node: node.id.clone(),
                config: node.config.clone(),
                svc: self.svc.clone(),
                emitter: Emitter {
                    tx: self.ingress_tx.clone(),
                    chain_id: chain.id,
                    node: node.id.clone(),
                },
            };
            if let Err(e) = src.start(ctx).await {
                self.svc.log.write(
                    chain.id,
                    None,
                    "error",
                    Some(&node.id),
                    format!("không khởi động được node nguồn `{}`: {e}", node.rule),
                );
                return Err(format!("node nguồn `{}` lỗi: {e}", node.id));
            }
            sources.push((node.id.clone(), node.rule.clone()));
        }

        self.deployments
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(chain.id, Deployment { tasks, sources });

        self.svc.log.write(
            chain.id,
            None,
            "info",
            None,
            format!(
                "đã nạp luồng `{}` — {} node, {} cạnh, {} nguồn",
                chain.name,
                nodes.len(),
                edges.len(),
                compiled.sources.len()
            ),
        );
        self.svc.bus.publish(EngineEvent::ChainStatus {
            chain_id: chain.id,
            status: "ACTIVE".into(),
            error: None,
        });
        Ok(issues)
    }

    pub async fn undeploy(&self, chain_id: ChainId) {
        let dep = self
            .deployments
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&chain_id);
        self.live
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&chain_id);

        if let Some(dep) = dep {
            for (node, rule) in &dep.sources {
                if let Some(src) = self.registry.source(rule) {
                    src.stop(chain_id, node).await;
                }
            }
            // Abort rather than drain: a rule stuck in a slow I/O call must not
            // be able to hang undeploy. A run aborted mid-`handle` may have
            // already performed its side effect; `runs.drop_chain` marks every
            // such run `failed` with a clear reason, which is the run-level
            // trace of the interruption.
            let active = self.runs.active_for(chain_id);
            for t in dep.tasks {
                t.abort();
            }
            if active > 0 {
                self.svc.log.write(
                    chain_id,
                    None,
                    "warn",
                    None,
                    format!(
                        "dừng luồng khi còn {active} run đang chạy — các run này bị đánh dấu lỗi"
                    ),
                );
            }
        }
        self.joins.drop_chain(chain_id);
        self.runs.drop_chain(chain_id);
        self.svc.bus.publish(EngineEvent::ChainStatus {
            chain_id,
            status: "INACTIVE".into(),
            error: None,
        });
    }

    // ------------------------------------------------------------ ingress

    async fn ingress_loop(self: Arc<Self>, mut rx: mpsc::Receiver<Ingress>) {
        while let Some(ing) = rx.recv().await {
            let engine = self.clone();
            tokio::spawn(async move {
                engine
                    .start_run(ing.chain_id, &ing.node, &ing.port, ing.data, ing.meta)
                    .await;
            });
        }
    }

    /// Inject an event as if a source produced it. Returns the run id.
    pub async fn start_run(
        &self,
        chain_id: ChainId,
        node_id: &str,
        port: &str,
        data: Value,
        meta: Value,
    ) -> Option<RunId> {
        let live = self.live_of(chain_id)?;
        let node = live.graph.node(node_id)?.clone();
        let debug = live.graph.debug || node.debug;
        let run = self.runs.start(chain_id, node_id, debug);

        // Hold one virtual message so an unconnected source still closes the
        // run instead of leaving it `running` forever.
        self.runs.retain(&run, 1);

        let carrier = Message::seed(
            run.id,
            chain_id,
            PortRef::new(node_id, PORT_OUT),
            data.clone(),
            meta,
        );
        if debug {
            self.record_hop(&run, &node, "", port, &carrier, None, 0);
        }
        self.route(
            &live,
            &run,
            &node,
            &carrier,
            vec![Emission::new(port, data)],
        )
        .await;
        self.runs.release(&run, 1);
        Some(run.id)
    }

    /// Synchronous request/response: inject an event, wait for that run to
    /// finish, and return its status plus whatever a `respond` node recorded.
    ///
    /// We subscribe to the event bus *before* starting the run, so the terminal
    /// `RunEnd` can't slip past between start and subscribe. On timeout the run
    /// keeps going in the background but we stop waiting and drop any result it
    /// may later produce, so the result map can't leak.
    pub async fn start_run_wait(
        &self,
        chain_id: ChainId,
        node_id: &str,
        port: &str,
        data: Value,
        meta: Value,
        timeout_ms: u64,
    ) -> RunOutcome {
        let mut rx = self.svc.bus.subscribe();
        let Some(run_id) = self.start_run(chain_id, node_id, port, data, meta).await else {
            return RunOutcome {
                run_id: 0,
                status: "error".into(),
                result: None,
                error: Some(format!(
                    "không tìm thấy luồng đang chạy hoặc node `{node_id}`"
                )),
            };
        };

        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                self.svc.results.discard(run_id);
                return RunOutcome {
                    run_id,
                    status: "timeout".into(),
                    result: None,
                    error: Some(format!("quá {timeout_ms}ms mà run chưa xong")),
                };
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(line)) => {
                    // Only parse the cheap shape we care about.
                    let Ok(ev) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if ev.get("type").and_then(|t| t.as_str()) != Some("runEnd") {
                        continue;
                    }
                    if ev.get("runId").and_then(|r| r.as_u64()) != Some(run_id) {
                        continue;
                    }
                    let status = ev
                        .get("status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("done")
                        .to_string();
                    let error = ev
                        .get("error")
                        .and_then(|e| e.as_str())
                        .map(|s| s.to_string());
                    return RunOutcome {
                        run_id,
                        status,
                        result: self.svc.results.take(run_id),
                        error,
                    };
                }
                // Lagged past some events — the RunEnd may have been one of them,
                // so fall back to a short grace check rather than hang forever.
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(_)) | Err(_) => {
                    // Bus closed or the recv timed out: give the result slot one
                    // last look in case `respond` fired between events.
                    let result = self.svc.results.take(run_id);
                    if result.is_some() {
                        return RunOutcome {
                            run_id,
                            status: "done".into(),
                            result,
                            error: None,
                        };
                    }
                    self.svc.results.discard(run_id);
                    return RunOutcome {
                        run_id,
                        status: "timeout".into(),
                        result: None,
                        error: Some(format!("quá {timeout_ms}ms mà run chưa xong")),
                    };
                }
            }
        }
    }

    // ------------------------------------------------------------- worker

    async fn node_worker(
        engine: Arc<Engine>,
        live: Arc<Live>,
        node: Arc<CompiledNode>,
        rx: Arc<AsyncMutex<mpsc::UnboundedReceiver<Message>>>,
    ) {
        loop {
            let msg = {
                let mut guard = rx.lock().await;
                match guard.recv().await {
                    Some(m) => m,
                    None => return,
                }
            };
            engine.execute(&live, &node, msg).await;
        }
    }

    async fn execute(&self, live: &Arc<Live>, node: &Arc<CompiledNode>, msg: Message) {
        let Some(run) = self.runs.get(msg.run_id) else {
            return; // run already finished or was reaped
        };

        let hops = run.bump_hops();
        if hops > self.runs.max_hops() {
            run.set_error(format!(
                "vượt trần {} bước — nghi ngờ vòng lặp vô tận",
                self.runs.max_hops()
            ));
            self.runs.finish(&run, RunStatus::Failed);
            return;
        }

        let in_port = msg.target.port.clone();
        let parts = match self.joins.gate(node, msg, self.join_timeout_ms) {
            Gate::Park => return, // still counted in flight, on purpose
            Gate::Fire(parts) => parts,
        };
        // The barrier consumed N messages and produces one execution.
        if parts.len() > 1 {
            self.runs.release(&run, parts.len() as i64 - 1);
        }
        let message = combine(node.opts.join, parts);

        let Some(rule) = self.registry.rule(&node.rule) else {
            let err = EngineError::config(format!("không có loại node `{}`", node.rule))
                .at(&node.id, &node.rule);
            self.handle_failure(live, &run, node, &message, err, &in_port)
                .await;
            self.runs.release(&run, 1);
            return;
        };

        let ctx = RunCtx {
            chain_id: node_chain(live),
            run_id: run.id,
            node: node.id.clone(),
            rule: node.rule.clone(),
            config: node.config.clone(),
            svc: self.svc.clone(),
        };

        let started = Instant::now();
        let mut attempt = 0u32;
        let outcome = loop {
            let out = rule.handle(&ctx, message.clone()).await;
            match out {
                Outcome::Fail(ref e) if attempt < node.opts.retries => {
                    attempt += 1;
                    self.svc.log.write(
                        ctx.chain_id,
                        Some(run.id),
                        "warn",
                        Some(&node.id),
                        format!("thử lại lần {attempt} sau lỗi: {e}"),
                    );
                    tokio::time::sleep(Duration::from_millis(
                        node.opts.retry_backoff_ms * attempt as u64,
                    ))
                    .await;
                }
                other => break other,
            }
        };
        let dur_ms = started.elapsed().as_millis() as i64;
        let debug = run.debug || node.debug;

        match outcome {
            Outcome::Emit(emissions) => {
                if debug {
                    if emissions.is_empty() {
                        self.record_hop(&run, node, &in_port, "", &message, None, dur_ms);
                    }
                    for e in &emissions {
                        let mut shown = message.clone();
                        shown.data = e.data.clone();
                        self.record_hop(&run, node, &in_port, &e.port, &shown, None, dur_ms);
                    }
                }
                self.route(live, &run, node, &message, emissions).await;
            }
            Outcome::Terminal => {
                if debug {
                    self.record_hop(&run, node, &in_port, "", &message, None, dur_ms);
                }
            }
            Outcome::Fail(err) => {
                self.handle_failure(live, &run, node, &message, err, &in_port)
                    .await;
            }
        }
        self.runs.release(&run, 1);
    }

    /// Route a failure to the implicit `error` port.
    ///
    /// The Go rules left `Next` unset on a bad option, so the branch died with
    /// no record at all. Here an unwired `error` port still logs and marks the
    /// run failed.
    async fn handle_failure(
        &self,
        live: &Arc<Live>,
        run: &Arc<RunState>,
        node: &Arc<CompiledNode>,
        message: &Message,
        err: EngineError,
        in_port: &str,
    ) {
        let targets = node.targets(PORT_ERROR);
        if run.debug || node.debug {
            self.record_hop(
                run,
                node,
                in_port,
                if targets.is_empty() { "" } else { PORT_ERROR },
                message,
                Some(&err),
                0,
            );
        }
        if targets.is_empty() {
            run.set_error(err.to_string());
            self.svc.log.write(
                message.chain_id,
                Some(run.id),
                "error",
                Some(&node.id),
                format!("{err} (cổng `error` chưa nối — nhánh dừng tại đây)"),
            );
            return;
        }
        let payload = json!({
            "error": { "code": err.code, "message": err.message, "node": node.id, "rule": node.rule },
            "data": message.data,
        });
        let mut emission = Emission::new(PORT_ERROR, payload);
        emission.meta = Some(message.meta.clone());
        self.route_error(live, run, node, message, emission, err)
            .await;
    }

    async fn route_error(
        &self,
        live: &Arc<Live>,
        run: &Arc<RunState>,
        node: &Arc<CompiledNode>,
        base: &Message,
        emission: Emission,
        err: EngineError,
    ) {
        for target in node.targets(PORT_ERROR) {
            let mut msg = base.clone();
            msg.seq = run.next_seq();
            msg.from = Some(PortRef::new(&node.id, PORT_ERROR));
            msg.target = target.clone();
            msg.data = emission.data.clone();
            if let Some(m) = &emission.meta {
                msg.meta = m.clone();
            }
            msg.kind = MsgKind::Error;
            msg.error = Some(err.clone());
            msg.ts = now_ms();
            self.send(live, run, msg).await;
        }
    }

    async fn route(
        &self,
        live: &Arc<Live>,
        run: &Arc<RunState>,
        node: &Arc<CompiledNode>,
        base: &Message,
        emissions: Vec<Emission>,
    ) {
        for e in emissions {
            let targets = node.targets(&e.port);
            if targets.is_empty() {
                continue; // a branch that ends here — normal for sinks
            }
            for target in targets {
                let mut msg = base.clone();
                msg.seq = run.next_seq();
                msg.from = Some(PortRef::new(&node.id, &e.port));
                msg.target = target.clone();
                // Value::clone is a deep copy, so fan-out branches can never
                // alias each other the way the Go shallow Clone() allowed.
                msg.data = e.data.clone();
                if let Some(m) = &e.meta {
                    msg.meta = m.clone();
                }
                msg.kind = MsgKind::Data;
                msg.error = None;
                msg.ts = now_ms();
                self.send(live, run, msg).await;
            }
        }
    }

    async fn send(&self, live: &Arc<Live>, run: &Arc<RunState>, msg: Message) {
        let Some(tx) = live.node_tx.get(&msg.target.node) else {
            self.svc.log.write(
                msg.chain_id,
                Some(run.id),
                "error",
                Some(&msg.target.node),
                format!("không tìm thấy node đích `{}`", msg.target.node),
            );
            return;
        };
        self.runs.retain(run, 1);
        // Unbounded send never blocks, so a cyclic graph can't deadlock here.
        if tx.send(msg).is_err() {
            // Receiver gone: the chain was undeployed mid-flight.
            self.runs.release(run, 1);
        }
    }

    fn record_hop(
        &self,
        run: &Arc<RunState>,
        node: &Arc<CompiledNode>,
        in_port: &str,
        out_port: &str,
        msg: &Message,
        err: Option<&EngineError>,
        dur_ms: i64,
    ) {
        let seq = run.next_seq();
        let kind = if err.is_some() || msg.is_error() {
            "error"
        } else {
            "data"
        };
        let data_str = truncate(&msg.data.to_string(), 8000);
        let err_str = err
            .map(|e| e.to_string())
            .or_else(|| msg.error.as_ref().map(|e| e.to_string()))
            .unwrap_or_default();

        let _ = self.svc.db.insert_hop(&HopRow {
            id: 0,
            run_id: run.id as i64,
            chain_id: msg.chain_id,
            seq: seq as i64,
            node: node.id.clone(),
            rule: node.rule.clone(),
            in_port: in_port.to_string(),
            out_port: out_port.to_string(),
            kind: kind.to_string(),
            data: data_str,
            error: err_str.clone(),
            ts: now_ms(),
            dur_ms,
        });
        self.svc.bus.publish(EngineEvent::Hop {
            run_id: run.id,
            chain_id: msg.chain_id,
            seq,
            node: node.id.clone(),
            rule: node.rule.clone(),
            in_port: in_port.to_string(),
            out_port: out_port.to_string(),
            kind: kind.to_string(),
            data: msg.data.clone(),
            error: if err_str.is_empty() {
                None
            } else {
                Some(err_str)
            },
            dur_ms,
        });
    }

    // ------------------------------------------------------------- reaper

    async fn reaper_loop(self: Arc<Self>) {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;

            for exp in self.joins.take_expired() {
                let Some(run) = self.runs.get(exp.run_id) else {
                    continue;
                };
                let n = exp.parts.len() as i64;
                if let Some(first) = exp.parts.first() {
                    self.svc.log.write(
                        first.chain_id,
                        Some(exp.run_id),
                        "error",
                        Some(&exp.node),
                        format!(
                            "join quá hạn: mới nhận {} cổng ({}) — huỷ nhánh",
                            n,
                            exp.parts
                                .iter()
                                .map(|m| m.target.port.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                }
                run.set_error("join quá hạn");
                self.runs.release(&run, n);
            }

            for run in self.runs.expired(self.run_ttl_secs) {
                let orphans = self.joins.drop_run(run.id);
                let _ = orphans;
                run.set_error(format!("run quá hạn {}s", self.run_ttl_secs));
                self.runs.finish(&run, RunStatus::Timeout);
            }
        }
    }
}

fn node_chain(live: &Arc<Live>) -> ChainId {
    live.graph.chain_id
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::engine::registry::Registry;
    use crate::engine::services::EventBus;
    use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, SourceRule};
    use crate::model::{ChainStatus, JoinPolicy, NodeOpts};
    use async_trait::async_trait;
    use std::sync::Mutex as StdMutex;

    /// Records everything that reaches it so a test can assert on both the
    /// values and how many times a node ran.
    #[derive(Default)]
    struct Recorder(StdMutex<Vec<(String, Value)>>);

    impl Recorder {
        fn push(&self, node: &str, v: Value) {
            self.0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push((node.to_string(), v));
        }
        fn seen(&self, node: &str) -> Vec<Value> {
            self.0
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .filter(|(n, _)| n == node)
                .map(|(_, v)| v.clone())
                .collect()
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

    /// Adds `{ "seen": <node id> }` and passes the payload along.
    struct Pass(RuleSpec, Arc<Recorder>);
    #[async_trait]
    impl Rule for Pass {
        fn spec(&self) -> &RuleSpec {
            &self.0
        }
        async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
            self.1.push(&ctx.node, msg.data.clone());
            let mut d = msg.data;
            crate::daq::set(&mut d, "seen", json!(ctx.node));
            Outcome::out(d)
        }
    }

    /// Emits on two named ports at once — the multi-output case.
    struct Tee(RuleSpec);
    #[async_trait]
    impl Rule for Tee {
        fn spec(&self) -> &RuleSpec {
            &self.0
        }
        async fn handle(&self, _ctx: &RunCtx, msg: Message) -> Outcome {
            Outcome::Emit(vec![
                Emission::new("a", msg.data.clone()),
                Emission::new("b", msg.data),
            ])
        }
    }

    struct Boom(RuleSpec);
    #[async_trait]
    impl Rule for Boom {
        fn spec(&self) -> &RuleSpec {
            &self.0
        }
        async fn handle(&self, ctx: &RunCtx, _msg: Message) -> Outcome {
            ctx.fail_runtime("nổ có chủ đích")
        }
    }

    struct Sink(RuleSpec, Arc<Recorder>);
    #[async_trait]
    impl Rule for Sink {
        fn spec(&self) -> &RuleSpec {
            &self.0
        }
        async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
            self.1.push(&ctx.node, msg.data);
            Outcome::Terminal
        }
    }

    /// Loops forever: proves the hop budget is what stops a cycle.
    struct Loop(RuleSpec);
    #[async_trait]
    impl Rule for Loop {
        fn spec(&self) -> &RuleSpec {
            &self.0
        }
        async fn handle(&self, _ctx: &RunCtx, msg: Message) -> Outcome {
            Outcome::out(msg.data)
        }
    }

    /// Emits many messages on `out` from one input — the fan-out that, in a
    /// cycle with bounded mailboxes, used to wedge the whole chain.
    struct Burst(RuleSpec);
    #[async_trait]
    impl Rule for Burst {
        fn spec(&self) -> &RuleSpec {
            &self.0
        }
        async fn handle(&self, _ctx: &RunCtx, msg: Message) -> Outcome {
            Outcome::Emit(
                (0..64)
                    .map(|_| Emission::new("out", msg.data.clone()))
                    .collect(),
            )
        }
    }

    fn harness() -> (Arc<Engine>, Arc<Recorder>) {
        let db = Arc::new(Db::open(":memory:").unwrap());
        let _ = db.create_chain(1, "test", "");
        let svc = Arc::new(Services::new(db, EventBus::new()));
        let rec = Arc::new(Recorder::default());

        let mut reg = Registry::new();
        reg.add_source(Arc::new(Src(RuleSpec::builder(
            "src",
            "Src",
            Category::Source,
        )
        .build())));
        reg.add_rule(Arc::new(Pass(
            RuleSpec::builder("pass", "Pass", Category::Transform).build(),
            rec.clone(),
        )));
        reg.add_rule(Arc::new(Tee(RuleSpec::builder(
            "tee",
            "Tee",
            Category::Logic,
        )
        .outputs(vec![PortSpec::new("a", "a"), PortSpec::new("b", "b")])
        .build())));
        reg.add_rule(Arc::new(Boom(
            RuleSpec::builder("boom", "Boom", Category::Transform).build(),
        )));
        reg.add_rule(Arc::new(Sink(
            RuleSpec::builder("sink", "Sink", Category::Sink).build(),
            rec.clone(),
        )));
        reg.add_rule(Arc::new(Loop(
            RuleSpec::builder("loop", "Loop", Category::Transform).build(),
        )));
        reg.add_rule(Arc::new(Burst(
            RuleSpec::builder("burst", "Burst", Category::Transform).build(),
        )));
        reg.add_rule(Arc::new(Pass(
            RuleSpec::builder("gate", "Gate", Category::Logic)
                .inputs(vec![PortSpec::new("a", "a"), PortSpec::new("b", "b")])
                .build(),
            rec.clone(),
        )));
        (Engine::start(Arc::new(reg), svc), rec)
    }

    fn chain(debug: bool) -> Chain {
        Chain {
            id: 1,
            name: "test".into(),
            description: String::new(),
            status: ChainStatus::Active,
            debug,
            version: 1,
            created_at: String::new(),
            updated_at: String::new(),
        }
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

    /// Wait for the run to leave the table, or give up.
    async fn settle(engine: &Arc<Engine>) {
        for _ in 0..200 {
            if engine.runs.active() == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("run không kết thúc trong 1s");
    }

    #[tokio::test]
    async fn a_linear_chain_runs_end_to_end_and_the_run_closes() {
        let (engine, rec) = harness();
        let nodes = vec![node("s", "src"), node("p", "pass"), node("k", "sink")];
        let edges = vec![
            edge("e1", "s", "out", "p", "in"),
            edge("e2", "p", "out", "k", "in"),
        ];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();

        let run = engine
            .start_run(1, "s", "out", json!({ "v": 1 }), json!({}))
            .await
            .unwrap();
        settle(&engine).await;

        assert_eq!(rec.seen("p").len(), 1);
        let at_sink = rec.seen("k");
        assert_eq!(at_sink.len(), 1);
        assert_eq!(at_sink[0]["seen"], "p");

        let runs = engine.svc.db.list_runs(Some(1), 10).unwrap();
        let row = runs.iter().find(|r| r.id as u64 == run).unwrap();
        assert_eq!(row.status, "done");
    }

    #[tokio::test]
    async fn fan_out_gives_each_branch_an_independent_copy() {
        let (engine, rec) = harness();
        let nodes = vec![
            node("s", "src"),
            node("t", "tee"),
            node("k1", "sink"),
            node("k2", "sink"),
        ];
        let edges = vec![
            edge("e1", "s", "out", "t", "in"),
            edge("e2", "t", "a", "k1", "in"),
            edge("e3", "t", "b", "k2", "in"),
        ];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();
        engine
            .start_run(1, "s", "out", json!({ "v": 1 }), json!({}))
            .await
            .unwrap();
        settle(&engine).await;

        assert_eq!(rec.seen("k1").len(), 1, "nhánh a");
        assert_eq!(rec.seen("k2").len(), 1, "nhánh b");
    }

    /// The behaviour the Go engine could not express: two edges into one node
    /// normally fire it twice.
    #[tokio::test]
    async fn two_edges_into_one_node_fire_it_twice_by_default() {
        let (engine, rec) = harness();
        let nodes = vec![node("s", "src"), node("t", "tee"), node("k", "sink")];
        let edges = vec![
            edge("e1", "s", "out", "t", "in"),
            edge("e2", "t", "a", "k", "in"),
            edge("e3", "t", "b", "k", "in"),
        ];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();
        engine
            .start_run(1, "s", "out", json!({ "v": 1 }), json!({}))
            .await
            .unwrap();
        settle(&engine).await;
        assert_eq!(rec.seen("k").len(), 2);
    }

    #[tokio::test]
    async fn join_all_waits_for_every_connected_input_then_fires_once() {
        let (engine, rec) = harness();
        let mut gate = node("g", "gate");
        gate.opts = NodeOpts {
            join: JoinPolicy::All,
            ..Default::default()
        };
        let nodes = vec![node("s", "src"), node("t", "tee"), gate, node("k", "sink")];
        let edges = vec![
            edge("e1", "s", "out", "t", "in"),
            edge("e2", "t", "a", "g", "a"),
            edge("e3", "t", "b", "g", "b"),
            edge("e4", "g", "out", "k", "in"),
        ];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();
        engine
            .start_run(1, "s", "out", json!({ "v": 7 }), json!({}))
            .await
            .unwrap();
        settle(&engine).await;

        let fired = rec.seen("g");
        assert_eq!(fired.len(), 1, "barrier phải gộp thành đúng một lần chạy");
        // combine() keys the parts by the port they arrived on.
        assert_eq!(fired[0]["a"]["v"], 7);
        assert_eq!(fired[0]["b"]["v"], 7);
        assert_eq!(rec.seen("k").len(), 1);
    }

    #[tokio::test]
    async fn a_failure_routes_to_a_wired_error_port_and_the_run_still_succeeds() {
        let (engine, rec) = harness();
        let nodes = vec![node("s", "src"), node("b", "boom"), node("k", "sink")];
        let edges = vec![
            edge("e1", "s", "out", "b", "in"),
            edge("e2", "b", "error", "k", "in"),
        ];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();
        let run = engine
            .start_run(1, "s", "out", json!({ "v": 1 }), json!({}))
            .await
            .unwrap();
        settle(&engine).await;

        let caught = rec.seen("k");
        assert_eq!(caught.len(), 1);
        assert_eq!(caught[0]["error"]["code"], "runtime_error");
        assert_eq!(caught[0]["error"]["node"], "b");

        let runs = engine.svc.db.list_runs(Some(1), 10).unwrap();
        let row = runs.iter().find(|r| r.id as u64 == run).unwrap();
        assert_eq!(
            row.status, "done",
            "lỗi đã được bắt thì run không phải failed"
        );
    }

    /// The Go rules dropped a bad option silently; an unwired error port must
    /// still leave a trace.
    #[tokio::test]
    async fn an_unwired_error_port_fails_the_run_and_logs() {
        let (engine, _rec) = harness();
        let nodes = vec![node("s", "src"), node("b", "boom")];
        let edges = vec![edge("e1", "s", "out", "b", "in")];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();
        let run = engine
            .start_run(1, "s", "out", json!({}), json!({}))
            .await
            .unwrap();
        settle(&engine).await;

        let runs = engine.svc.db.list_runs(Some(1), 10).unwrap();
        let row = runs.iter().find(|r| r.id as u64 == run).unwrap();
        assert_eq!(row.status, "failed");
        assert!(row
            .error
            .as_deref()
            .unwrap_or("")
            .contains("nổ có chủ đích"));

        let logs = engine.svc.db.list_logs(1, 50).unwrap();
        assert!(
            logs.iter()
                .any(|l| l.message.contains("cổng `error` chưa nối")),
            "phải ghi log khi lỗi không có nơi đi"
        );
    }

    #[tokio::test]
    async fn a_cycle_is_stopped_by_the_hop_budget_not_by_running_forever() {
        let (engine, _rec) = harness();
        let nodes = vec![node("s", "src"), node("l1", "loop"), node("l2", "loop")];
        let edges = vec![
            edge("e1", "s", "out", "l1", "in"),
            edge("e2", "l1", "out", "l2", "in"),
            edge("e3", "l2", "out", "l1", "in"),
        ];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();
        let run = engine
            .start_run(1, "s", "out", json!({}), json!({}))
            .await
            .unwrap();

        for _ in 0..400 {
            if engine.runs.get(run).is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let runs = engine.svc.db.list_runs(Some(1), 10).unwrap();
        let row = runs.iter().find(|r| r.id as u64 == run).unwrap();
        assert_eq!(row.status, "failed");
        assert!(row.error.as_deref().unwrap_or("").contains("vòng lặp"));
    }

    /// A cycle whose nodes fan out far more messages than any single mailbox
    /// could hold. With bounded mailboxes each worker blocked writing into the
    /// other's full queue and the whole chain wedged. Unbounded mailboxes let
    /// the sends through, and the per-run hop budget stops the runaway. The
    /// point of the test is that it TERMINATES at all — a hang fails via the
    /// test timeout.
    #[tokio::test]
    async fn a_high_fanout_cycle_terminates_instead_of_deadlocking() {
        let (engine, _rec) = harness();
        let nodes = vec![node("s", "src"), node("b1", "burst"), node("b2", "burst")];
        let edges = vec![
            edge("e1", "s", "out", "b1", "in"),
            edge("e2", "b1", "out", "b2", "in"),
            edge("e3", "b2", "out", "b1", "in"),
        ];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();
        let run = engine
            .start_run(1, "s", "out", json!({ "v": 1 }), json!({}))
            .await
            .unwrap();

        let mut ended = false;
        for _ in 0..2000 {
            if engine.runs.get(run).is_none() {
                ended = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(ended, "run bị treo — deadlock chưa được sửa");
        let runs = engine.svc.db.list_runs(Some(1), 10).unwrap();
        let row = runs.iter().find(|r| r.id as u64 == run).unwrap();
        assert_eq!(row.status, "failed");
        assert!(row.error.as_deref().unwrap_or("").contains("vòng lặp"));
    }

    #[tokio::test]
    async fn debug_mode_records_a_hop_per_step() {
        let (engine, _rec) = harness();
        let nodes = vec![node("s", "src"), node("p", "pass"), node("k", "sink")];
        let edges = vec![
            edge("e1", "s", "out", "p", "in"),
            edge("e2", "p", "out", "k", "in"),
        ];
        engine.deploy(&chain(true), &nodes, &edges).await.unwrap();
        let run = engine
            .start_run(1, "s", "out", json!({ "v": 1 }), json!({}))
            .await
            .unwrap();
        settle(&engine).await;

        let hops = engine.svc.db.list_hops(run as i64).unwrap();
        assert!(hops.len() >= 3, "nguồn + pass + sink, nhận {}", hops.len());
        assert!(hops.iter().any(|h| h.node == "p" && h.out_port == "out"));
        assert!(hops.iter().any(|h| h.node == "k" && h.out_port.is_empty()));
    }

    #[tokio::test]
    async fn undeploy_stops_the_chain_and_abandons_its_runs() {
        let (engine, _rec) = harness();
        let nodes = vec![node("s", "src"), node("p", "pass")];
        let edges = vec![edge("e1", "s", "out", "p", "in")];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();
        assert!(engine.is_deployed(1));
        engine.undeploy(1).await;
        assert!(!engine.is_deployed(1));
        assert!(engine
            .start_run(1, "s", "out", json!({}), json!({}))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn redeploy_replaces_the_running_graph() {
        let (engine, rec) = harness();
        let nodes = vec![node("s", "src"), node("p", "pass"), node("k", "sink")];
        let edges = vec![
            edge("e1", "s", "out", "p", "in"),
            edge("e2", "p", "out", "k", "in"),
        ];
        engine.deploy(&chain(false), &nodes, &edges).await.unwrap();

        // Second version skips `p` entirely — the Go loader would have kept the
        // first version running forever.
        let nodes2 = vec![node("s", "src"), node("k", "sink")];
        let edges2 = vec![edge("e1", "s", "out", "k", "in")];
        engine
            .deploy(&chain(false), &nodes2, &edges2)
            .await
            .unwrap();

        engine
            .start_run(1, "s", "out", json!({ "v": 2 }), json!({}))
            .await
            .unwrap();
        settle(&engine).await;
        assert!(rec.seen("p").is_empty(), "node cũ không được chạy nữa");
        assert_eq!(rec.seen("k").len(), 1);
    }

    #[tokio::test]
    async fn deploy_refuses_a_graph_with_errors() {
        let (engine, _rec) = harness();
        let nodes = vec![node("s", "src"), node("p", "pass")];
        let edges = vec![edge("e1", "s", "khong-co-cong", "p", "in")];
        let err = engine
            .deploy(&chain(false), &nodes, &edges)
            .await
            .unwrap_err();
        assert!(err.contains("cổng ra"), "{err}");
        assert!(!engine.is_deployed(1));
    }
}
