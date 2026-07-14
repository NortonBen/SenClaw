//! HTTP surface (axum). Owns `AppState`: the metadata DB plus an in-memory
//! registry of one Oxigraph `Graph` per project (loaded lazily from the DB's
//! TriG snapshot, re-persisted after every mutation).

use crate::db::{now, Db};
use crate::graph::Graph;
use crate::{llm, mapping, prov, profile as prof, reason, resolve, shacl, tbox};
use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct AppState {
    pub db: Db,
    pub graphs: Mutex<HashMap<i64, Graph>>,
    pub mcp_tx: broadcast::Sender<String>,
}

impl AppState {
    /// Get-or-load the project's RDF graph.
    pub fn graph_for(&self, pid: i64) -> Result<Graph> {
        let mut map = self.graphs.lock().unwrap();
        if let Some(g) = map.get(&pid) {
            return Ok(g.clone());
        }
        let g = Graph::new()?;
        let trig = self.db.get_dataset(pid)?;
        if !trig.trim().is_empty() {
            g.load_trig(&trig)?;
        }
        map.insert(pid, g.clone());
        Ok(g)
    }

    /// Persist the project's graph back to the DB as a TriG snapshot.
    pub fn persist(&self, pid: i64, g: &Graph) -> Result<()> {
        let trig = g.dump_trig()?;
        self.db.set_dataset(pid, &trig)?;
        Ok(())
    }

    /// Base IRI + prefixes for a project.
    pub fn ctx(&self, pid: i64) -> Result<(String, HashMap<String, String>)> {
        let p = self
            .db
            .get_project(pid)?
            .ok_or_else(|| anyhow::anyhow!("project {pid} not found"))?;
        let prefixes = p
            .prefixes
            .as_object()
            .map(|o| o.iter().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect())
            .unwrap_or_default();
        Ok((p.base_iri, prefixes))
    }
}

pub fn make_state() -> Arc<AppState> {
    let db = Db::open(crate::db::default_db_path()).expect("open ontology db");
    let (tx, _) = broadcast::channel(256);
    Arc::new(AppState {
        db,
        graphs: Mutex::new(HashMap::new()),
        mcp_tx: tx,
    })
}

// ---- error plumbing -------------------------------------------------------

pub struct ApiErr(pub String);
impl<E: std::fmt::Display> From<E> for ApiErr {
    fn from(e: E) -> Self {
        ApiErr(e.to_string())
    }
}
impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, Json(json!({ "error": self.0 }))).into_response()
    }
}
type R = std::result::Result<Json<Value>, ApiErr>;

