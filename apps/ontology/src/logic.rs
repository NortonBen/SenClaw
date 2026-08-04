//! **AIP Logic** — LLM-powered functions whose output is a *proposal*, not a
//! write.
//!
//! A logic function takes an input (a text document, or the rows of a source)
//! and an instruction in plain language, and returns a list of typed
//! [`crate::action::Action`]s. Every action is run through the T-Box type
//! checker; the valid ones land in the **proposal queue** ([`crate::db`]
//! `proposals`) for a human to approve, the invalid ones are recorded with the
//! reason. Nothing mutates the graph until someone approves — human-in-the-loop
//! is the default, `auto_apply` is an explicit opt-in.
//!
//! Two kinds, matching the two halves of AIP:
//! * **extract** (design-time-ish): pull structured facts out of one text blob.
//! * **classify** (the run-time "LLM node"): run the same instruction over each
//!   row of a source at scale, one `set_attribute` proposal per row, with
//!   per-row **retry** so a transient rate-limit doesn't lose the batch, and a
//!   **trial run** to preview on a few rows before committing to the whole set.
//!
//! The metadata/data boundary from Assist does NOT apply here: `classify` is
//! the run-time node, so it legitimately sees row values — that is the point.
//! What stays invariant is that its *output* is still a validated proposal.

use crate::action::{self, Action, Schema};
use crate::api::AppState;
use crate::db::now;
use crate::{llm, profile as prof, tbox};
use serde_json::{json, Value};
use std::sync::Arc;

/// One row of an LLM proposal before it is validated.
struct Raw {
    action: Value,
    rationale: String,
    confidence: f64,
}

fn raw_list(v: &Value) -> Vec<Raw> {
    let arr = v
        .get("actions")
        .and_then(|a| a.as_array())
        .or_else(|| v.as_array());
    arr.map(|a| {
        a.iter()
            .map(|x| Raw {
                action: x.clone(),
                rationale: x
                    .get("rationale")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string(),
                confidence: x.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.0),
            })
            .collect()
    })
    .unwrap_or_default()
}

const EXTRACT_SYS: &str = "You are an AIP Logic function. You turn input into a list of TYPED ACTIONS on an ontology — \
you never write queries or free text. Return ONLY JSON: {\"actions\":[{\"op\":\"...\",...,\"rationale\":\"why\",\"confidence\":0.0}]}. \
Allowed ops and fields: \
add_individual{class, key, label?} — declare an entity of a class (key = a stable natural key); \
set_attribute{subject, property, value, datatype?} — a literal/data property; \
add_relation{subject, property, object} — a link between two entities; \
link_entities{a, b, predicate?} — same/close entity. \
Use ONLY classes and properties from the given T-Box — if the ontology has no property for something, omit it rather than \
inventing one. subject/object are natural keys or labels; reuse the same label for the same real-world entity. Never \
state a fact not supported by the input. Give a short rationale and a 0..1 confidence per action.";

/// Extract typed actions from one text blob, grounded on the T-Box. `on` pins a
/// specific model (evals); `None` uses the app's selected model.
async fn extract_actions(
    text: &str,
    tb: &Value,
    instruction: &str,
    on: Option<&str>,
) -> Result<Vec<Raw>, String> {
    let prompt = format!(
        "T-Box (the ONLY vocabulary you may use):\n{}\n\nTask: {}\n\nInput:\n\"\"\"\n{}\n\"\"\"\n\nReturn the actions JSON.",
        serde_json::to_string_pretty(tb).unwrap_or_default(),
        if instruction.trim().is_empty() { "Extract every fact that the ontology can represent." } else { instruction },
        llm::truncate_for_prompt(text),
    );
    let (v, _model) = llm::ask_json_on(EXTRACT_SYS, &prompt, 1500, on).await?;
    Ok(raw_list(&v))
}

const CLASSIFY_SYS: &str = "You are an AIP Logic classification node running over ONE row of a table. Apply the instruction \
and return ONLY JSON: {\"actions\":[{\"op\":\"set_attribute\",\"subject\":\"<row key>\",\"property\":\"<T-Box property>\",\
\"value\":\"...\",\"rationale\":\"why\",\"confidence\":0.0}]}. Use ONLY a property that exists in the given T-Box. Return an \
empty actions list if the row does not warrant a value. Do not invent facts beyond the row.";

