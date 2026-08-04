//! **Autobuild** and **Ask** — the two shortcuts that turn a seven-tab pipeline
//! into two buttons.
//!
//! * [`start`] runs the whole discipline for you: profile → competency
//!   questions → T-Box → mapping → lift → extract from text → SHACL shapes →
//!   validate → answer the competency suite → reason. Each stage is still the
//!   same deterministic module the manual tabs call; the LLM only ever produces
//!   *drafts*, and every draft is checked (and mechanically repaired) before it
//!   touches the store. It runs as a background job because a full build is a
//!   dozen bridge round-trips — far past any HTTP timeout.
//!
//! * [`ask`] closes the loop at the other end: a question in, a sentence out,
//!   with the SPARQL and the rows attached so the answer stays auditable.
//!
//! Nothing here holds a `Mutex` across an `.await` — the DB and graph guards are
//! taken, used, and dropped inside each helper, which is what keeps the whole
//! job `Send` (and what keeps the graphs→db lock order from ever inverting).

use crate::api::AppState;
use crate::db::now;
use crate::graph::Graph;
use crate::{llm, mapping, profile as prof, prov, reason, shacl, tbox, vocab};
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// job model
// ---------------------------------------------------------------------------

#[derive(Clone, serde::Serialize)]
pub struct Step {
    pub key: String,
    pub label: String,
    /// `pending | running | ok | warn | error | skipped`
    pub status: String,
    pub detail: String,
}

impl Step {
    fn new(key: &str, label: &str) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            status: "pending".into(),
            detail: String::new(),
        }
    }
}