fn ok(v: Value) -> R {
    Ok(Json(v))
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/:id", get(get_project).delete(delete_project))
        .route("/projects/:id/prefixes", put(set_prefixes))
        .route("/projects/:id/base", put(set_base))
        .route("/projects/:id/export", get(export_ttl))
        .route("/projects/:id/sources", get(list_sources).post(add_source))
        .route("/projects/:id/sources/:sid", delete(delete_source))
        .route("/projects/:id/sources/:sid/profile", post(profile_source))
        .route("/projects/:id/tbox", get(get_tbox))
        .route("/projects/:id/tbox/class", post(add_class))
        .route("/projects/:id/tbox/property", post(add_property))
        .route("/projects/:id/tbox/apply", post(apply_tbox))
        .route("/projects/:id/tbox/term", delete(remove_term))
        .route("/projects/:id/tbox/draft", post(draft_tbox))
        .route("/projects/:id/tbox/graph", get(tbox_graph))
        .route("/projects/:id/mapping", get(get_mapping).put(set_mapping))
        .route("/projects/:id/mapping/preview", post(preview_mapping))
        .route("/projects/:id/mapping/lift", post(lift_mapping))
        .route("/projects/:id/mapping/draft", post(draft_mapping))
        .route("/projects/:id/sparql", post(run_sparql))
        .route("/projects/:id/nl2sparql", post(nl2sparql))
        .route("/projects/:id/graph", get(data_graph))
        .route("/projects/:id/competency", get(list_cq).post(add_cq))
        .route("/projects/:id/competency/run", post(run_cq))
        .route("/projects/:id/competency/:cid", put(update_cq).delete(delete_cq))
        .route("/projects/:id/shapes", get(get_shapes).put(set_shapes))
        .route("/projects/:id/shapes/draft", post(draft_shapes))
        .route("/projects/:id/validate", post(validate))
        .route("/projects/:id/materialize", post(materialize))
        .route("/projects/:id/materialize/clear", post(clear_inferred))
        .route("/projects/:id/resolve/candidates", post(resolve_candidates))
        .route("/projects/:id/resolve/apply", post(resolve_apply))
        .route("/projects/:id/batches", get(list_batches).delete(drop_batch))
        .route("/projects/:id/extract", post(extract))
        .route("/mcp/sse", get(crate::mcp::mcp_sse))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ---- status & projects ----------------------------------------------------

async fn status(State(s): State<Arc<AppState>>) -> R {
    let projects = s.db.list_projects(&|_| 0)?.len();
    ok(json!({ "ok": true, "name": "SenClaw Ontology", "version": "0.1.0", "projects": projects }))
}

async fn list_projects(State(s): State<Arc<AppState>>) -> R {
    let counts = |pid: i64| s.graph_for(pid).map(|g| g.len() as i64).unwrap_or(0);
    let list = s.db.list_projects(&counts)?;
    ok(json!(list))
}

async fn create_project(State(s): State<Arc<AppState>>, Json(b): Json<Value>) -> R {
    let name = b["name"].as_str().unwrap_or("").trim();
    if name.is_empty() {
        return Err(ApiErr("name is required".into()));
    }
    let base = b["baseIri"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("http://senclaw.local/onto/{}/", crate::vocab::encode_segment(name)));
    let id = s.db.create_project(name, b["description"].as_str().unwrap_or(""), &base)?;
    // Seed a default 'ex' domain prefix pointing at the project base.
    let ex = format!("{}#", base.trim_end_matches(['/', '#']));
    let _ = s.db.set_prefixes(id, &json!({ "ex": ex }));
    ok(json!({ "id": id, "baseIri": base }))
}

async fn get_project(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let mut p = s.db.get_project(id)?.ok_or_else(|| ApiErr("not found".into()))?;
    p.triple_count = s.graph_for(id).map(|g| g.len() as i64).unwrap_or(0);
    ok(serde_json::to_value(p)?)
}

async fn delete_project(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    // Hold the graphs lock across both the DB delete and the cache eviction so a
    // concurrent graph_for can't re-cache a ghost entry for the deleted project.
    // (graph_for also locks graphs-then-db, so the lock order matches — no deadlock.)
    let mut cache = s.graphs.lock().unwrap();
    s.db.delete_project(id)?;
    cache.remove(&id);
    ok(json!({ "ok": true }))
}

async fn set_prefixes(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    s.db.set_prefixes(id, &b["prefixes"])?;
    ok(json!({ "ok": true }))
}

async fn set_base(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let base = b["baseIri"].as_str().unwrap_or("").trim();
    if base.is_empty() {
        return Err(ApiErr("baseIri required".into()));
    }
    s.db.set_base_iri(id, base)?;
    ok(json!({ "ok": true }))
}

async fn export_ttl(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> std::result::Result<Response, ApiErr> {
    let g = s.graph_for(id)?;
    let trig = g.dump_trig()?;
    Ok(([(axum::http::header::CONTENT_TYPE, "application/trig")], trig).into_response())
}

// ---- sources --------------------------------------------------------------

async fn list_sources(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    ok(json!(s.db.list_sources(id)?))
}

async fn add_source(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let name = b["name"].as_str().unwrap_or("").trim();
    let content = b["content"].as_str().unwrap_or("");
    if name.is_empty() || content.is_empty() {
        return Err(ApiErr("name and content are required".into()));
    }
    let kind = b["kind"].as_str().unwrap_or_else(|| if name.ends_with(".json") { "json" } else { "csv" });
    let table = prof::parse(kind, content)?;
    let columns = json!(prof::profile(&table));
    let sid = s.db.add_source(id, name, kind, content, &columns, table.rows.len() as i64)?;
    s.db.log(id, "source.add", name);
    ok(json!({ "id": sid, "columns": columns, "rowCount": table.rows.len(), "kind": kind }))
}

async fn delete_source(State(s): State<Arc<AppState>>, Path((_id, sid)): Path<(i64, i64)>) -> R {
    s.db.delete_source(sid)?;
    ok(json!({ "ok": true }))
}

#[derive(serde::Deserialize)]
struct LlmFlag {
    #[serde(default)]
    llm: Option<String>,
}

async fn profile_source(
    State(s): State<Arc<AppState>>,
    Path((id, sid)): Path<(i64, i64)>,
    Query(q): Query<LlmFlag>,
) -> R {
    let (_name, kind, content) = s.db.get_source(sid)?.ok_or_else(|| ApiErr("source not found".into()))?;
    let table = prof::parse(&kind, &content)?;
    let columns = json!(prof::profile(&table));
    s.db.set_source_columns(sid, &columns)?;
    let mut out = json!({ "columns": columns, "rowCount": table.rows.len() });
    if q.llm.as_deref() == Some("1") {
        match llm::profile_roles(&columns).await {
            Ok((roles, model)) => out["llm"] = json!({ "roles": roles, "model": model }),
            Err(e) => out["llmError"] = json!(e),
        }
    }
    let _ = id;
    ok(out)
}

// ---- T-Box ----------------------------------------------------------------

async fn get_tbox(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let g = s.graph_for(id)?;
    ok(tbox::read(&g)?)
}

async fn add_class(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, pfx) = s.ctx(id)?;
    let def: tbox::ClassDef = serde_json::from_value(b)?;
    let g = s.graph_for(id)?;
    let iri = tbox::add_class(&g, &base, &pfx, &def)?;
    s.persist(id, &g)?;
    ok(json!({ "iri": iri }))
}

async fn add_property(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, pfx) = s.ctx(id)?;
    let def: tbox::PropertyDef = serde_json::from_value(b)?;
    let g = s.graph_for(id)?;
    let iri = tbox::add_property(&g, &base, &pfx, &def)?;
    s.persist(id, &g)?;
    ok(json!({ "iri": iri }))
}

