//! REST surface. Also the ingress for webhook-style sources.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::engine::graph;
use crate::engine::types::{next_id, Edge};
use crate::model::*;
use crate::state::AppState;

type Api = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn bad(msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "error": msg.into() })),
    )
}

fn not_found(msg: impl Into<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "ok": false, "error": msg.into() })),
    )
}

fn oops(e: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "ok": false, "error": e.to_string() })),
    )
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/registry", get(registry))
        .route("/chains", get(list_chains).post(create_chain))
        .route("/chains/:id", get(get_chain).delete(delete_chain))
        .route("/chains/:id", patch(patch_chain))
        .route("/chains/:id/graph", put(put_graph))
        .route("/chains/:id/validate", post(validate_chain))
        .route("/chains/:id/activate", post(activate))
        .route("/chains/:id/deactivate", post(deactivate))
        .route("/chains/:id/trigger", post(trigger))
        .route("/chains/:id/runs", get(list_runs))
        .route("/chains/:id/logs", get(list_logs))
        .route("/chains/:id/state", delete(clear_state))
        .route("/runs/:run_id/hops", get(list_hops))
        .route("/events", get(events))
        .route(
            "/hooks/:hook_id",
            post(webhook_ingress).get(webhook_ingress),
        )
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ------------------------------------------------------------------ status

async fn status(State(st): State<Arc<AppState>>) -> Json<Value> {
    let chains = st.db.list_chains().unwrap_or_default();
    Json(json!({
        "ok": true,
        "app": "rule-engine",
        "version": env!("CARGO_PKG_VERSION"),
        "chains": chains.len(),
        "active": chains.iter().filter(|c| c.status == ChainStatus::Active).count(),
        "deployed": st.engine.deployed_chains().len(),
        "runningRuns": st.engine.runs.active(),
        "nodeTypes": st.engine.registry.len(),
    }))
}

async fn registry(State(st): State<Arc<AppState>>) -> Json<Value> {
    let specs: Vec<Value> = st
        .engine
        .registry
        .specs()
        .into_iter()
        .map(|s| {
            let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
            v["isSource"] = json!(st.engine.registry.is_source(&s.id));
            v
        })
        .collect();
    Json(json!({ "ok": true, "rules": specs }))
}

// ------------------------------------------------------------------ chains

async fn list_chains(State(st): State<Arc<AppState>>) -> Api {
    let chains = st.db.list_chains().map_err(oops)?;
    let deployed = st.engine.deployed_chains();
    let out: Vec<Value> = chains
        .into_iter()
        .map(|c| {
            let mut v = serde_json::to_value(&c).unwrap_or(Value::Null);
            v["deployed"] = json!(deployed.contains(&c.id));
            v
        })
        .collect();
    Ok(Json(json!({ "ok": true, "chains": out })))
}

#[derive(Deserialize)]
struct CreateChainBody {
    name: String,
    #[serde(default)]
    description: String,
}

async fn create_chain(State(st): State<Arc<AppState>>, Json(body): Json<CreateChainBody>) -> Api {
    if body.name.trim().is_empty() {
        return Err(bad("Tên luồng không được để trống."));
    }
    let id = next_id() as i64;
    let chain = st
        .db
        .create_chain(id, body.name.trim(), &body.description)
        .map_err(oops)?;
    Ok(Json(json!({ "ok": true, "chain": chain })))
}