/// Classify one row → a set_attribute (or nothing). Retried by the caller.
/// `on` pins a specific model (evals); `None` uses the app's selected model.
async fn classify_row(
    row: &Value,
    subject: &str,
    tb: &Value,
    instruction: &str,
    on: Option<&str>,
) -> Result<Vec<Raw>, String> {
    let prompt = format!(
        "T-Box properties available:\n{}\n\nInstruction: {}\n\nRow key (use as subject): {}\nRow: {}\n\nReturn the actions JSON.",
        serde_json::to_string_pretty(&tb["properties"]).unwrap_or_default(),
        instruction,
        subject,
        serde_json::to_string(row).unwrap_or_default(),
    );
    let (v, _model) = llm::ask_json_on(CLASSIFY_SYS, &prompt, 500, on).await?;
    Ok(raw_list(&v))
}

/// Retry a per-row LLM call a few times on transient failure. Rate limits and
/// blips are the norm when fanning a classify function over a whole source, so a
/// single failure must not sink the row — but a validation-shaped error (bad
/// JSON that keeps coming back) is not worth hammering, so the cap is small.
async fn with_retry<F, Fut>(mut f: F, attempts: usize) -> Result<Vec<Raw>, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Vec<Raw>, String>>,
{
    let mut last = String::new();
    for i in 0..attempts {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                last = e;
                // Back off only on signals that look transient.
                let transient = last.contains("429")
                    || last.to_lowercase().contains("rate")
                    || last.contains("timeout")
                    || last.contains("timed out")
                    || last.contains("503");
                if !transient || i + 1 == attempts {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(400 * (i as u64 + 1))).await;
            }
        }
    }
    Err(last)
}

/// Result of running (or trialing) a function.
#[derive(serde::Serialize, Default)]
pub struct RunReport {
    pub proposed: usize,
    pub invalid: usize,
    pub applied: usize,
    pub skipped_inputs: usize,
    /// Preview rows for a trial (never persisted).
    pub preview: Vec<Value>,
    pub errors: Vec<String>,
    pub batch: String,
}

/// Turn raw LLM actions into (validated) proposals: enqueue them (or, for a
/// trial, just collect a preview). Shared by extract and classify.
fn stage(
    state: &Arc<AppState>,
    pid: i64,
    fid: Option<i64>,
    schema: &Schema,
    raws: Vec<Raw>,
    trial: bool,
    rep: &mut RunReport,
) {
    for raw in raws {
        let Ok(act) = serde_json::from_value::<Action>(raw.action.clone()) else {
            continue;
        };
        let (valid, reason) = match act.validate(schema) {
            Ok(_) => (true, String::new()),
            Err(e) => (false, e),
        };
        if valid {
            rep.proposed += 1;
        } else {
            rep.invalid += 1;
        }
        if trial {
            rep.preview.push(json!({
                "action": raw.action, "summary": act.summary(),
                "valid": valid, "invalidReason": reason,
                "confidence": raw.confidence, "rationale": raw.rationale,
            }));
        } else {
            let _ = state.db.add_proposal(
                pid,
                fid,
                &serde_json::to_string(&act).unwrap_or_default(),
                &act.summary(),
                &raw.rationale,
                raw.confidence,
                valid,
                &reason,
            );
        }
    }
}