async fn apply_tbox(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, pfx) = s.ctx(id)?;
    let draft: tbox::TboxDraft = serde_json::from_value(b.get("draft").cloned().unwrap_or(b))?;
    let g = s.graph_for(id)?;
    let (nc, np) = tbox::apply_draft(&g, &base, &pfx, &draft)?;
    // Merge any new prefixes from the draft into the project.
    if !draft.prefixes.is_empty() {
        let mut merged = pfx.clone();
        merged.extend(draft.prefixes.clone());
        s.db.set_prefixes(id, &json!(merged))?;
    }
    s.persist(id, &g)?;
    ok(json!({ "classes": nc, "properties": np }))
}

async fn remove_term(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, pfx) = s.ctx(id)?;
    let iri = b["iri"].as_str().unwrap_or("");
    let g = s.graph_for(id)?;
    tbox::remove_term(&g, &base, &pfx, iri)?;
    s.persist(id, &g)?;
    ok(json!({ "ok": true }))
}

async fn draft_tbox(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, _pfx) = s.ctx(id)?;
    let cqs: Vec<String> = s.db.list_cq(id)?.into_iter().map(|c| c.question).collect();
    // Use the specified source's columns, else the first source.
    let sources = s.db.list_sources(id)?;
    let sid = b["sourceId"].as_i64();
    let cols = sources
        .iter()
        .find(|src| Some(src.id) == sid)
        .or_else(|| sources.first())
        .map(|src| src.columns.clone())
        .unwrap_or_else(|| json!([]));
    let (draft, model) = llm::draft_tbox(&cqs, &cols, &base).await.map_err(ApiErr)?;
    ok(json!({ "draft": draft, "model": model }))
}