async fn get_chain(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Api {
    let chain = st
        .db
        .get_chain(id)
        .map_err(oops)?
        .ok_or_else(|| not_found("Không có luồng này."))?;
    let nodes = st.db.list_nodes(id).map_err(oops)?;
    let edges = st.db.list_edges(id).map_err(oops)?;
    let issues = graph::validate(&nodes, &edges, &st.engine.registry);
    Ok(Json(json!({
        "ok": true,
        "chain": chain,
        "nodes": nodes,
        "edges": edges,
        "issues": issues,
        "deployed": st.engine.is_deployed(id),
    })))
}

#[derive(Deserialize)]
struct PatchChainBody {
    name: Option<String>,
    description: Option<String>,
    debug: Option<bool>,
}

async fn patch_chain(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<PatchChainBody>,
) -> Api {
    st.db
        .update_chain_meta(
            id,
            body.name.as_deref(),
            body.description.as_deref(),
            body.debug,
        )
        .map_err(oops)?;
    // Debug is compiled into the deployment, so a live chain must be rebuilt.
    if body.debug.is_some() && st.engine.is_deployed(id) {
        redeploy(&st, id).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn delete_chain(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Api {
    st.engine.undeploy(id).await;
    st.db.delete_chain(id).map_err(oops)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct GraphBody {
    #[serde(default)]
    nodes: Vec<Node>,
    #[serde(default)]
    edges: Vec<Edge>,
}

async fn put_graph(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<GraphBody>,
) -> Api {
    let chain = st
        .db
        .get_chain(id)
        .map_err(oops)?
        .ok_or_else(|| not_found("Không có luồng này."))?;

    let issues = graph::validate(&body.nodes, &body.edges, &st.engine.registry);
    // Saving a broken draft is allowed; activating it is not.
    st.db
        .replace_graph(id, &body.nodes, &body.edges)
        .map_err(oops)?;

    let mut redeployed = false;
    if chain.status == ChainStatus::Active {
        if graph::has_errors(&issues) {
            st.engine.undeploy(id).await;
            st.db
                .set_chain_status(id, ChainStatus::Error)
                .map_err(oops)?;
        } else {
            redeploy(&st, id).await?;
            redeployed = true;
        }
    }
    Ok(Json(
        json!({ "ok": true, "issues": issues, "redeployed": redeployed }),
    ))
}

async fn validate_chain(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Api {
    let nodes = st.db.list_nodes(id).map_err(oops)?;
    let edges = st.db.list_edges(id).map_err(oops)?;
    let issues = graph::validate(&nodes, &edges, &st.engine.registry);
    Ok(Json(json!({
        "ok": !graph::has_errors(&issues),
        "issues": issues,
    })))
}

async fn redeploy(
    st: &Arc<AppState>,
    id: i64,
) -> Result<Vec<graph::Issue>, (StatusCode, Json<Value>)> {
    let chain = st
        .db
        .get_chain(id)
        .map_err(oops)?
        .ok_or_else(|| not_found("Không có luồng này."))?;
    let nodes = st.db.list_nodes(id).map_err(oops)?;
    let edges = st.db.list_edges(id).map_err(oops)?;
    st.engine.deploy(&chain, &nodes, &edges).await.map_err(bad)
}

async fn activate(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Api {
    let issues = redeploy(&st, id).await?;
    st.db
        .set_chain_status(id, ChainStatus::Active)
        .map_err(oops)?;
    Ok(Json(json!({ "ok": true, "issues": issues })))
}

async fn deactivate(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Api {
    st.engine.undeploy(id).await;
    st.db
        .set_chain_status(id, ChainStatus::Inactive)
        .map_err(oops)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct TriggerBody {
    node: Option<String>,
    port: Option<String>,
    #[serde(default)]
    data: Value,
    #[serde(default)]
    meta: Value,
}

async fn trigger(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<TriggerBody>,
) -> Api {
    if !st.engine.is_deployed(id) {
        return Err(bad(
            "Luồng chưa chạy. Bấm Kích hoạt trước khi bơm sự kiện thử.",
        ));
    }
    let node = match body.node {
        Some(n) => n,
        None => {
            let nodes = st.db.list_nodes(id).map_err(oops)?;
            nodes
                .iter()
                .find(|n| n.rule == "manual")
                .map(|n| n.id.clone())
                .ok_or_else(|| {
                    bad("Luồng không có node `manual` nào — chỉ rõ `node` cần bơm sự kiện.")
                })?
        }
    };
    let port = body.port.unwrap_or_else(|| "out".to_string());
    let data = if body.data.is_null() {
        json!({})
    } else {
        body.data
    };
    let meta = if body.meta.is_null() {
        json!({})
    } else {
        body.meta
    };
    match st.engine.start_run(id, &node, &port, data, meta).await {
        Some(run_id) => Ok(Json(json!({ "ok": true, "runId": run_id }))),
        None => Err(bad(format!(
            "Không tìm thấy node `{node}` trong luồng đang chạy."
        ))),
    }
}

// -------------------------------------------------------------- runs/logs

#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<i64>,
}

async fn list_runs(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(q): Query<LimitQuery>,
) -> Api {
    let runs = st
        .db
        .list_runs(Some(id), q.limit.unwrap_or(50).clamp(1, 500))
        .map_err(oops)?;
    Ok(Json(json!({ "ok": true, "runs": runs })))
}

async fn list_hops(State(st): State<Arc<AppState>>, Path(run_id): Path<i64>) -> Api {
    let hops = st.db.list_hops(run_id).map_err(oops)?;
    Ok(Json(json!({ "ok": true, "hops": hops })))
}

async fn list_logs(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(q): Query<LimitQuery>,
) -> Api {
    let logs = st
        .db
        .list_logs(id, q.limit.unwrap_or(200).clamp(1, 2000))
        .map_err(oops)?;
    Ok(Json(json!({ "ok": true, "logs": logs })))
}

async fn clear_state(State(st): State<Arc<AppState>>, Path(id): Path<i64>) -> Api {
    st.db.state_clear(id, None).map_err(oops)?;
    Ok(Json(json!({ "ok": true })))
}

// ------------------------------------------------------------------ events

async fn events(
    State(st): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = st.engine.svc.bus.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("ready").data("{}"));
        loop {
            match rx.recv().await {
                Ok(msg) => yield Ok(Event::default().event("engine").data(msg)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    yield Ok(Event::default()
                        .event("lagged")
                        .data(json!({ "skipped": n }).to_string()));
                }
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ----------------------------------------------------------------- ingress

/// `POST /api/hooks/:hook_id` — every deployed `webhook` node with that id.
async fn webhook_ingress(
    State(st): State<Arc<AppState>>,
    Path(hook_id): Path<String>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> Api {
    let emitters = crate::rules::webhook::routes().get(&hook_id);
    if emitters.is_empty() {
        return Err(not_found(format!(
            "Không có node webhook nào đang lắng nghe `{hook_id}`."
        )));
    }
    let payload = body.map(|Json(v)| v).unwrap_or(json!({}));
    let header_map: HashMap<String, String> = headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|s| (k.as_str().to_string(), s.to_string()))
        })
        .collect();

    let given_secret = header_map
        .get("x-webhook-secret")
        .or_else(|| header_map.get("x-hook-secret"))
        .cloned()
        .unwrap_or_default();

    let mut accepted = 0usize;
    let mut rejected = 0usize;
    for em in emitters {
        // Load this node's webhook config once. If it can't be read (DB error,
        // node vanished mid-request) we CANNOT verify the secret, so we refuse
        // to deliver — fail closed, not open.
        let Some((secret, include_headers)) = node_webhook_config(&st, em.chain_id(), em.node())
        else {
            rejected += 1;
            continue;
        };
        if let Some(secret) = secret {
            if given_secret != secret {
                rejected += 1;
                continue;
            }
        }
        // Attach headers only when the node opts in, and never forward the
        // sensitive ones downstream even then — the webhook secret, and the
        // standard auth/credential headers an inbound caller might send.
        let mut meta = json!({ "_event": "webhook", "webhookId": hook_id });
        if include_headers {
            let safe: HashMap<&String, &String> = header_map
                .iter()
                .filter(|(k, _)| !is_sensitive_header(k))
                .collect();
            meta["headers"] = json!(safe);
        }
        em.emit("out", payload.clone(), meta).await;
        accepted += 1;
    }
    if accepted == 0 {
        // Every listener refused: wrong secret, or none could be verified.
        let msg = if rejected > 0 {
            "Sai secret của webhook, hoặc không xác minh được node."
        } else {
            "Không có node nào nhận."
        };
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "ok": false, "error": msg })),
        ));
    }
    Ok(Json(json!({ "ok": true, "delivered": accepted })))
}

/// Header names never forwarded into the flow, even with `includeHeaders` on:
/// the webhook's own secret plus the caller's credential headers, which have no
/// business ending up in a log or a downstream request.
fn is_sensitive_header(name: &str) -> bool {
    matches!(
        name,
        "x-webhook-secret" | "x-hook-secret" | "authorization" | "proxy-authorization" | "cookie"
    )
}

/// `(configured secret, whether to forward headers)` for a webhook node, or
/// `None` when the node can't be read — which the caller treats as "cannot
/// verify, do not deliver".
fn node_webhook_config(
    st: &Arc<AppState>,
    chain_id: i64,
    node: &str,
) -> Option<(Option<String>, bool)> {
    let nodes = st.db.list_nodes(chain_id).ok()?;
    let cfg = &nodes.iter().find(|n| n.id == node)?.config;
    Some((
        crate::rules::webhook::secret_of(cfg),
        crate::rules::webhook::include_headers(cfg),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sensitive_headers_are_never_forwarded() {
        for h in [
            "x-webhook-secret",
            "x-hook-secret",
            "authorization",
            "proxy-authorization",
            "cookie",
        ] {
            assert!(is_sensitive_header(h), "`{h}` phải bị lược");
        }
        assert!(!is_sensitive_header("x-custom"));
        assert!(!is_sensitive_header("content-type"));
    }

    fn state() -> Arc<AppState> {
        std::env::set_var("RULE_ENGINE_DATA_DIR", "");
        let db = Arc::new(crate::db::Db::open(":memory:").unwrap());
        let bus = crate::engine::services::EventBus::new();
        let svc = Arc::new(crate::engine::services::Services::new(db.clone(), bus));
        let mut reg = crate::engine::registry::Registry::new();
        crate::rules::register(&mut reg);
        let engine = crate::engine::Engine::start(Arc::new(reg), svc);
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        Arc::new(AppState { db, engine, mcp_tx })
    }

    #[tokio::test]
    async fn status_reports_the_registry_size() {
        let st = state();
        let Json(v) = status(State(st.clone())).await;
        assert_eq!(v["ok"], true);
        assert!(v["nodeTypes"].as_u64().unwrap() > 10);
    }

    #[tokio::test]
    async fn registry_exposes_ports_for_the_ui() {
        let st = state();
        let Json(v) = registry(State(st)).await;
        let rules = v["rules"].as_array().unwrap();
        let cond = rules
            .iter()
            .find(|r| r["id"] == "conditional")
            .expect("conditional phải có trong registry");
        let ports: Vec<&str> = cond["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert!(ports.contains(&"true"));
        assert!(ports.contains(&"false"));
        assert!(ports.contains(&"error"));
        assert_eq!(cond["isSource"], false);
    }

    #[tokio::test]
    async fn create_then_read_a_chain() {
        let st = state();
        let Json(v) = create_chain(
            State(st.clone()),
            Json(CreateChainBody {
                name: "Luồng test".into(),
                description: String::new(),
            }),
        )
        .await
        .unwrap();
        let id = v["chain"]["id"].as_i64().unwrap();
        let Json(got) = get_chain(State(st), Path(id)).await.unwrap();
        assert_eq!(got["chain"]["name"], "Luồng test");
        assert_eq!(got["deployed"], false);
    }

    #[tokio::test]
    async fn creating_a_chain_without_a_name_is_rejected() {
        let st = state();
        let err = create_chain(
            State(st),
            Json(CreateChainBody {
                name: "   ".into(),
                description: String::new(),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn activating_a_broken_graph_fails_with_the_reason() {
        let st = state();
        let Json(v) = create_chain(
            State(st.clone()),
            Json(CreateChainBody {
                name: "hỏng".into(),
                description: String::new(),
            }),
        )
        .await
        .unwrap();
        let id = v["chain"]["id"].as_i64().unwrap();
        // An edge to a port that does not exist.
        let nodes = vec![Node {
            id: "a".into(),
            chain_id: id,
            rule: "manual".into(),
            name: "A".into(),
            config: json!({}),
            opts: NodeOpts::default(),
            x: 0.0,
            y: 0.0,
            debug: false,
        }];
        let edges = vec![Edge {
            id: "e".into(),
            from: crate::engine::types::PortRef::new("a", "nope"),
            to: crate::engine::types::PortRef::new("a", "in"),
        }];
        put_graph(
            State(st.clone()),
            Path(id),
            Json(GraphBody { nodes, edges }),
        )
        .await
        .unwrap();
        let err = activate(State(st), Path(id)).await.unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn trigger_refuses_when_the_chain_is_not_running() {
        let st = state();
        let Json(v) = create_chain(
            State(st.clone()),
            Json(CreateChainBody {
                name: "x".into(),
                description: String::new(),
            }),
        )
        .await
        .unwrap();
        let id = v["chain"]["id"].as_i64().unwrap();
        let err = trigger(
            State(st),
            Path(id),
            Json(TriggerBody {
                node: None,
                port: None,
                data: json!({}),
                meta: json!({}),
            }),
        )
        .await
        .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }
}
