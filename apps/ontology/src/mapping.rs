//! Stage [3][4] Mapping + Lifting. A small, RML-inspired **declarative mapping
//! DSL** (stored as JSON, not code — auditable, editable without recompiling)
//! and its interpreter. There is no mature pure-Rust RML engine, so this is a
//! deliberately compact subset covering the cases the profiler surfaces:
//! templated IRIs, stable hashed IRIs for keyless entities, typed/plain/lang
//! literals, and object references to other entities (joins).
//!
//! The interpreter emits triples as SPARQL `INSERT DATA` into a named graph
//! (one per import batch) — inserts are idempotent (RDF set semantics = natural
//! de-duplication / normalization).

use crate::graph::Graph;
use crate::profile::Table;
use crate::vocab;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Default)]
pub struct Mapping {
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub prefixes: HashMap<String, String>,
    #[serde(default, rename = "triplesMaps")]
    pub triples_maps: Vec<TriplesMap>,
}

#[derive(Deserialize)]
pub struct TriplesMap {
    #[serde(default)]
    pub name: String,
    /// Logical source name (matches a project source's `name`).
    pub source: String,
    pub subject: SubjectMap,
    #[serde(default, rename = "predicateObjectMaps")]
    pub poms: Vec<Pom>,
}

#[derive(Deserialize)]
pub struct SubjectMap {
    /// `product/{sku}` — `{col}` tokens substituted from the row.
    #[serde(default)]
    pub template: Option<String>,
    /// Columns to hash into a stable IRI when there is no natural key.
    #[serde(default)]
    pub hash: Option<Vec<String>>,
    /// Path segment for the hashed IRI (defaults to the class local name).
    #[serde(default)]
    pub seg: Option<String>,
    /// Curie/IRI of the class to `rdf:type` the subject as.
    #[serde(default)]
    pub class: Option<String>,
}

#[derive(Deserialize)]
pub struct Pom {
    pub predicate: String,
    pub object: ObjectMap,
}

#[derive(Deserialize)]
pub struct ObjectMap {
    /// Literal from a column value.
    #[serde(default)]
    pub column: Option<String>,
    /// XSD datatype curie/IRI for a column literal.
    #[serde(default)]
    pub datatype: Option<String>,
    /// Language tag for a column literal (mutually exclusive with datatype).
    #[serde(default)]
    pub lang: Option<String>,
    /// Constant value. `iri: true` → an IRI/curie object, else a plain literal.
    #[serde(default)]
    pub constant: Option<String>,
    #[serde(default)]
    pub iri: Option<bool>,
    /// IRI object built from a template (references another entity by key).
    #[serde(default)]
    pub template: Option<String>,
    /// IRI object built from a hash of columns (references a keyless entity).
    #[serde(default, rename = "parentHash")]
    pub parent_hash: Option<Vec<String>>,
    #[serde(default, rename = "parentSeg")]
    pub parent_seg: Option<String>,
}

/// Result of a lift/preview.
pub struct LiftReport {
    pub triples: usize,
    pub subjects: usize,
    pub skipped_rows: usize,
    /// Sample triples (subject, predicate, object) as display strings.
    pub samples: Vec<(String, String, String)>,
}

fn base_of<'a>(mapping: &'a Mapping, fallback: &'a str) -> &'a str {
    mapping.base.as_deref().filter(|s| !s.is_empty()).unwrap_or(fallback)
}

/// Turn a template/absolute string into a full IRI under `base`.
fn iri_from_template(templ: &str, row: &HashMap<String, String>, base: &str) -> Option<String> {
    let filled = vocab::apply_template(templ, row)?;
    if filled.starts_with("http://") || filled.starts_with("https://") || filled.starts_with("urn:") {
        Some(filled)
    } else {
        Some(format!("{}{}", base.trim_end_matches('/'), format!("/{}", filled.trim_start_matches('/'))))
    }
}

fn hashed(base: &str, seg: &str, cols: &[String], row: &HashMap<String, String>) -> Option<String> {
    // Map an absent column to "" (not drop it) so hashing stays positional and
    // stable across ragged rows — otherwise `[a, <missing>]` and `[a]` collide.
    let parts: Vec<String> = cols.iter().map(|c| row.get(c).cloned().unwrap_or_default()).collect();
    if parts.is_empty() || parts.iter().all(|p| p.trim().is_empty()) {
        return None;
    }
    let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    Some(vocab::hashed_iri(base, seg, &refs))
}

fn local_name(iri: &str) -> String {
    iri.rsplit(['#', '/']).next().unwrap_or("entity").to_lowercase()
}

