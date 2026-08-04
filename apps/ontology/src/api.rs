//! HTTP surface (axum). Owns `AppState`: the metadata DB plus an in-memory
//! registry of one Oxigraph `Graph` per project (loaded lazily from the DB's
//! TriG snapshot, re-persisted after every mutation).

use crate::db::{now, Db};
use crate::graph::Graph;
use crate::{
    aip, auto, ingest, llm, logic, mapping, profile as prof, prov, reason, resolve, shacl, tbox,
};
use anyhow::Result;
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// Uploads are base64 in a JSON body; axum's 2 MB default would reject a
/// perfectly ordinary spreadsheet.
const MAX_UPLOAD: usize = 64 * 1024 * 1024;

pub struct AppState {
    pub db: Db,
    pub graphs: Mutex<HashMap<i64, Graph>>,
    pub mcp_tx: broadcast::Sender<String>,
    /// In-flight (and recently finished) autobuild jobs, polled by the UI.
    pub jobs: Mutex<HashMap<String, auto::JobHandle>>,
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
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();
        Ok((p.base_iri, prefixes))
    }
}

pub fn make_state() -> Arc<AppState> {
    let db = Db::open(crate::db::default_db_path()).expect("open ontology db");
    // Seed the process-wide LLM profile from the persisted setting, falling back
    // to ONTOLOGY_LLM_PROFILE the first time (before the user has picked one in
    // the UI). After that the stored value wins, so a restart keeps the choice.
    let stored = db.get_setting("llm_profile", "");
    let seed = if stored.trim().is_empty() {
        std::env::var("ONTOLOGY_LLM_PROFILE").unwrap_or_default()
    } else {
        stored
    };
    llm::set_profile(&seed);
    let (tx, _) = broadcast::channel(256);
    Arc::new(AppState {
        db,
        graphs: Mutex::new(HashMap::new()),
        mcp_tx: tx,
        jobs: Mutex::new(HashMap::new()),
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
        .route("/projects/:id/ingest", post(ingest_file))
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
        .route(
            "/projects/:id/competency/:cid",
            put(update_cq).delete(delete_cq),
        )
        .route("/projects/:id/shapes", get(get_shapes).put(set_shapes))
        .route("/projects/:id/shapes/draft", post(draft_shapes))
        .route("/projects/:id/validate", post(validate))
        .route("/projects/:id/materialize", post(materialize))
        .route("/projects/:id/materialize/clear", post(clear_inferred))
        .route("/projects/:id/resolve/candidates", post(resolve_candidates))
        .route("/projects/:id/resolve/apply", post(resolve_apply))
        .route(
            "/projects/:id/batches",
            get(list_batches).delete(drop_batch),
        )
        .route("/projects/:id/extract", post(extract))
        .route("/projects/:id/schema", get(live_schema))
        .route("/projects/:id/autobuild", post(autobuild))
        .route("/projects/:id/autobuild/:job", get(autobuild_status))
        .route("/projects/:id/ask", post(ask))
        .route("/projects/:id/assist", post(assist))
        .route("/projects/:id/assist/index", get(assist_index))
        // AIP Logic: typed-action functions + proposal queue + evals
        .route(
            "/projects/:id/functions",
            get(list_functions).post(create_function),
        )
        .route("/projects/:id/functions/:fid", delete(delete_function))
        .route("/projects/:id/functions/:fid/run", post(run_function))
        .route("/projects/:id/functions/:fid/trial", post(trial_function))
        .route(
            "/projects/:id/functions/:fid/evals",
            get(list_evals).post(add_eval),
        )
        .route("/projects/:id/functions/:fid/evals/run", post(run_evals))
        .route("/projects/:id/proposals", get(list_proposals))
        .route("/projects/:id/proposals/approve", post(approve_proposals))
        .route("/projects/:id/proposals/reject", post(reject_proposals))
        .route("/formats", get(formats))
        .route("/models", get(list_models))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/mcp/sse", get(crate::mcp::mcp_sse))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD))
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
        .unwrap_or_else(|| {
            format!(
                "http://senclaw.local/onto/{}/",
                crate::vocab::encode_segment(name)
            )
        });
    let id =
        s.db.create_project(name, b["description"].as_str().unwrap_or(""), &base)?;
    // Seed a default 'ex' domain prefix pointing at the project base.
    let ex = format!("{}#", base.trim_end_matches(['/', '#']));
    let _ = s.db.set_prefixes(id, &json!({ "ex": ex }));
    ok(json!({ "id": id, "baseIri": base }))
}

async fn get_project(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let mut p =
        s.db.get_project(id)?
            .ok_or_else(|| ApiErr("not found".into()))?;
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

async fn set_prefixes(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
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

async fn export_ttl(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> std::result::Result<Response, ApiErr> {
    let g = s.graph_for(id)?;
    let trig = g.dump_trig()?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "application/trig")],
        trig,
    )
        .into_response())
}

// ---- sources --------------------------------------------------------------

async fn list_sources(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    ok(json!(s.db.list_sources(id)?))
}

/// Add a source whose format you already know (`csv | json | text`). For "here
/// is a file, figure it out" use `/ingest` instead.
async fn add_source(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let name = b["name"].as_str().unwrap_or("").trim();
    let content = b["content"].as_str().unwrap_or("");
    if name.is_empty() || content.is_empty() {
        return Err(ApiErr("name and content are required".into()));
    }
    // An explicit kind is honoured; otherwise the sniffer decides, so a caller
    // that pastes YAML/XML/JSONL into `content` still gets a usable table.
    match b["kind"].as_str() {
        Some(kind) if !kind.trim().is_empty() => {
            let table = prof::parse(kind, content)?;
            let columns = json!(prof::profile(&table));
            let sid = s.db.add_source(
                id,
                name,
                kind,
                content,
                &columns,
                table.rows.len() as i64,
                kind,
                "",
            )?;
            s.db.log(id, "source.add", name);
            ok(json!({ "id": sid, "columns": columns, "rowCount": table.rows.len(), "kind": kind }))
        }
        _ => {
            let created = store_ingested(
                &s,
                id,
                ingest::ingest_text(name, content).map_err(ApiErr::from)?,
            )?;
            ok(json!({ "sources": created }))
        }
    }
}

/// **Universal ingest**: hand over a file (text in `content`, or bytes in
/// `contentBase64`) and get back one or more profiled sources. Format detection
/// is by magic bytes and structure — the filename is only a naming hint.
async fn ingest_file(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let filename = b["filename"]
        .as_str()
        .or_else(|| b["name"].as_str())
        .unwrap_or("upload")
        .trim();
    let parts = if let Some(b64) = b["contentBase64"].as_str().filter(|x| !x.trim().is_empty()) {
        use base64::Engine;
        // Tolerate a `data:...;base64,` prefix from a browser FileReader.
        let raw = b64.rsplit_once("base64,").map(|(_, r)| r).unwrap_or(b64);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(raw.trim())
            .map_err(|e| ApiErr(format!("contentBase64 is not valid base64: {e}")))?;
        ingest::ingest(filename, &bytes).map_err(ApiErr::from)?
    } else {
        let content = b["content"].as_str().unwrap_or("");
        if content.trim().is_empty() {
            return Err(ApiErr("content or contentBase64 is required".into()));
        }
        ingest::ingest(filename, content.as_bytes()).map_err(ApiErr::from)?
    };
    let created = store_ingested(&s, id, parts)?;
    s.db.log(id, "source.ingest", filename);
    ok(json!({ "sources": created }))
}

/// Profile and persist everything one upload produced (a workbook yields one
/// source per sheet).
fn store_ingested(
    s: &AppState,
    pid: i64,
    parts: Vec<ingest::Ingested>,
) -> Result<Vec<Value>, ApiErr> {
    let mut out = Vec::new();
    for p in parts {
        let table = prof::parse(&p.kind, &p.content)?;
        let columns = json!(prof::profile(&table));
        let sid = s.db.add_source(
            pid,
            &p.name,
            &p.kind,
            &p.content,
            &columns,
            table.rows.len() as i64,
            &p.origin,
            &p.note,
        )?;
        out.push(json!({
            "id": sid, "name": p.name, "kind": p.kind, "origin": p.origin, "note": p.note,
            "rowCount": table.rows.len(), "columns": columns,
        }));
    }
    if out.is_empty() {
        return Err(ApiErr("nothing usable was found in that file".into()));
    }
    Ok(out)
}

async fn formats() -> R {
    ok(json!({ "extensions": ingest::SUPPORTED }))
}

// ---- AIP Logic: functions, proposals, evals --------------------------------

async fn list_functions(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    ok(json!({
        "functions": s.db.list_functions(id)?,
        "proposalCounts": s.db.proposal_counts(id)?,
    }))
}

async fn create_function(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let name = b["name"].as_str().unwrap_or("").trim();
    if name.is_empty() {
        return Err(ApiErr("name is required".into()));
    }
    let kind = match b["kind"].as_str().unwrap_or("extract") {
        "classify" => "classify",
        "resolve" => "resolve",
        _ => "extract",
    };
    let input_kind = match kind {
        "classify" => "source",
        "resolve" => "class",
        _ => "text",
    };
    let fid = s.db.create_function(
        id,
        name,
        kind,
        input_kind,
        b["target"].as_str().unwrap_or(""),
        b["instruction"].as_str().unwrap_or(""),
        b["autoApply"].as_bool().unwrap_or(false),
    )?;
    ok(json!({ "id": fid }))
}

async fn delete_function(State(s): State<Arc<AppState>>, Path((_id, fid)): Path<(i64, i64)>) -> R {
    s.db.delete_function(fid)?;
    ok(json!({ "ok": true }))
}

async fn run_function(State(s): State<Arc<AppState>>, Path((id, fid)): Path<(i64, i64)>) -> R {
    ok(serde_json::to_value(
        logic::run(&s, id, fid, false).await.map_err(ApiErr)?,
    )?)
}

async fn trial_function(State(s): State<Arc<AppState>>, Path((id, fid)): Path<(i64, i64)>) -> R {
    ok(serde_json::to_value(
        logic::run(&s, id, fid, true).await.map_err(ApiErr)?,
    )?)
}

async fn list_proposals(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(q): Query<StatusQ>,
) -> R {
    ok(json!({
        "proposals": s.db.list_proposals(id, q.status.as_deref())?,
        "counts": s.db.proposal_counts(id)?,
    }))
}

#[derive(serde::Deserialize)]
struct StatusQ {
    #[serde(default)]
    status: Option<String>,
}

fn id_list(b: &Value) -> Vec<i64> {
    b["ids"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
        .unwrap_or_default()
}

async fn approve_proposals(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    ok(logic::approve(&s, id, &id_list(&b)).await.map_err(ApiErr)?)
}

async fn reject_proposals(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let n = s.db.reject_proposals(id, &id_list(&b))?;
    ok(json!({ "rejected": n }))
}

async fn list_evals(State(s): State<Arc<AppState>>, Path((_id, fid)): Path<(i64, i64)>) -> R {
    let cases: Vec<Value> =
        s.db.list_eval_cases(fid)?
            .into_iter()
            .map(|(id, input, expect)| json!({ "id": id, "input": input, "expect": expect }))
            .collect();
    ok(json!(cases))
}

async fn add_eval(
    State(s): State<Arc<AppState>>,
    Path((_id, fid)): Path<(i64, i64)>,
    Json(b): Json<Value>,
) -> R {
    let input = b["input"].as_str().unwrap_or("").trim();
    if input.is_empty() {
        return Err(ApiErr("input is required".into()));
    }
    let cid =
        s.db.add_eval_case(fid, input, b["expect"].as_str().unwrap_or(""))?;
    ok(json!({ "id": cid }))
}

async fn run_evals(
    State(s): State<Arc<AppState>>,
    Path((id, fid)): Path<(i64, i64)>,
    Json(b): Json<Value>,
) -> R {
    let profiles: Vec<String> = b["profiles"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    ok(logic::run_evals(&s, id, fid, &profiles)
        .await
        .map_err(ApiErr)?)
}

// ---- LLM model selection (per-app, does NOT touch the daemon active model) --

/// Configured LLMs in SenClaw, for the settings picker.
async fn list_models(State(_s): State<Arc<AppState>>) -> R {
    ok(llm::list_models().await.map_err(ApiErr)?)
}

/// Current app settings. `llmProfile` is "" when following the daemon's active
/// model; the resolved value is echoed back so the UI can show what is in force.
async fn get_settings(State(s): State<Arc<AppState>>) -> R {
    ok(json!({
        "llmProfile": s.db.get_setting("llm_profile", ""),
        "resolved": llm::profile().unwrap_or_default(),
    }))
}

/// Update settings. Setting `llmProfile` picks the model THIS app runs on — it
/// never changes the daemon's active model, which every other app and the agent
/// share. Empty string = follow the active model again.
async fn put_settings(State(s): State<Arc<AppState>>, Json(b): Json<Value>) -> R {
    if let Some(p) = b.get("llmProfile").and_then(|v| v.as_str()) {
        let p = p.trim();
        s.db.set_setting("llm_profile", p)?;
        llm::set_profile(p);
    }
    ok(json!({ "ok": true, "llmProfile": s.db.get_setting("llm_profile", "") }))
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
    let (_name, kind, content) =
        s.db.get_source(sid)?
            .ok_or_else(|| ApiErr("source not found".into()))?;
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

async fn add_property(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let (base, pfx) = s.ctx(id)?;
    let def: tbox::PropertyDef = serde_json::from_value(b)?;
    let g = s.graph_for(id)?;
    let iri = tbox::add_property(&g, &base, &pfx, &def)?;
    s.persist(id, &g)?;
    ok(json!({ "iri": iri }))
}

async fn apply_tbox(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
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

async fn remove_term(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let (base, pfx) = s.ctx(id)?;
    let iri = b["iri"].as_str().unwrap_or("");
    let g = s.graph_for(id)?;
    tbox::remove_term(&g, &base, &pfx, iri)?;
    s.persist(id, &g)?;
    ok(json!({ "ok": true }))
}

async fn draft_tbox(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
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

async fn set_mapping(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let m = b.get("mapping").cloned().unwrap_or(b);
    // Validate it parses as the DSL before saving.
    let _: mapping::Mapping =
        serde_json::from_value(m.clone()).map_err(|e| ApiErr(format!("invalid mapping: {e}")))?;
    s.db.set_mapping(id, &m)?;
    ok(json!({ "ok": true }))
}

/// Resolve the tables referenced by a mapping from the project's sources.
fn resolve_sources(
    s: &AppState,
    pid: i64,
    m: &mapping::Mapping,
) -> Result<HashMap<String, prof::Table>> {
    let mut out = HashMap::new();
    for tm in &m.triples_maps {
        if out.contains_key(&tm.source) {
            continue;
        }
        let (kind, content) =
            s.db.source_by_name(pid, &tm.source)?
                .ok_or_else(|| anyhow::anyhow!("source '{}' not uploaded", tm.source))?;
        out.insert(tm.source.clone(), prof::parse(&kind, &content)?);
    }
    Ok(out)
}

fn mapping_from(s: &AppState, pid: i64, b: &Value) -> Result<mapping::Mapping> {
    let raw = b
        .get("mapping")
        .cloned()
        .filter(|v| !v.is_null())
        .unwrap_or(s.db.get_mapping(pid)?);
    Ok(serde_json::from_value(raw)?)
}

async fn preview_mapping(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let (base, _pfx) = s.ctx(id)?;
    let m = mapping_from(&s, id, &b)?;
    let sources = resolve_sources(&s, id, &m)?;
    let rep = mapping::preview(&m, &sources, &base, 25)?;
    ok(json!({
        "triples": rep.triples, "subjects": rep.subjects, "skippedRows": rep.skipped_rows,
        "samples": rep.samples.iter().map(|(a,b,c)| json!([a,b,c])).collect::<Vec<_>>(),
    }))
}

async fn lift_mapping(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let (base, _pfx) = s.ctx(id)?;
    let m = mapping_from(&s, id, &b)?;
    let sources = resolve_sources(&s, id, &m)?;
    let g = s.graph_for(id)?;
    let ts = now();
    let label = m
        .triples_maps
        .first()
        .map(|t| t.source.clone())
        .unwrap_or_else(|| "import".into());
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

async fn draft_mapping(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let (base, _pfx) = s.ctx(id)?;
    let sources = s.db.list_sources(id)?;
    let sid = b["sourceId"].as_i64();
    let src = sources
        .iter()
        .find(|x| Some(x.id) == sid)
        .or_else(|| sources.first())
        .ok_or_else(|| ApiErr("upload a source first".into()))?;
    let g = s.graph_for(id)?;
    let tb = tbox::read(&g)?;
    let (draft, model) = llm::draft_mapping(&src.columns, &tb, &src.name, &base)
        .await
        .map_err(ApiErr)?;
    ok(json!({ "mapping": draft, "model": model }))
}

// ---- SPARQL + graph viz ---------------------------------------------------

async fn run_sparql(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
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
    let live = tbox::live_schema(&g)?;
    let (sparql, model) = llm::nl_to_sparql(question, &tb, &live)
        .await
        .map_err(ApiErr)?;
    ok(json!({ "sparql": sparql, "model": model }))
}

/// The classes and predicates the data actually uses (as opposed to the ones
/// the T-Box declares) — the grounding the NL→SPARQL step runs on.
async fn live_schema(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let g = s.graph_for(id)?;
    ok(tbox::live_schema(&g)?)
}

// ---- one-click autobuild + ask --------------------------------------------

async fn autobuild(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    if s.db.get_project(id)?.is_none() {
        return Err(ApiErr("project not found".into()));
    }
    let opts: auto::AutoOpts = serde_json::from_value(b).unwrap_or_default();
    let job_id = auto::start(s.clone(), id, opts);
    ok(json!({ "jobId": job_id }))
}

async fn autobuild_status(
    State(s): State<Arc<AppState>>,
    Path((_id, job)): Path<(i64, String)>,
) -> R {
    let handle = s.jobs.lock().unwrap().get(&job).cloned();
    match handle {
        Some(h) => {
            let snapshot = h.lock().unwrap().clone();
            ok(serde_json::to_value(snapshot)?)
        }
        None => Err(ApiErr("no such job".into())),
    }
}

async fn ask(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let question = b["question"].as_str().unwrap_or("");
    ok(auto::ask(&s, id, question).await.map_err(ApiErr)?)
}

// ---- AIP Assist: context-aware RAG over metadata ---------------------------

async fn assist(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let question = b["question"].as_str().unwrap_or("");
    let ctx: aip::Context = serde_json::from_value(b["context"].clone()).unwrap_or_default();
    ok(aip::assist(&s, id, question, &ctx).await.map_err(ApiErr)?)
}

/// The retrievable index itself. Exposed so the "metadata, never data" claim is
/// inspectable rather than something the user has to take on trust.
async fn assist_index(State(s): State<Arc<AppState>>, Path(id): Path<i64>) -> R {
    let docs = aip::build_index(&s, id)?;
    ok(json!({ "count": docs.len(), "documents": docs }))
}

#[derive(serde::Deserialize)]
struct LimitQ {
    #[serde(default)]
    limit: Option<usize>,
}

/// A-Box sample as nodes/edges for the explorer.
async fn data_graph(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(q): Query<LimitQ>,
) -> R {
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
            nodes
                .entry(s.to_string())
                .or_insert_with(|| json!({ "id": s, "label": short(s), "kind": "iri" }));
            if p.ends_with("22-rdf-syntax-ns#type") {
                // fold rdf:type into a node attribute rather than an edge.
                if let Some(n) = nodes.get_mut(s) {
                    n["type"] = json!(short(oval));
                }
                nodes.entry(oval.to_string()).or_insert_with(
                    || json!({ "id": oval, "label": short(oval), "kind": "class" }),
                );
                continue;
            }
            if otype == "uri" {
                nodes
                    .entry(oval.to_string())
                    .or_insert_with(|| json!({ "id": oval, "label": short(oval), "kind": "iri" }));
                edges.push(json!({ "source": s, "target": oval, "label": short(p) }));
            } else {
                let lit_id = format!("lit:{s}:{p}:{oval}");
                nodes.insert(
                    lit_id.clone(),
                    json!({ "id": lit_id, "label": oval, "kind": "literal" }),
                );
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

async fn update_cq(
    State(s): State<Arc<AppState>>,
    Path((_id, cid)): Path<(i64, i64)>,
    Json(b): Json<Value>,
) -> R {
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

async fn set_shapes(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let shapes = b.get("shapes").cloned().unwrap_or(b);
    let _: shacl::Shapes = serde_json::from_value(shapes.clone())
        .map_err(|e| ApiErr(format!("invalid shapes: {e}")))?;
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
    let raw = b
        .get("shapes")
        .cloned()
        .filter(|v| !v.is_null())
        .unwrap_or(s.db.get_shapes(id)?);
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

async fn resolve_candidates(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let (base, pfx) = s.ctx(id)?;
    let g = s.graph_for(id)?;
    let class = b["class"].as_str().unwrap_or("");
    let label_prop = b["labelProp"].as_str().unwrap_or("rdfs:label");
    let threshold = b["threshold"].as_f64().unwrap_or(0.9);
    ok(resolve::candidates(
        &g, &base, &pfx, class, label_prop, threshold,
    )?)
}

async fn resolve_apply(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
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

async fn drop_batch(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<Value>,
) -> R {
    let iri = b["iri"].as_str().unwrap_or("");
    let g = s.graph_for(id)?;
    prov::drop_batch(&g, iri)?;
    s.persist(id, &g)?;
    // Dropping a batch undoes an approval — the proposals that fed it go back to
    // pending so the audit trail stays honest (nothing is "approved" once its
    // triples are gone).
    let reverted = s.db.revert_proposals_for_batch(id, iri)?;
    ok(json!({ "ok": true, "revertedProposals": reverted }))
}

// ---- unstructured extraction ----------------------------------------------

/// Extract triples from unstructured text. Long documents are chunked — the
/// bridge returns a roughly fixed amount of output per call, so a whole report
/// in one prompt comes back summarized (i.e. most facts silently dropped)
/// rather than fully extracted.
async fn extract(State(s): State<Arc<AppState>>, Path(id): Path<i64>, Json(b): Json<Value>) -> R {
    let (base, pfx) = s.ctx(id)?;
    let text = b["text"].as_str().unwrap_or("");
    if text.trim().is_empty() {
        return Err(ApiErr("text is required".into()));
    }
    let label = b["label"].as_str().unwrap_or("text extraction");
    let max_chunks = b["maxChunks"].as_u64().unwrap_or(12) as usize;
    let g = s.graph_for(id)?;
    let tb = tbox::read(&g)?;
    let (triples, model, chunks, errors) = llm::extract_triples_chunked(text, &tb, max_chunks)
        .await
        .map_err(ApiErr)?;
    let inserted =
        auto::insert_extracted(&s, id, &g, label, &triples, &base, &pfx).map_err(ApiErr)?;
    ok(json!({
        "inserted": inserted, "model": model, "chunks": chunks,
        "errors": errors, "triples": triples,
    }))
}