#[derive(Clone, serde::Serialize)]
pub struct Job {
    pub id: String,
    #[serde(rename = "projectId")]
    pub project_id: i64,
    pub steps: Vec<Step>,
    pub done: bool,
    pub error: Option<String>,
    pub result: Value,
    #[serde(rename = "startedAt")]
    pub started_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

pub type JobHandle = Arc<Mutex<Job>>;

static JOB_SEQ: AtomicU64 = AtomicU64::new(1);

const STEPS: &[(&str, &str)] = &[
    ("profile", "Profile sources"),
    ("competency", "Draft competency questions"),
    ("tbox", "Design the ontology (T-Box)"),
    ("mapping", "Author & repair the mapping"),
    ("lift", "Lift rows into the graph"),
    ("extract", "Extract triples from text"),
    ("shapes", "Draft SHACL shapes & validate"),
    ("answer", "Answer the competency suite"),
    ("reason", "Materialize inferences"),
];

/// Tunables for one run. Defaults are "do everything".
#[derive(serde::Deserialize, Clone)]
pub struct AutoOpts {
    /// Run RDFS/OWL-RL materialization at the end.
    #[serde(default = "yes")]
    pub reason: bool,
    /// Run LLM extraction over unstructured (`text`) sources.
    #[serde(default = "yes")]
    pub extract: bool,
    /// Cap on extraction chunks per text source — the cost control knob.
    #[serde(default = "default_chunks", rename = "maxChunks")]
    pub max_chunks: usize,
}

fn yes() -> bool {
    true
}
fn default_chunks() -> usize {
    12
}

impl Default for AutoOpts {
    fn default() -> Self {
        Self {
            reason: true,
            extract: true,
            max_chunks: default_chunks(),
        }
    }
}

fn set(job: &JobHandle, key: &str, status: &str, detail: impl Into<String>) {
    let mut j = job.lock().unwrap();
    j.updated_at = now();
    if let Some(s) = j.steps.iter_mut().find(|s| s.key == key) {
        s.status = status.into();
        s.detail = detail.into();
    }
}

fn merge_result(job: &JobHandle, key: &str, v: Value) {
    let mut j = job.lock().unwrap();
    j.updated_at = now();
    if let Some(o) = j.result.as_object_mut() {
        o.insert(key.to_string(), v);
    }
}

/// Register a job and spawn the pipeline. Returns immediately with the job id;
/// poll [`AppState::job`] for progress.
pub fn start(state: Arc<AppState>, pid: i64, opts: AutoOpts) -> String {
    let id = format!("ab-{}-{}", now(), JOB_SEQ.fetch_add(1, Ordering::Relaxed));
    let job: JobHandle = Arc::new(Mutex::new(Job {
        id: id.clone(),
        project_id: pid,
        steps: STEPS.iter().map(|(k, l)| Step::new(k, l)).collect(),
        done: false,
        error: None,
        result: json!({}),
        started_at: now(),
        updated_at: now(),
    }));
    state.jobs.lock().unwrap().insert(id.clone(), job.clone());
    tokio::spawn(async move {
        let outcome = run(&state, pid, &job, &opts).await;
        let mut j = job.lock().unwrap();
        j.done = true;
        j.updated_at = now();
        if let Err(e) = outcome {
            j.error = Some(e);
        }
    });
    id
}

// ---------------------------------------------------------------------------
// the pipeline
// ---------------------------------------------------------------------------

async fn run(
    state: &Arc<AppState>,
    pid: i64,
    job: &JobHandle,
    opts: &AutoOpts,
) -> Result<(), String> {
    let (base, _pfx) = state.ctx(pid).map_err(|e| e.to_string())?;

    // ---- [1] profile every source -----------------------------------------
    set(job, "profile", "running", "");
    let sources = state.db.list_sources(pid).map_err(|e| e.to_string())?;
    if sources.is_empty() {
        set(job, "profile", "error", "no sources — upload a file first");
        return Err("no sources in this project — upload a file first".into());
    }
    let mut tabular: Vec<(String, Value)> = Vec::new(); // (name, columns)
    let mut texts: Vec<(String, String)> = Vec::new(); // (name, content)
    let mut summary: Vec<Value> = Vec::new();
    for s in &sources {
        let Ok(Some((name, kind, content))) = state.db.get_source(s.id) else {
            continue;
        };
        match prof::parse(&kind, &content) {
            Ok(table) => {
                let columns = json!(prof::profile(&table));
                let _ = state.db.set_source_columns(s.id, &columns);
                if kind == "text" {
                    texts.push((name.clone(), content));
                    summary.push(json!({ "source": name, "kind": "text", "origin": s.origin,
                                         "characters": s.row_count }));
                } else {
                    summary.push(json!({ "source": name, "kind": kind, "origin": s.origin,
                                         "rowCount": table.rows.len(), "columns": trim_columns(&columns) }));
                    tabular.push((name.clone(), columns));
                }
            }
            Err(e) => summary.push(json!({ "source": name, "error": e.to_string() })),
        }
    }
    set(
        job,
        "profile",
        "ok",
        format!(
            "{} tabular source(s), {} text source(s)",
            tabular.len(),
            texts.len()
        ),
    );
    merge_result(job, "sources", json!(summary));
    let sources_json = json!(summary);

    // ---- [2] competency questions ------------------------------------------
    set(job, "competency", "running", "");
    let mut cqs: Vec<String> = state
        .db
        .list_cq(pid)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|c| c.question)
        .collect();
    if cqs.is_empty() {
        match llm::draft_competency(&sources_json).await {
            Ok((qs, _model)) if !qs.is_empty() => {
                for q in &qs {
                    let _ = state.db.add_cq(pid, q, "", "nonempty");
                }
                cqs = qs;
                set(
                    job,
                    "competency",
                    "ok",
                    format!("{} question(s) drafted", cqs.len()),
                );
            }
            Ok(_) => set(job, "competency", "warn", "the model returned no questions"),
            Err(e) => set(job, "competency", "warn", format!("skipped: {e}")),
        }
    } else {
        set(
            job,
            "competency",
            "ok",
            format!("{} existing question(s) kept", cqs.len()),
        );
    }

    // ---- [3] T-Box ---------------------------------------------------------
    set(job, "tbox", "running", "");
    let g = state.graph_for(pid).map_err(|e| e.to_string())?;
    let (draft, _model) = llm::draft_tbox(&cqs, &sources_json, &base)
        .await
        .inspect_err(|e| {
            set(job, "tbox", "error", e.clone());
        })?;
    let (nc, np) = apply_tbox(state, pid, &g, &draft).inspect_err(|e| {
        set(job, "tbox", "error", e.clone());
    })?;
    set(
        job,
        "tbox",
        "ok",
        format!("{nc} class(es), {np} propert(ies)"),
    );
    merge_result(job, "tbox", json!({ "classes": nc, "properties": np }));
    let (_base2, pfx) = state.ctx(pid).map_err(|e| e.to_string())?;
    let tb = tbox::read(&g).map_err(|e| e.to_string())?;