/// Build the triples for one triples-map row; returns (subject_iri, [(p,o_term,is_literal_display)]).
/// Emitted terms are already SPARQL-ready (`<iri>` or `"lit"...`).
fn row_triples(
    tm: &TriplesMap,
    row: &HashMap<String, String>,
    base: &str,
    prefixes: &HashMap<String, String>,
) -> Option<(String, Vec<(String, String, String)>)> {
    // Subject IRI.
    let subject_iri = if let Some(t) = &tm.subject.template {
        iri_from_template(t, row, base)?
    } else if let Some(cols) = &tm.subject.hash {
        let seg = tm
            .subject
            .seg
            .clone()
            .or_else(|| tm.subject.class.as_ref().map(|c| local_name(&vocab::expand(c, prefixes, base))))
            .unwrap_or_else(|| "entity".to_string());
        hashed(base, &seg, cols, row)?
    } else {
        return None;
    };
    let subj_term = vocab::iri_term(&subject_iri)?;

    let mut triples: Vec<(String, String, String)> = Vec::new();

    if let Some(class) = &tm.subject.class {
        let cls = vocab::expand(class, prefixes, base);
        if let Some(ct) = vocab::iri_term(&cls) {
            triples.push((subj_term.clone(), format!("<{}type>", vocab::RDF), ct));
        }
    }

    for pom in &tm.poms {
        let pred = vocab::expand(&pom.predicate, prefixes, base);
        let pred_term = match vocab::iri_term(&pred) {
            Some(p) => p,
            None => continue,
        };
        let obj = &pom.object;
        let obj_term = if let Some(col) = &obj.column {
            let val = row.get(col).map(|s| s.as_str()).unwrap_or("");
            if val.trim().is_empty() {
                continue;
            }
            if let Some(lang) = &obj.lang {
                // A langtag lands unescaped after `@`; reject invalid ones (which
                // could otherwise inject SPARQL) and fall back to a plain literal.
                if vocab::valid_langtag(lang) {
                    format!("\"{}\"@{}", vocab::escape_literal(val), lang)
                } else {
                    format!("\"{}\"", vocab::escape_literal(val))
                }
            } else {
                let dt = obj.datatype.as_ref().map(|d| vocab::expand(d, prefixes, base));
                vocab::literal_term(val, dt.as_deref())
            }
        } else if let Some(t) = &obj.template {
            match iri_from_template(t, row, base).and_then(|i| vocab::iri_term(&i)) {
                Some(x) => x,
                None => continue,
            }
        } else if let Some(cols) = &obj.parent_hash {
            let seg = obj.parent_seg.clone().unwrap_or_else(|| "entity".to_string());
            match hashed(base, &seg, cols, row).and_then(|i| vocab::iri_term(&i)) {
                Some(x) => x,
                None => continue,
            }
        } else if let Some(cst) = &obj.constant {
            if obj.iri.unwrap_or(false) {
                let e = vocab::expand(cst, prefixes, base);
                match vocab::iri_term(&e) {
                    Some(x) => x,
                    None => continue,
                }
            } else {
                format!("\"{}\"", vocab::escape_literal(cst))
            }
        } else {
            continue;
        };
        triples.push((subj_term.clone(), pred_term, obj_term));
    }

    Some((subject_iri, triples))
}

/// Run every triples-map against its resolved source table and INSERT into
/// `batch_graph`. `sources` maps a logical source name → parsed `Table`.
pub fn lift(
    graph: &Graph,
    mapping: &Mapping,
    sources: &HashMap<String, Table>,
    base_iri: &str,
    batch_graph: &str,
) -> Result<LiftReport> {
    let base = base_of(mapping, base_iri).to_string();
    let mut pending: Vec<(String, String, String)> = Vec::new();
    let mut subjects = std::collections::HashSet::new();
    let mut total = 0usize;
    let mut skipped = 0usize;
    let mut samples = Vec::new();

    let batch_ok = vocab::iri_term(batch_graph).ok_or_else(|| anyhow!("bad batch graph iri"))?;
    let _ = batch_ok;

    for tm in &mapping.triples_maps {
        let table = sources
            .get(&tm.source)
            .ok_or_else(|| anyhow!("source '{}' not found for triples-map '{}'", tm.source, tm.name))?;
        for row in &table.rows {
            let rm = table.row_map(row);
            match row_triples(tm, &rm, &base, &mapping.prefixes) {
                Some((subj, triples)) => {
                    subjects.insert(subj);
                    for (s, p, o) in triples {
                        if samples.len() < 20 {
                            samples.push((s.clone(), p.clone(), o.clone()));
                        }
                        pending.push((s, p, o));
                        total += 1;
                        if pending.len() >= 2000 {
                            flush(graph, batch_graph, &mut pending)?;
                        }
                    }
                }
                None => skipped += 1,
            }
        }
    }
    flush(graph, batch_graph, &mut pending)?;
    // NOTE: we deliberately do NOT dedup a data batch against other batches —
    // that would let dropping the earliest batch delete triples a later batch
    // also asserts, breaking provenance isolation. Cross-batch identical triples
    // are honest multi-source provenance; only the inferred graph is deduped.

    Ok(LiftReport {
        triples: total,
        subjects: subjects.len(),
        skipped_rows: skipped,
        samples,
    })
}