// ---- mapping --------------------------------------------------------------

async fn get_mapping(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    ok(s.db.get_mapping(id)?)
}

async fn set_mapping(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let m = b.get("mapping").cloned().unwrap_or(b);
    // Validate it parses as the DSL before saving.
    let _: mapping::Mapping = serde_json::from_value(m.clone()).map_err(|e| ApiErr(format!("invalid mapping: {e}")))?;
    s.db.set_mapping(id, &m)?;
    ok(json!({ "ok": true }))
}

/// Resolve the tables referenced by a mapping from the project's sources.
fn resolve_sources(s: &AppState, pid: i64, m: &mapping::Mapping) -> Result<HashMap<String, prof::Table>> {
    let mut out = HashMap::new();
    for tm in &m.triples_maps {
        if out.contains_key(&tm.source) {
            continue;
        }
        let (kind, content) = s
            .db
            .source_by_name(pid, &tm.source)?
            .ok_or_else(|| anyhow::anyhow!("source '{}' not uploaded", tm.source))?;
        out.insert(tm.source.clone(), prof::parse(&kind, &content)?);
    }
    Ok(out)
}

fn mapping_from(s: &AppState, pid: i64, b: &Value) -> Result<mapping::Mapping> {
    let raw = b.get("mapping").cloned().filter(|v| !v.is_null()).unwrap_or(s.db.get_mapping(pid)?);
    Ok(serde_json::from_value(raw)?)
}

async fn preview_mapping(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, _pfx) = s.ctx(id)?;
    let m = mapping_from(&s, id, &b)?;
    let sources = resolve_sources(&s, id, &m)?;
    let rep = mapping::preview(&m, &sources, &base, 25)?;
    ok(json!({
        "triples": rep.triples, "subjects": rep.subjects, "skippedRows": rep.skipped_rows,
        "samples": rep.samples.iter().map(|(a,b,c)| json!([a,b,c])).collect::<Vec<_>>(),
    }))
}

async fn lift_mapping(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, _pfx) = s.ctx(id)?;
    let m = mapping_from(&s, id, &b)?;
    let sources = resolve_sources(&s, id, &m)?;
    let g = s.graph_for(id)?;
    let ts = now();
    let label = m.triples_maps.first().map(|t| t.source.clone()).unwrap_or_else(|| "import".into());
    let batch = prov::batch_iri(ts, &label);
    let rep = mapping::lift(&g, &m, &sources, &base, &batch)?;
    prov::record_batch(&g, &batch, &label, &label, "lift", rep.triples, ts)?;
    s.persist(id, &g)?;
    s.db.log(id, "lift", &format!("{} triples into {batch}", rep.triples));
    ok(json!({
        "batch": batch, "triples": rep.triples, "subjects": rep.subjects, "skippedRows": rep.skipped_rows,
        "totalTriples": g.len(),
    }))
}

async fn draft_mapping(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, _pfx) = s.ctx(id)?;
    let sources = s.db.list_sources(id)?;
    let sid = b["sourceId"].as_i64();
    let src = sources.iter().find(|x| Some(x.id) == sid).or_else(|| sources.first())
        .ok_or_else(|| ApiErr("upload a source first".into()))?;
    let g = s.graph_for(id)?;
    let tb = tbox::read(&g)?;
    let (draft, model) = llm::draft_mapping(&src.columns, &tb, &src.name, &base).await.map_err(ApiErr)?;
    ok(json!({ "mapping": draft, "model": model }))
}

// ---- SPARQL + graph viz ---------------------------------------------------