    // ---- [4] mapping (drafted per source, then mechanically repaired) -------
    set(job, "mapping", "running", "");
    let known: HashSet<String> = tabular.iter().map(|(n, _)| n.clone()).collect();
    let mut triples_maps: Vec<Value> = Vec::new();
    let mut repairs: Vec<String> = Vec::new();
    let mut map_prefixes: Map<String, Value> = Map::new();
    for (name, columns) in &tabular {
        match llm::draft_mapping(columns, &tb, name, &base).await {
            Ok((m, _)) => {
                if let Some(o) = m["prefixes"].as_object() {
                    for (k, v) in o {
                        map_prefixes.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                }
                let cols = column_names(columns);
                if let Some(arr) = m["triplesMaps"].as_array() {
                    for tm in arr {
                        if let Some(fixed) =
                            repair_triples_map(tm, name, &cols, &known, &mut repairs)
                        {
                            triples_maps.push(fixed);
                        }
                    }
                }
            }
            Err(e) => repairs.push(format!("{name}: draft failed ({e})")),
        }
    }
    if triples_maps.is_empty() && !tabular.is_empty() {
        set(
            job,
            "mapping",
            "error",
            "no usable triples map was produced",
        );
        return Err(format!("mapping draft unusable: {}", repairs.join("; ")));
    }
    let full_mapping =
        json!({ "base": base, "prefixes": map_prefixes, "triplesMaps": triples_maps });
    if !tabular.is_empty() {
        // Reject anything the DSL cannot parse before it is ever persisted.
        serde_json::from_value::<mapping::Mapping>(full_mapping.clone())
            .map_err(|e| format!("drafted mapping is invalid: {e}"))?;
        state
            .db
            .set_mapping(pid, &full_mapping)
            .map_err(|e| e.to_string())?;
        // Keep the step line short — the full repair list travels in
        // `result.repairs`, which the UI shows behind a disclosure.
        set(
            job,
            "mapping",
            if repairs.is_empty() { "ok" } else { "warn" },
            if repairs.is_empty() {
                format!("{} triples map(s)", triples_maps.len())
            } else {
                format!(
                    "{} triples map(s) · {} repair(s) applied",
                    triples_maps.len(),
                    repairs.len()
                )
            },
        );
    } else {
        set(job, "mapping", "skipped", "no tabular source");
    }
    merge_result(job, "repairs", json!(repairs));

    // ---- [5] lift ----------------------------------------------------------
    if tabular.is_empty() {
        set(job, "lift", "skipped", "no tabular source");
    } else {
        set(job, "lift", "running", "");
        match lift(state, pid, &g, &full_mapping, &base) {
            Ok(rep) => {
                set(
                    job,
                    "lift",
                    "ok",
                    format!(
                        "{} triples · {} subjects{}",
                        rep["triples"],
                        rep["subjects"],
                        match rep["skippedRows"].as_u64() {
                            Some(n) if n > 0 => format!(" · {n} row(s) skipped"),
                            _ => String::new(),
                        }
                    ),
                );
                merge_result(job, "lift", rep);
            }
            Err(e) => {
                set(job, "lift", "error", e.clone());
                return Err(e);
            }
        }
    }

    // ---- [6] unstructured extraction ---------------------------------------
    if texts.is_empty() {
        set(job, "extract", "skipped", "no text source");
    } else if !opts.extract {
        set(job, "extract", "skipped", "disabled for this run");
    } else {
        set(
            job,
            "extract",
            "running",
            format!("{} document(s)", texts.len()),
        );
        let tb_now = tbox::read(&g).map_err(|e| e.to_string())?;
        let mut inserted = 0usize;
        let mut notes: Vec<String> = Vec::new();
        for (name, content) in &texts {
            match llm::extract_triples_chunked(content, &tb_now, opts.max_chunks).await {
                Ok((triples, _model, chunks, errs)) => {
                    match insert_extracted(state, pid, &g, name, &triples, &base, &pfx) {
                        Ok(n) => {
                            inserted += n;
                            notes.push(format!("{name}: {n} triple(s) from {chunks} chunk(s)"));
                        }
                        Err(e) => notes.push(format!("{name}: {e}")),
                    }
                    if !errs.is_empty() {
                        notes.push(format!("{name}: {} chunk error(s)", errs.len()));
                    }
                }
                Err(e) => notes.push(format!("{name}: {e}")),
            }
        }
        set(
            job,
            "extract",
            if inserted > 0 { "ok" } else { "warn" },
            notes.join(" · "),
        );
        merge_result(job, "extracted", json!(inserted));
    }

    // ---- [7] shapes + validation -------------------------------------------
    set(job, "shapes", "running", "");
    let tb_final = tbox::read(&g).map_err(|e| e.to_string())?;
    match llm::draft_shapes(&tb_final).await {
        Ok((shapes, _)) => match serde_json::from_value::<shacl::Shapes>(shapes.clone()) {
            Ok(parsed) => {
                let _ = state.db.set_shapes(pid, &shapes);
                match shacl::validate(&g, &base, &pfx, &parsed) {
                    Ok(report) => {
                        let conforms = report["conforms"].as_bool().unwrap_or(false);
                        let n = report["violationCount"].as_u64().unwrap_or(0);
                        set(
                            job,
                            "shapes",
                            if conforms { "ok" } else { "warn" },
                            if conforms {
                                "data conforms".into()
                            } else {
                                format!("{n} violation(s)")
                            },
                        );
                        merge_result(job, "validation", report);
                    }
                    Err(e) => set(job, "shapes", "warn", format!("validation failed: {e}")),
                }
            }
            Err(e) => set(
                job,
                "shapes",
                "warn",
                format!("drafted shapes invalid: {e}"),
            ),
        },
        Err(e) => set(job, "shapes", "warn", format!("skipped: {e}")),
    }

    // ---- [8] answer the competency questions -------------------------------
    set(job, "answer", "running", "");
    let live = tbox::live_schema(&g).map_err(|e| e.to_string())?;
    let pending = state.db.list_cq(pid).map_err(|e| e.to_string())?;
    let mut written = 0usize;
    for cq in pending.iter().filter(|c| c.sparql.trim().is_empty()) {
        if let Ok((q, _)) = llm::nl_to_sparql(&cq.question, &tb_final, &live).await {
            if !q.trim().is_empty() {
                let _ = state.db.update_cq(cq.id, &cq.question, &q, &cq.expect);
                written += 1;
            }
        }
    }
    let suite = run_suite(state, pid, &g, &pfx).map_err(|e| e.to_string())?;
    let passed = suite["passed"].as_u64().unwrap_or(0);
    let total = suite["total"].as_u64().unwrap_or(0);
    set(
        job,
        "answer",
        if total > 0 && passed == total {
            "ok"
        } else {
            "warn"
        },
        format!("{passed}/{total} competency question(s) pass ({written} query/queries written)"),
    );
    merge_result(job, "competency", suite);

    // ---- [9] reasoning -----------------------------------------------------
    if opts.reason {
        set(job, "reason", "running", "");
        match reason::materialize(&g) {
            Ok(rep) => {
                let _ = state.persist(pid, &g);
                set(
                    job,
                    "reason",
                    "ok",
                    format!("{} inferred triple(s)", rep["inferred"]),
                );
                merge_result(job, "reason", rep);
            }
            Err(e) => set(job, "reason", "warn", e.to_string()),
        }
    } else {
        set(job, "reason", "skipped", "disabled for this run");
    }

    merge_result(job, "tripleCount", json!(g.len()));
    state.db.log(pid, "autobuild", "completed");
    Ok(())
}

// ---------------------------------------------------------------------------
// deterministic mapping repair
// ---------------------------------------------------------------------------

fn column_names(columns: &Value) -> HashSet<String> {
    columns
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| c["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Trim a column profile down to what the LLM actually needs, so a 200-column
/// source does not blow the prompt budget.
fn trim_columns(columns: &Value) -> Value {
    let arr = columns.as_array().cloned().unwrap_or_default();
    Value::Array(
        arr.into_iter()
            .take(80)
            .map(|c| {
                json!({
                    "name": c["name"], "datatype": c["datatype"], "role": c["role"],
                    "isUnique": c["isUnique"], "isEnum": c["isEnum"], "nullRatio": c["nullRatio"],
                    "samples": c["samples"].as_array().map(|s| s.iter().take(3).cloned().collect::<Vec<_>>()).unwrap_or_default(),
                })
            })
            .collect(),
    )
}

/// `{a}/{b}` → `["a","b"]`.
fn template_columns(t: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = t;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push(rest[open + 1..open + close].trim().to_string());
        rest = &rest[open + close + 1..];
    }
    out
}

/// Make one drafted triples map safe to run. The model hallucinates columns —
/// this is the deterministic gate that keeps a hallucination from becoming a
/// silently-empty lift (an unknown `{sku}` template renders literally and mints
/// one nonsense IRI per row) or a hard error mid-lift.
///
/// Returns `None` when nothing usable survives.
fn repair_triples_map(
    tm: &Value,
    source: &str,
    cols: &HashSet<String>,
    known_sources: &HashSet<String>,
    log: &mut Vec<String>,
) -> Option<Value> {
    let mut out = tm.as_object()?.clone();

    // The source must be one we actually hold; a drafted name that does not
    // exist is re-pointed at the source this draft was made for.
    let named = out
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if named != source {
        if !known_sources.contains(&named) {
            log.push(format!("{source}: source '{named}' unknown → repointed"));
        }
        out.insert("source".into(), json!(source));
    }

    // Subject: an unresolvable template becomes a stable hash of the row's
    // identifying columns, which is the DSL's own answer to "no natural key".
    let subject = out
        .get("subject")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default();
    let mut subject = subject;
    if let Some(t) = subject
        .get("template")
        .and_then(|v| v.as_str())
        .map(str::to_string)
    {
        let missing: Vec<String> = template_columns(&t)
            .into_iter()
            .filter(|c| !cols.contains(c))
            .collect();
        if !missing.is_empty() || template_columns(&t).is_empty() {
            let seg = subject
                .get("class")
                .and_then(|v| v.as_str())
                .map(local_name)
                .unwrap_or_else(|| source.to_string());
            let hash: Vec<String> = cols.iter().take(6).cloned().collect();
            if hash.is_empty() {
                return None;
            }
            subject.remove("template");
            subject.insert("hash".into(), json!(hash));
            subject.insert("seg".into(), json!(seg.to_lowercase()));
            log.push(format!(
                "{source}: subject template '{t}' → hashed IRI (missing {})",
                missing.join(", ")
            ));
        }
    } else if let Some(h) = subject.get("hash").and_then(|v| v.as_array()) {
        let kept: Vec<Value> = h
            .iter()
            .filter(|c| c.as_str().map(|s| cols.contains(s)).unwrap_or(false))
            .cloned()
            .collect();
        if kept.is_empty() {
            subject.insert(
                "hash".into(),
                json!(cols.iter().take(6).cloned().collect::<Vec<_>>()),
            );
            log.push(format!(
                "{source}: subject hash columns unknown → hashing all columns"
            ));
        } else if kept.len() != h.len() {
            subject.insert("hash".into(), Value::Array(kept));
        }
    } else {
        // Neither form given: hash whatever we have.
        subject.insert(
            "hash".into(),
            json!(cols.iter().take(6).cloned().collect::<Vec<_>>()),
        );
    }
    out.insert("subject".into(), Value::Object(subject));

    // Predicate-object maps: drop anything that references a column we do not
    // have. Dropping is right — a bad triple is worse than a missing one.
    let mut poms: Vec<Value> = Vec::new();
    if let Some(arr) = out.get("predicateObjectMaps").and_then(|v| v.as_array()) {
        for pom in arr {
            let pred = pom["predicate"].as_str().unwrap_or("").trim();
            if pred.is_empty() {
                continue;
            }
            let obj = &pom["object"];
            let ok = if let Some(c) = obj["column"].as_str() {
                let good = cols.contains(c);
                if !good {
                    log.push(format!("{source}: dropped {pred} (no column '{c}')"));
                }
                good
            } else if let Some(t) = obj["template"].as_str() {
                let missing: Vec<String> = template_columns(t)
                    .into_iter()
                    .filter(|c| !cols.contains(c))
                    .collect();
                if !missing.is_empty() {
                    log.push(format!(
                        "{source}: dropped {pred} (template needs {})",
                        missing.join(", ")
                    ));
                }
                missing.is_empty()
            } else if let Some(ph) = obj["parentHash"].as_array() {
                let missing: Vec<String> = ph
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter(|c| !cols.contains(*c))
                    .map(str::to_string)
                    .collect();
                if !missing.is_empty() {
                    log.push(format!(
                        "{source}: dropped {pred} (parentHash needs {})",
                        missing.join(", ")
                    ));
                }
                missing.is_empty()
            } else {
                obj["constant"].is_string()
            };
            if ok {
                poms.push(pom.clone());
            }
        }
    }
    let has_class = out
        .get("subject")
        .and_then(|s| s.get("class"))
        .and_then(|c| c.as_str())
        .is_some_and(|c| !c.trim().is_empty());
    if poms.is_empty() && !has_class {
        log.push(format!(
            "{source}: triples map dropped (nothing usable left)"
        ));
        return None;
    }
    out.insert("predicateObjectMaps".into(), Value::Array(poms));
    Some(Value::Object(out))
}

fn local_name(iri: &str) -> String {
    iri.rsplit(['#', '/', ':'])
        .next()
        .unwrap_or(iri)
        .to_string()
}

// ---------------------------------------------------------------------------
// shared building blocks (also used by the REST handlers)
// ---------------------------------------------------------------------------

fn apply_tbox(
    state: &Arc<AppState>,
    pid: i64,
    g: &Graph,
    draft: &Value,
) -> Result<(usize, usize), String> {
    let (base, pfx) = state.ctx(pid).map_err(|e| e.to_string())?;
    let parsed: tbox::TboxDraft =
        serde_json::from_value(draft.clone()).map_err(|e| format!("invalid T-Box draft: {e}"))?;
    // Every field of TboxDraft is `#[serde(default)]`, so a reply of the wrong
    // shape deserializes cleanly into an *empty* schema. Without this check the
    // run would report "ok, 0 classes" and carry on building an A-Box with no
    // ontology behind it.
    if parsed.classes.is_empty() && parsed.properties.is_empty() {
        return Err(format!(
            "the T-Box draft contained no classes or properties (got keys: {})",
            draft
                .as_object()
                .map(|o| o.keys().cloned().collect::<Vec<_>>().join(", "))
                .unwrap_or_else(|| "not an object".into())
        ));
    }
    let (nc, np) = tbox::apply_draft(g, &base, &pfx, &parsed).map_err(|e| e.to_string())?;
    if !parsed.prefixes.is_empty() {
        let mut merged = pfx.clone();
        merged.extend(parsed.prefixes.clone());
        let _ = state.db.set_prefixes(pid, &json!(merged));
    }
    state.persist(pid, g).map_err(|e| e.to_string())?;
    Ok((nc, np))
}

fn lift(
    state: &Arc<AppState>,
    pid: i64,
    g: &Graph,
    m: &Value,
    base: &str,
) -> Result<Value, String> {
    let parsed: mapping::Mapping = serde_json::from_value(m.clone()).map_err(|e| e.to_string())?;
    let mut tables: HashMap<String, prof::Table> = HashMap::new();
    for tm in &parsed.triples_maps {
        if tables.contains_key(&tm.source) {
            continue;
        }
        let (kind, content) = state
            .db
            .source_by_name(pid, &tm.source)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("source '{}' not uploaded", tm.source))?;
        tables.insert(
            tm.source.clone(),
            prof::parse(&kind, &content).map_err(|e| e.to_string())?,
        );
    }
    let ts = now();
    let label = parsed
        .triples_maps
        .first()
        .map(|t| t.source.clone())
        .unwrap_or_else(|| "import".into());
    let batch = prov::batch_iri(ts, &label);
    let rep = mapping::lift(g, &parsed, &tables, base, &batch).map_err(|e| e.to_string())?;
    prov::record_batch(g, &batch, &label, &label, "autobuild", rep.triples, ts)
        .map_err(|e| e.to_string())?;
    state.persist(pid, g).map_err(|e| e.to_string())?;
    state.db.log(
        pid,
        "lift",
        &format!("{} triples into {batch}", rep.triples),
    );
    Ok(json!({
        "batch": batch, "triples": rep.triples, "subjects": rep.subjects,
        "skippedRows": rep.skipped_rows, "totalTriples": g.len(),
    }))
}

/// Write LLM-extracted triples into their own provenance batch. Shared with the
/// REST `/extract` handler so both paths mint IRIs and escape literals the same
/// way — the escaping here is the injection boundary for model-authored text.
pub fn insert_extracted(
    state: &Arc<AppState>,
    pid: i64,
    g: &Graph,
    label: &str,
    triples: &[Value],
    base: &str,
    pfx: &HashMap<String, String>,
) -> Result<usize, String> {
    let ts = now();
    let batch = prov::batch_iri(ts, &format!("extract-{label}"));
    let mut body = String::new();
    let mut inserted = 0usize;
    for t in triples {
        let s_raw = t["s"].as_str().unwrap_or("").trim();
        let p_raw = t["p"].as_str().unwrap_or("").trim();
        let o_raw = t["o"].as_str().unwrap_or("").trim();
        if s_raw.is_empty() || p_raw.is_empty() || o_raw.is_empty() {
            continue;
        }
        let subj = to_iri_or_mint(s_raw, base, pfx);
        let pred = vocab::expand(p_raw, pfx, base);
        let (Some(st), Some(pt)) = (vocab::iri_term(&subj), vocab::iri_term(&pred)) else {
            continue;
        };
        let obj = if t["oIsLiteral"].as_bool().unwrap_or(true) {
            format!("\"{}\"", vocab::escape_literal(o_raw))
        } else {
            match vocab::iri_term(&to_iri_or_mint(o_raw, base, pfx)) {
                Some(x) => x,
                None => continue,
            }
        };
        body.push_str(&format!("{st} {pt} {obj} .\n"));
        inserted += 1;
    }
    if inserted == 0 {
        return Ok(0);
    }
    g.update(&format!("INSERT DATA {{ GRAPH <{batch}> {{\n{body}}} }}"))
        .map_err(|e| e.to_string())?;
    prov::record_batch(
        g,
        &batch,
        label,
        "unstructured text (LLM)",
        "extract",
        inserted,
        ts,
    )
    .map_err(|e| e.to_string())?;
    state.persist(pid, g).map_err(|e| e.to_string())?;
    Ok(inserted)
}

/// An LLM-provided subject/object → IRI: expand a curie/IRI, else mint a stable
/// hashed IRI from the label so the same entity name always lands on the same
/// node across chunks and documents.
pub fn to_iri_or_mint(raw: &str, base: &str, pfx: &HashMap<String, String>) -> String {
    let r = raw.trim();
    if r.starts_with("http://")
        || r.starts_with("https://")
        || r.starts_with("urn:")
        || r.contains(':')
    {
        vocab::expand(r, pfx, base)
    } else {
        vocab::hashed_iri(base, "entity", &[r])
    }
}

/// Run the saved competency questions as a pass/fail suite.
pub fn run_suite(
    state: &Arc<AppState>,
    pid: i64,
    g: &Graph,
    pfx: &HashMap<String, String>,
) -> anyhow::Result<Value> {
    let cqs = state.db.list_cq(pid)?;
    let mut results = Vec::new();
    let mut passed = 0u64;
    for cq in &cqs {
        if cq.sparql.trim().is_empty() {
            results.push(json!({ "id": cq.id, "question": cq.question, "pass": false, "error": "no SPARQL" }));
            continue;
        }
        match g.query_json(&vocab::ensure_prefixes(&cq.sparql, pfx)) {
            Ok(res) => {
                let count = res["rows"]
                    .as_array()
                    .map(|a| a.len())
                    .or_else(|| res.get("boolean").map(|b| usize::from(b.as_bool().unwrap_or(false))))
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
            Err(e) => results.push(
                json!({ "id": cq.id, "question": cq.question, "pass": false, "error": e.to_string() }),
            ),
        }
    }
    Ok(json!({ "total": cqs.len(), "passed": passed, "results": results }))
}

// ---------------------------------------------------------------------------
// Ask
// ---------------------------------------------------------------------------

/// Question in, sentence out. The SPARQL and the rows come back too, so the
/// answer is always checkable — an LLM sentence over a graph is only worth
/// anything if you can see the query that produced it.
///
/// One self-repair round is allowed: a query that fails to parse, or that comes
/// back empty when the graph is not empty, is handed back to the model with the
/// exact problem.
pub async fn ask(state: &Arc<AppState>, pid: i64, question: &str) -> Result<Value, String> {
    if question.trim().is_empty() {
        return Err("question is required".into());
    }
    let (_base, pfx) = state.ctx(pid).map_err(|e| e.to_string())?;
    let g = state.graph_for(pid).map_err(|e| e.to_string())?;
    if g.is_empty() {
        return Err("this project has no triples yet — upload a source and run Auto-build".into());
    }
    let tb = tbox::read(&g).map_err(|e| e.to_string())?;
    let live = tbox::live_schema(&g).map_err(|e| e.to_string())?;

    let (mut sparql, model) = llm::nl_to_sparql(question, &tb, &live).await?;
    let mut repaired: Option<String> = None;
    let mut result = g.query_json(&vocab::ensure_prefixes(&sparql, &pfx));

    // Repair round: a parse error, or an empty answer over a non-empty graph.
    let problem = match &result {
        Err(e) => Some(e.to_string()),
        Ok(v) => {
            let empty = v["rows"].as_array().map(|a| a.is_empty()).unwrap_or(false);
            empty.then(|| "the query parsed but matched nothing".to_string())
        }
    };
    if let Some(problem) = problem {
        if let Ok((fixed, _)) = llm::repair_sparql(question, &live, &sparql, &problem).await {
            if !fixed.trim().is_empty() && fixed != sparql {
                if let Ok(v) = g.query_json(&vocab::ensure_prefixes(&fixed, &pfx)) {
                    let better = v["rows"].as_array().map(|a| !a.is_empty()).unwrap_or(true);
                    if better || result.is_err() {
                        repaired = Some(problem);
                        sparql = fixed;
                        result = Ok(v);
                    }
                }
            }
        }
    }

    let res = result.map_err(|e| format!("could not build a working query: {e}"))?;
    let rows = res.get("rows").cloned().unwrap_or_else(|| json!([]));
    let count = rows.as_array().map(|a| a.len()).unwrap_or(0);
    let is_ask = res.get("boolean").is_some();
    // Emptiness is decided here, not by the model. Handing zero rows to an LLM
    // and asking it to answer the question is an invitation to fabricate, and
    // it is the one failure mode we can rule out deterministically.
    let answer = if count == 0 && !is_ask {
        llm::no_data_answer(question).await
    } else {
        let payload = if is_ask { res.clone() } else { rows.clone() };
        llm::answer_from_rows(question, &payload, count)
            .await
            .map(|(a, _)| a)
            .unwrap_or_else(|e| format!("(no summary: {e})"))
    };

    Ok(json!({
        "question": question,
        "sparql": sparql,
        "repaired": repaired,
        "head": res.get("head").cloned().unwrap_or(json!([])),
        "rows": rows,
        "boolean": res.get("boolean").cloned(),
        "count": count,
        "answer": answer,
        "model": model,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(names: &[&str]) -> HashSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn template_columns_parsed() {
        assert_eq!(template_columns("product/{sku}/{lot}"), vec!["sku", "lot"]);
        assert!(template_columns("static/path").is_empty());
    }

    #[test]
    fn drops_poms_referencing_unknown_columns() {
        let tm = json!({
            "source": "products",
            "subject": { "template": "product/{sku}", "class": "ex:Product" },
            "predicateObjectMaps": [
                { "predicate": "rdfs:label", "object": { "column": "name" } },
                { "predicate": "ex:hasColour", "object": { "column": "colour" } }
            ]
        });
        let mut log = Vec::new();
        let fixed = repair_triples_map(
            &tm,
            "products",
            &cols(&["sku", "name"]),
            &cols(&["products"]),
            &mut log,
        )
        .unwrap();
        let poms = fixed["predicateObjectMaps"].as_array().unwrap();
        assert_eq!(poms.len(), 1);
        assert_eq!(poms[0]["predicate"], "rdfs:label");
        assert!(log.iter().any(|l| l.contains("colour")), "{log:?}");
    }

    #[test]
    fn hallucinated_subject_template_becomes_a_hash() {
        let tm = json!({
            "source": "rows",
            "subject": { "template": "thing/{id}", "class": "ex:Thing" },
            "predicateObjectMaps": [{ "predicate": "rdfs:label", "object": { "column": "title" } }]
        });
        let mut log = Vec::new();
        let fixed = repair_triples_map(
            &tm,
            "rows",
            &cols(&["title", "qty"]),
            &cols(&["rows"]),
            &mut log,
        )
        .unwrap();
        assert!(fixed["subject"]["template"].is_null());
        assert!(fixed["subject"]["hash"].is_array());
        assert_eq!(fixed["subject"]["seg"], "thing");
    }

    #[test]
    fn repoints_a_hallucinated_source_name() {
        let tm = json!({
            "source": "invented_table",
            "subject": { "hash": ["a"], "class": "ex:T" },
            "predicateObjectMaps": [{ "predicate": "rdfs:label", "object": { "column": "a" } }]
        });
        let mut log = Vec::new();
        let fixed =
            repair_triples_map(&tm, "real", &cols(&["a"]), &cols(&["real"]), &mut log).unwrap();
        assert_eq!(fixed["source"], "real");
        assert!(log.iter().any(|l| l.contains("invented_table")));
    }

    #[test]
    fn drops_a_triples_map_with_nothing_left() {
        let tm = json!({
            "source": "s",
            "subject": { "hash": ["x"] },
            "predicateObjectMaps": [{ "predicate": "ex:p", "object": { "column": "gone" } }]
        });
        let mut log = Vec::new();
        assert!(repair_triples_map(&tm, "s", &cols(&["a"]), &cols(&["s"]), &mut log).is_none());
    }
}