/// Preview without writing: run over the first `limit` rows total and collect
/// sample triples.
pub fn preview(
    mapping: &Mapping,
    sources: &HashMap<String, Table>,
    base_iri: &str,
    limit: usize,
) -> Result<LiftReport> {
    let base = base_of(mapping, base_iri).to_string();
    let mut samples = Vec::new();
    let mut subjects = std::collections::HashSet::new();
    let mut total = 0usize;
    let mut skipped = 0usize;
    'outer: for tm in &mapping.triples_maps {
        let table = sources
            .get(&tm.source)
            .ok_or_else(|| anyhow!("source '{}' not found for triples-map '{}'", tm.source, tm.name))?;
        for row in table.rows.iter().take(limit) {
            let rm = table.row_map(row);
            match row_triples(tm, &rm, &base, &mapping.prefixes) {
                Some((subj, triples)) => {
                    subjects.insert(subj);
                    for t in triples {
                        total += 1;
                        if samples.len() < 60 {
                            samples.push(t);
                        } else {
                            break 'outer;
                        }
                    }
                }
                None => skipped += 1,
            }
        }
    }
    Ok(LiftReport { triples: total, subjects: subjects.len(), skipped_rows: skipped, samples })
}

fn flush(graph: &Graph, batch_graph: &str, pending: &mut Vec<(String, String, String)>) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let mut body = String::new();
    for (s, p, o) in pending.drain(..) {
        body.push_str(&format!("{s} {p} {o} .\n"));
    }
    let update = format!("INSERT DATA {{ GRAPH <{batch_graph}> {{\n{body}}} }}");
    graph.update(&update)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::parse_csv;

    fn shop_mapping() -> Mapping {
        let json = serde_json::json!({
            "base": "http://ex/shop",
            "prefixes": { "ex": "http://ex/shop#" },
            "triplesMaps": [{
                "name": "ProductMap",
                "source": "products",
                "subject": { "template": "product/{sku}", "class": "ex:Product" },
                "predicateObjectMaps": [
                    { "predicate": "rdfs:label", "object": { "column": "name" } },
                    { "predicate": "ex:hasPrice", "object": { "column": "price", "datatype": "xsd:decimal" } },
                    { "predicate": "ex:hasSupplier", "object": { "template": "supplier/{supplier}" } }
                ]
            }]
        });
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn malicious_datatype_and_lang_do_not_inject() {
        let g = Graph::new().unwrap();
        // Seed a triple in another graph that a successful `DROP ALL` would erase.
        g.update("INSERT DATA { GRAPH <urn:keep> { <urn:s> <urn:p> \"v\" } }").unwrap();
        let evil_dt = "xsd:decimal> } } ; DROP ALL ; INSERT DATA { GRAPH <urn:evil> { <urn:a> <urn:b> \"c";
        let mapping: Mapping = serde_json::from_value(serde_json::json!({
            "base": "http://ex",
            "prefixes": { "ex": "http://ex#" },
            "triplesMaps": [{
                "name": "M", "source": "t",
                "subject": { "template": "i/{id}", "class": "ex:T" },
                "predicateObjectMaps": [
                    { "predicate": "ex:v", "object": { "column": "v", "datatype": evil_dt } },
                    { "predicate": "ex:l", "object": { "column": "v", "lang": "en } ; DROP ALL ; #" } }
                ]
            }]
        })).unwrap();
        let mut sources = HashMap::new();
        sources.insert("t".to_string(), parse_csv("id,v\n1,hello\n").unwrap());
        lift(&g, &mapping, &sources, "http://ex", "urn:batch:x").unwrap();
        // The seeded triple survives → no DROP ALL executed; no injected graph.
        assert_eq!(g.graph_len("urn:keep").unwrap(), 1);
        assert_eq!(g.graph_len("urn:evil").unwrap(), 0);
    }

    #[test]
    fn lifts_into_named_graph() {
        let g = Graph::new().unwrap();
        let csv = "sku,name,price,supplier\nA1,Widget,150000,acme\nA2,Gadget,90000,acme\n";
        let mut sources = HashMap::new();
        sources.insert("products".to_string(), parse_csv(csv).unwrap());
        let m = shop_mapping();
        let rep = lift(&g, &m, &sources, "http://ex/shop", "urn:batch:1").unwrap();
        // 2 products × (type + label + price + supplier) = 8 triples.
        assert_eq!(rep.triples, 8);
        assert_eq!(rep.subjects, 2);
        assert_eq!(g.graph_len("urn:batch:1").unwrap(), 8);
        // idempotent: lifting again does not grow the graph.
        lift(&g, &m, &sources, "http://ex/shop", "urn:batch:1").unwrap();
        assert_eq!(g.graph_len("urn:batch:1").unwrap(), 8);
    }
}