/// Run a saved function. `trial` = preview only (no queue writes, sampled).
pub async fn run(
    state: &Arc<AppState>,
    pid: i64,
    fid: i64,
    trial: bool,
) -> Result<RunReport, String> {
    let (kind, _input_kind, target, instruction, auto_apply) = state
        .db
        .get_function(fid)
        .map_err(|e| e.to_string())?
        .ok_or("function not found")?;
    let (base, pfx) = state.ctx(pid).map_err(|e| e.to_string())?;
    let g = state.graph_for(pid).map_err(|e| e.to_string())?;
    let schema = Schema::build(&g, &base, &pfx).map_err(|e| e.to_string())?;
    if schema_is_empty(&g) {
        return Err("this project has no T-Box yet — design or auto-build the ontology first, so actions can be type-checked".into());
    }
    let tb = tbox::read(&g).map_err(|e| e.to_string())?;
    let mut rep = RunReport::default();

    match kind.as_str() {
        "resolve" => {
            // Entity resolution as proposals — no LLM. Deterministic Jaro-Winkler
            // duplicate candidates for a class become link_entities proposals, so
            // dedup goes through the SAME review-then-apply flow as everything
            // else instead of a side channel. `target` is the class; an optional
            // trailing number in the instruction tunes the similarity threshold.
            if target.trim().is_empty() {
                return Err("resolve needs a target class (e.g. ex:Supplier)".into());
            }
            let threshold = parse_threshold(&instruction);
            let cands =
                crate::resolve::candidates(&g, &base, &pfx, &target, "rdfs:label", threshold)
                    .map_err(|e| e.to_string())?;
            let pairs = cands["pairs"].as_array().cloned().unwrap_or_default();
            let take = if trial { 5 } else { pairs.len() };
            for p in pairs.into_iter().take(take) {
                let (Some(a), Some(b)) = (p["a"].as_str(), p["b"].as_str()) else {
                    continue;
                };
                let score = p["score"].as_f64().unwrap_or(0.0);
                let raw = Raw {
                    action: json!({ "op": "link_entities", "a": a, "b": b, "predicate": "skos:closeMatch" }),
                    rationale: format!(
                        "label similarity {:.3}: '{}' ≈ '{}'",
                        score,
                        p["labelA"].as_str().unwrap_or(""),
                        p["labelB"].as_str().unwrap_or(""),
                    ),
                    confidence: score,
                };
                stage(state, pid, Some(fid), &schema, vec![raw], trial, &mut rep);
            }
        }
        "classify" => {
            // Run-time LLM node over the rows of a source.
            let (skind, content) = state
                .db
                .source_by_name(pid, &target)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("source '{target}' not found for classify"))?;
            let table = prof::parse(&skind, &content).map_err(|e| e.to_string())?;
            let key_col = pick_key_column(&table);
            let limit = if trial { 5 } else { table.rows.len() };
            for (i, row) in table.rows.iter().take(limit).enumerate() {
                let rowmap = table.row_map(row);
                let key_val = key_col
                    .as_deref()
                    .and_then(|k| rowmap.get(k))
                    .filter(|v| !v.trim().is_empty())
                    .cloned();
                // Classify ENRICHES an existing entity, so the subject must be the
                // IRI the lift already minted for this row — not a fresh hash of
                // the key, which would attach the value to an orphan node. Resolve
                // by the key value; fall back to the raw key only if nothing lifted.
                let subject = key_val
                    .as_deref()
                    .and_then(|k| resolve_existing(&g, k))
                    .or(key_val.clone())
                    .unwrap_or_else(|| format!("row-{}", i + 1));
                let rowval = json!(rowmap);
                let res = with_retry(
                    || classify_row(&rowval, &subject, &tb, &instruction, None),
                    3,
                )
                .await;
                match res {
                    Ok(raws) => stage(state, pid, Some(fid), &schema, raws, trial, &mut rep),
                    Err(e) => rep.errors.push(format!("row '{subject}': {e}")),
                }
            }
        }
        _ => {
            // extract: over text sources (or a single supplied one via target).
            let sources = state.db.list_sources(pid).map_err(|e| e.to_string())?;
            let texts: Vec<(String, String)> = sources
                .iter()
                .filter(|s| s.kind == "text" && (target.is_empty() || s.name == target))
                .filter_map(|s| {
                    state
                        .db
                        .get_source(s.id)
                        .ok()
                        .flatten()
                        .map(|(_, _, c)| (s.name.clone(), c))
                })
                .collect();
            if texts.is_empty() {
                return Err("no text source to extract from — upload a document, or set the function's target to one".into());
            }
            for (name, content) in texts {
                let chunks = llm::chunk_text(&content, llm::CHUNK_CHARS);
                let take = if trial { 1 } else { chunks.len() };
                for chunk in chunks.into_iter().take(take.max(1)) {
                    match extract_actions(&chunk, &tb, &instruction, None).await {
                        Ok(raws) => stage(state, pid, Some(fid), &schema, raws, trial, &mut rep),
                        Err(e) => rep.errors.push(format!("{name}: {e}")),
                    }
                }
            }
        }
    }

    // auto_apply: skip the queue and apply the pending valid proposals now. The
    // audit trail (provenance batch) is identical to a human approval — only the
    // human step is removed, and only because the function's author asked for it.
    if !trial && auto_apply && rep.proposed > 0 {
        let applied = approve(state, pid, &[]).await?;
        rep.applied = applied["applied"].as_u64().unwrap_or(0) as usize;
        rep.batch = applied["batch"].as_str().unwrap_or("").to_string();
    }
    Ok(rep)
}