async fn run_sparql(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let q = b["query"].as_str().unwrap_or("");
    if q.trim().is_empty() {
        return Err(ApiErr("query is required".into()));
    }
    let (_base, pfx) = s.ctx(id)?;
    let g = s.graph_for(id)?;
    ok(g.query_json(&crate::vocab::ensure_prefixes(q, &pfx))?)
}

async fn nl2sparql(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let question = b["question"].as_str().unwrap_or("");
    let g = s.graph_for(id)?;
    let tb = tbox::read(&g)?;
    let (sparql, model) = llm::nl_to_sparql(question, &tb).await.map_err(ApiErr)?;
    ok(json!({ "sparql": sparql, "model": model }))
}

#[derive(serde::Deserialize)]
struct LimitQ {
    #[serde(default)]
    limit: Option<usize>,
}

/// A-Box sample as nodes/edges for the explorer.
async fn data_graph(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Query(q): Query<LimitQ>) -> R {
    let g = s.graph_for(id)?;
    let limit = q.limit.unwrap_or(250).min(2000);
    let rows = g.query_json(&format!(
        "SELECT ?s ?p ?o WHERE {{ ?s ?p ?o . FILTER(isIRI(?s)) }} LIMIT {limit}"
    ))?;
    ok(build_graph_viz(&rows))
}

async fn tbox_graph(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let g = s.graph_for(id)?;
    let rows = g.query_json(&format!(
        "{}SELECT ?s ?p ?o WHERE {{ GRAPH <{}> {{ ?s ?p ?o }} FILTER(isIRI(?s)) }} LIMIT 1000",
        crate::vocab::PREFIXES,
        crate::graph::TBOX_GRAPH
    ))?;
    ok(build_graph_viz(&rows))
}

/// Turn `?s ?p ?o` rows into `{ nodes:[{id,label,kind}], edges:[{source,target,label}] }`.
fn build_graph_viz(rows: &Value) -> Value {
    use std::collections::BTreeMap;
    let mut nodes: BTreeMap<String, Value> = BTreeMap::new();
    let mut edges: Vec<Value> = Vec::new();
    let short = |iri: &str| iri.rsplit(['#', '/']).next().unwrap_or(iri).to_string();
    if let Some(rows) = rows["rows"].as_array() {
        for r in rows {
            let s = r["s"]["value"].as_str().unwrap_or("");
            let p = r["p"]["value"].as_str().unwrap_or("");
            let o = &r["o"];
            let otype = o["type"].as_str().unwrap_or("literal");
            let oval = o["value"].as_str().unwrap_or("");
            if s.is_empty() {
                continue;
            }
            nodes.entry(s.to_string()).or_insert_with(|| json!({ "id": s, "label": short(s), "kind": "iri" }));
            if p.ends_with("22-rdf-syntax-ns#type") {
                // fold rdf:type into a node attribute rather than an edge.
                if let Some(n) = nodes.get_mut(s) {
                    n["type"] = json!(short(oval));
                }
                nodes.entry(oval.to_string()).or_insert_with(|| json!({ "id": oval, "label": short(oval), "kind": "class" }));
                continue;
            }
            if otype == "uri" {
                nodes.entry(oval.to_string()).or_insert_with(|| json!({ "id": oval, "label": short(oval), "kind": "iri" }));
                edges.push(json!({ "source": s, "target": oval, "label": short(p) }));
            } else {
                let lit_id = format!("lit:{s}:{p}:{oval}");
                nodes.insert(lit_id.clone(), json!({ "id": lit_id, "label": oval, "kind": "literal" }));
                edges.push(json!({ "source": s, "target": lit_id, "label": short(p) }));
            }
        }
    }
    json!({ "nodes": nodes.into_values().collect::<Vec<_>>(), "edges": edges })
}

// ---- competency questions -------------------------------------------------

async fn list_cq(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    ok(json!(s.db.list_cq(id)?))
}

async fn add_cq(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let cid = s.db.add_cq(
        id,
        b["question"].as_str().unwrap_or(""),
        b["sparql"].as_str().unwrap_or(""),
        b["expect"].as_str().unwrap_or("nonempty"),
    )?;
    ok(json!({ "id": cid }))
}