/// Approve pending proposals (all, or a specific set) → validate again and
/// apply as one provenance batch. Re-validating at approval time matters: the
/// T-Box may have changed since the proposal was made, and a proposal that no
/// longer type-checks must not slip through.
pub async fn approve(state: &Arc<AppState>, pid: i64, ids: &[i64]) -> Result<Value, String> {
    let (base, pfx) = state.ctx(pid).map_err(|e| e.to_string())?;
    let g = state.graph_for(pid).map_err(|e| e.to_string())?;
    let schema = Schema::build(&g, &base, &pfx).map_err(|e| e.to_string())?;
    let pending = state
        .db
        .pending_actions(pid, ids)
        .map_err(|e| e.to_string())?;
    if pending.is_empty() {
        return Ok(json!({ "applied": 0, "triples": 0, "batch": "", "rejected": [] }));
    }
    let mut actions = Vec::new();
    let mut ok_ids = Vec::new();
    let mut stale = Vec::new();
    for (id, jsn) in pending {
        match serde_json::from_str::<Action>(&jsn) {
            Ok(a) if a.validate(&schema).is_ok() => {
                actions.push(a);
                ok_ids.push(id);
            }
            Ok(_) => stale.push(id), // no longer type-checks
            Err(_) => stale.push(id),
        }
    }
    let ts = now();
    let rep = action::apply(&g, &schema, &actions, "approved", ts).map_err(|e| e.to_string())?;
    if rep.triples > 0 {
        state.persist(pid, &g).map_err(|e| e.to_string())?;
    }
    for id in &ok_ids {
        let _ = state.db.set_proposal_status(*id, "approved", &rep.batch);
    }
    for id in &stale {
        let _ = state.db.set_proposal_status(*id, "invalid", "");
    }
    state.db.log(
        pid,
        "logic.approve",
        &format!("{} action(s) → {} triples", rep.applied, rep.triples),
    );
    Ok(json!({
        "applied": rep.applied, "triples": rep.triples, "batch": rep.batch,
        "staleRejected": stale.len(),
    }))
}

/// Whether the project has any T-Box terms. Actions cannot be type-checked
/// without a schema, so running a logic function against an empty one is an
/// error, not an empty result.
fn schema_is_empty(g: &crate::graph::Graph) -> bool {
    tbox::read(g)
        .map(|tb| {
            tb["classes"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(true)
                && tb["properties"]
                    .as_array()
                    .map(|a| a.is_empty())
                    .unwrap_or(true)
        })
        .unwrap_or(true)
}

/// Find an already-lifted individual whose IRI ends in this key, or which
/// carries the key as a literal value — so a classify attaches its value to the
/// real entity the mapping created rather than to a new orphan node. Returns a
/// full IRI (which the action layer keeps verbatim). None → nothing lifted yet.
fn resolve_existing(g: &crate::graph::Graph, key: &str) -> Option<String> {
    let esc = crate::vocab::escape_literal(key.trim());
    // Prefer a subject that has the key as a data value (e.g. hasSku "A1")…
    let q = format!(
        "SELECT ?s WHERE {{ ?s ?p ?v . FILTER(isIRI(?s)) FILTER(str(?v) = \"{esc}\") }} LIMIT 1"
    );
    if let Ok(res) = g.query_json(&q) {
        if let Some(iri) = res["rows"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|r| r["s"]["value"].as_str())
        {
            return Some(iri.to_string());
        }
    }
    // …else a subject whose IRI path segment is the key (templated `.../A1`).
    let q2 = format!(
        "SELECT ?s WHERE {{ ?s a ?c . FILTER(isIRI(?s)) FILTER(STRENDS(STR(?s), \"/{esc}\")) }} LIMIT 1"
    );
    g.query_json(&q2).ok().and_then(|res| {
        res["rows"]
            .as_array()
            .and_then(|a| a.first())
            .and_then(|r| r["s"]["value"].as_str())
            .map(String::from)
    })
}

/// Pull a similarity threshold out of a resolve function's instruction (the
/// first number in 0..1, or a percentage), else a sensible default.
fn parse_threshold(instruction: &str) -> f64 {
    for tok in instruction.split(|c: char| !c.is_ascii_digit() && c != '.' && c != '%') {
        if let Some(pct) = tok.strip_suffix('%') {
            if let Ok(n) = pct.parse::<f64>() {
                if (0.0..=100.0).contains(&n) {
                    return n / 100.0;
                }
            }
        } else if let Ok(n) = tok.parse::<f64>() {
            if (0.0..=1.0).contains(&n) {
                return n;
            }
        }
    }
    0.85
}

/// Best identifying column for a row subject: the first unique identifier-role
/// column, else the first column.
fn pick_key_column(table: &prof::Table) -> Option<String> {
    let profiles = prof::profile(table);
    profiles
        .iter()
        .find(|c| c.role == "identifier" && c.is_unique)
        .or_else(|| profiles.iter().find(|c| c.is_unique))
        .map(|c| c.name.clone())
        .or_else(|| table.headers.first().cloned())
}

// ---------------------------------------------------------------------------
// Evals
// ---------------------------------------------------------------------------

/// Run a function's eval cases across one or more model profiles. For each case
/// the function's instruction is applied to the input and the produced action
/// summaries are checked to contain the expected substring; each case is run
/// twice per model to surface run-to-run variance.
pub async fn run_evals(
    state: &Arc<AppState>,
    pid: i64,
    fid: i64,
    profiles: &[String],
) -> Result<Value, String> {
    let (kind, _ik, _t, instruction, _aa) = state
        .db
        .get_function(fid)
        .map_err(|e| e.to_string())?
        .ok_or("function not found")?;
    let cases = state.db.list_eval_cases(fid).map_err(|e| e.to_string())?;
    if cases.is_empty() {
        return Err("no eval cases for this function".into());
    }
    let g = state.graph_for(pid).map_err(|e| e.to_string())?;
    let tb = tbox::read(&g).map_err(|e| e.to_string())?;
    // Empty profiles = the currently-selected model only.
    let runs: Vec<Option<String>> = if profiles.is_empty() {
        vec![None]
    } else {
        profiles.iter().map(|p| Some(p.clone())).collect()
    };

    let mut model_results = Vec::new();
    for prof_opt in &runs {
        // The model is passed EXPLICITLY per call (`on`) — never by mutating the
        // shared profile cell, which would race with other requests and could be
        // left wrong if the run returned early.
        let on = prof_opt.as_deref();
        let mut passed = 0usize;
        let mut varied = 0usize;
        let mut case_out = Vec::new();
        for (_cid, input, expect) in &cases {
            // Two runs → summaries; pass = expect substring present in either.
            let a = summaries_for(&kind, input, &tb, &instruction, on).await;
            let b = summaries_for(&kind, input, &tb, &instruction, on).await;
            let joined_a = a.join(" | ").to_lowercase();
            let joined_b = b.join(" | ").to_lowercase();
            let want = expect.trim().to_lowercase();
            let pass = want.is_empty() || joined_a.contains(&want) || joined_b.contains(&want);
            if pass {
                passed += 1;
            }
            if joined_a != joined_b {
                varied += 1;
            }
            case_out.push(json!({
                "input": input.chars().take(60).collect::<String>(), "expect": expect,
                "pass": pass, "varied": joined_a != joined_b,
                "run1": a, "run2": b,
            }));
        }
        model_results.push(json!({
            "model": prof_opt.clone().unwrap_or_else(|| "current".into()),
            "passed": passed, "total": cases.len(), "varied": varied,
            "cases": case_out,
        }));
    }
    Ok(json!({ "functionId": fid, "results": model_results }))
}

async fn summaries_for(
    kind: &str,
    input: &str,
    tb: &Value,
    instruction: &str,
    on: Option<&str>,
) -> Vec<String> {
    let raws = if kind == "classify" {
        classify_row(&json!({ "text": input }), "eval-input", tb, instruction, on).await
    } else {
        extract_actions(input, tb, instruction, on).await
    };
    raws.unwrap_or_default()
        .into_iter()
        .filter_map(|r| serde_json::from_value::<Action>(r.action).ok())
        .map(|a| a.summary())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_threshold;

    #[test]
    fn threshold_parsing() {
        assert_eq!(parse_threshold("merge duplicates above 0.92"), 0.92);
        assert_eq!(parse_threshold("similarity 90%"), 0.9);
        assert_eq!(parse_threshold("just find dupes"), 0.85); // default
        assert_eq!(parse_threshold("ignore 250 and use 0.8"), 0.8); // 250 out of range, 0.8 taken
    }
}