async fn update_cq(State(s): State<Arc<AppState>>, Path((_id, cid)): Path<(i64, i64)>, Json(b): Json<Value>) -> R {
    s.db.update_cq(
        cid,
        b["question"].as_str().unwrap_or(""),
        b["sparql"].as_str().unwrap_or(""),
        b["expect"].as_str().unwrap_or("nonempty"),
    )?;
    ok(json!({ "ok": true }))
}

async fn delete_cq(State(s): State<Arc<AppState>>, Path((_id, cid)): Path<(i64, i64)>) -> R {
    s.db.delete_cq(cid)?;
    ok(json!({ "ok": true }))
}

/// Run every competency question and report pass/fail (the "each CQ = one
/// SPARQL that returns the right thing" checklist from the research).
async fn run_cq(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let (_base, pfx) = s.ctx(id)?;
    let g = s.graph_for(id)?;
    let mut results = Vec::new();
    let mut passed = 0;
    let cqs = s.db.list_cq(id)?;
    for cq in &cqs {
        if cq.sparql.trim().is_empty() {
            results.push(json!({ "id": cq.id, "question": cq.question, "pass": false, "error": "no SPARQL" }));
            continue;
        }
        match g.query_json(&crate::vocab::ensure_prefixes(&cq.sparql, &pfx)) {
            Ok(res) => {
                let count = res["rows"].as_array().map(|a| a.len())
                    .or_else(|| res.get("boolean").map(|b| if b.as_bool().unwrap_or(false) { 1 } else { 0 }))
                    .unwrap_or(0);
                let pass = match cq.expect.as_str() {
                    "empty" => count == 0,
                    "boolean" => res["boolean"].as_bool().unwrap_or(false),
                    _ => count > 0,
                };
                if pass {
                    passed += 1;
                }
                results.push(json!({ "id": cq.id, "question": cq.question, "pass": pass, "count": count }));
            }
            Err(e) => results.push(json!({ "id": cq.id, "question": cq.question, "pass": false, "error": e.to_string() })),
        }
    }
    ok(json!({ "total": cqs.len(), "passed": passed, "results": results }))
}

// ---- SHACL ----------------------------------------------------------------

async fn get_shapes(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    ok(s.db.get_shapes(id)?)
}

async fn set_shapes(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let shapes = b.get("shapes").cloned().unwrap_or(b);
    let _: shacl::Shapes = serde_json::from_value(shapes.clone()).map_err(|e| ApiErr(format!("invalid shapes: {e}")))?;
    s.db.set_shapes(id, &shapes)?;
    ok(json!({ "ok": true }))
}

async fn draft_shapes(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let g = s.graph_for(id)?;
    let tb = tbox::read(&g)?;
    let (shapes, model) = llm::draft_shapes(&tb).await.map_err(ApiErr)?;
    ok(json!({ "shapes": shapes, "model": model }))
}

async fn validate(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, pfx) = s.ctx(id)?;
    let raw = b.get("shapes").cloned().filter(|v| !v.is_null()).unwrap_or(s.db.get_shapes(id)?);
    let shapes: shacl::Shapes = serde_json::from_value(raw)?;
    let g = s.graph_for(id)?;
    ok(shacl::validate(&g, &base, &pfx, &shapes)?)
}

// ---- reasoning ------------------------------------------------------------

async fn materialize(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let g = s.graph_for(id)?;
    let rep = reason::materialize(&g)?;
    s.persist(id, &g)?;
    s.db.log(id, "materialize", &rep.to_string());
    ok(rep)
}

async fn clear_inferred(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let g = s.graph_for(id)?;
    reason::clear(&g)?;
    s.persist(id, &g)?;
    ok(json!({ "ok": true }))
}

// ---- entity resolution ----------------------------------------------------

async fn resolve_candidates(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, pfx) = s.ctx(id)?;
    let g = s.graph_for(id)?;
    let class = b["class"].as_str().unwrap_or("");
    let label_prop = b["labelProp"].as_str().unwrap_or("rdfs:label");
    let threshold = b["threshold"].as_f64().unwrap_or(0.9);
    ok(resolve::candidates(&g, &base, &pfx, class, label_prop, threshold)?)
}

async fn resolve_apply(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, pfx) = s.ctx(id)?;
    let g = s.graph_for(id)?;
    let predicate = b["predicate"].as_str().unwrap_or("skos:closeMatch");
    let pairs: Vec<(String, String)> = b["pairs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default();
    let n = resolve::apply(&g, &base, &pfx, predicate, &pairs)?;
    s.persist(id, &g)?;
    ok(json!({ "applied": n }))
}

// ---- provenance batches ---------------------------------------------------

async fn list_batches(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let g = s.graph_for(id)?;
    ok(prov::list_batches(&g)?)
}

async fn drop_batch(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let iri = b["iri"].as_str().unwrap_or("");
    let g = s.graph_for(id)?;
    prov::drop_batch(&g, iri)?;
    s.persist(id, &g)?;
    ok(json!({ "ok": true }))
}

// ---- unstructured extraction ----------------------------------------------

async fn extract(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, pfx) = s.ctx(id)?;
    let text = b["text"].as_str().unwrap_or("");
    if text.trim().is_empty() {
        return Err(ApiErr("text is required".into()));
    }
    let g = s.graph_for(id)?;
    let tb = tbox::read(&g)?;
    let (extracted, model) = llm::extract_triples(text, &tb).await.map_err(ApiErr)?;
    // Insert into a dedicated extraction batch with provenance + confidence.
    let ts = now();
    let batch = prov::batch_iri(ts, "text-extract");
    let mut body = String::new();
    let mut inserted = 0;
    if let Some(arr) = extracted["triples"].as_array() {
        for t in arr {
            let s_raw = t["s"].as_str().unwrap_or("");
            let p_raw = t["p"].as_str().unwrap_or("");
            let o_raw = t["o"].as_str().unwrap_or("");
            let o_is_lit = t["oIsLiteral"].as_bool().unwrap_or(true);
            if s_raw.is_empty() || p_raw.is_empty() || o_raw.is_empty() {
                continue;
            }
            let subj = to_iri_or_mint(s_raw, &base, &pfx);
            let pred = crate::vocab::expand(p_raw, &pfx, &base);
            let (Some(st), Some(pt)) = (crate::vocab::iri_term(&subj), crate::vocab::iri_term(&pred)) else { continue };
            let obj = if o_is_lit {
                format!("\"{}\"", crate::vocab::escape_literal(o_raw))
            } else {
                match crate::vocab::iri_term(&to_iri_or_mint(o_raw, &base, &pfx)) {
                    Some(x) => x,
                    None => continue,
                }
            };
            body.push_str(&format!("{st} {pt} {obj} .\n"));
            inserted += 1;
        }
    }
    if inserted > 0 {
        g.update(&format!("INSERT DATA {{ GRAPH <{batch}> {{\n{body}}} }}"))?;
        prov::record_batch(&g, &batch, "text extraction", "unstructured text (LLM)", "extract", inserted, ts)?;
        s.persist(id, &g)?;
    }
    ok(json!({ "inserted": inserted, "model": model, "batch": batch, "raw": extracted }))
}

/// Turn an LLM-provided subject/object into an IRI: expand a curie/IRI, else
/// mint a stable hashed IRI from the label.
fn to_iri_or_mint(raw: &str, base: &str, pfx: &HashMap<String, String>) -> String {
    let r = raw.trim();
    if r.starts_with("http://") || r.starts_with("https://") || r.starts_with("urn:") || r.contains(':') {
        crate::vocab::expand(r, pfx, base)
    } else {
        crate::vocab::hashed_iri(base, "entity", &[r])
    }
}
