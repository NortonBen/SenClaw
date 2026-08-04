//! Stage [6] Validation — a **SHACL-lite** validator implemented directly on
//! top of SPARQL against the Oxigraph store. This is intentionally *not* the
//! rudof/shacl_validation crate: keeping it in-house removes the oxrdf-version
//! alignment risk flagged in the research and keeps the app a single
//! self-contained binary. It covers the constraints the mapping pipeline
//! actually produces: cardinality, datatype, class, nodeKind, numeric range,
//! and regex pattern.
//!
//! SHACL is *closed-world* (missing data = violation), the counterpart to OWL's
//! open-world reasoning — the two do not replace each other.

use crate::graph::Graph;
use crate::vocab;
use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Default)]
pub struct Shapes {
    #[serde(default, rename = "nodeShapes")]
    pub node_shapes: Vec<NodeShape>,
}

#[derive(Deserialize)]
pub struct NodeShape {
    #[serde(rename = "targetClass")]
    pub target_class: String,
    #[serde(default)]
    pub properties: Vec<PropConstraint>,
}

#[derive(Deserialize)]
pub struct PropConstraint {
    pub path: String,
    #[serde(default)]
    pub datatype: Option<String>,
    #[serde(default)]
    pub class: Option<String>,
    #[serde(default, rename = "nodeKind")]
    pub node_kind: Option<String>,
    #[serde(default, rename = "minCount")]
    pub min_count: Option<i64>,
    #[serde(default, rename = "maxCount")]
    pub max_count: Option<i64>,
    #[serde(default, rename = "minInclusive")]
    pub min_inclusive: Option<f64>,
    #[serde(default, rename = "maxInclusive")]
    pub max_inclusive: Option<f64>,
    #[serde(default)]
    pub pattern: Option<String>,
}

const LIMIT: usize = 100;

fn push_violations(
    graph: &Graph,
    out: &mut Vec<serde_json::Value>,
    query: &str,
    focus_var: &str,
    value_var: Option<&str>,
    path: &str,
    constraint: &str,
    message: String,
) -> Result<usize> {
    let res = graph.query_json(query)?;
    let mut n = 0;
    if let Some(rows) = res["rows"].as_array() {
        for r in rows {
            let focus = r[focus_var]["value"].as_str().unwrap_or("").to_string();
            let value = value_var
                .and_then(|v| r[v]["value"].as_str())
                .unwrap_or("")
                .to_string();
            out.push(serde_json::json!({
                "focusNode": focus,
                "path": path,
                "constraint": constraint,
                "value": value,
                "message": message,
            }));
            n += 1;
        }
    }
    Ok(n)
}

/// Validate the store against `shapes`. Returns
/// `{ conforms, violationCount, checked, violations: [...] }`.
pub fn validate(
    graph: &Graph,
    base: &str,
    prefixes: &HashMap<String, String>,
    shapes: &Shapes,
) -> Result<serde_json::Value> {
    let mut violations: Vec<serde_json::Value> = Vec::new();
    let mut checked = 0usize;

    for shape in &shapes.node_shapes {
        let tc = vocab::expand(&shape.target_class, prefixes, base);
        for pc in &shape.properties {
            let path = vocab::expand(&pc.path, prefixes, base);
            let (tc_t, path_t) = match (vocab::iri_term(&tc), vocab::iri_term(&path)) {
                (Some(a), Some(b)) => (a, b),
                _ => continue,
            };

            if let Some(m) = pc.min_count {
                checked += 1;
                let q = format!(
                    "SELECT ?f (COUNT(DISTINCT ?v) AS ?n) WHERE {{ ?f a {tc_t} . OPTIONAL {{ ?f {path_t} ?v }} }} \
                     GROUP BY ?f HAVING (COUNT(DISTINCT ?v) < {m}) LIMIT {LIMIT}"
                );
                push_violations(
                    graph,
                    &mut violations,
                    &q,
                    "f",
                    None,
                    &path,
                    "minCount",
                    format!("fewer than {m} value(s) for {}", pc.path),
                )?;
            }
            if let Some(mx) = pc.max_count {
                checked += 1;
                let q = format!(
                    "SELECT ?f (COUNT(DISTINCT ?v) AS ?n) WHERE {{ ?f a {tc_t} . OPTIONAL {{ ?f {path_t} ?v }} }} \
                     GROUP BY ?f HAVING (COUNT(DISTINCT ?v) > {mx}) LIMIT {LIMIT}"
                );
                push_violations(
                    graph,
                    &mut violations,
                    &q,
                    "f",
                    None,
                    &path,
                    "maxCount",
                    format!("more than {mx} value(s) for {}", pc.path),
                )?;
            }
            if let Some(dt) = &pc.datatype {
                checked += 1;
                let dti = vocab::expand(dt, prefixes, base);
                let q = format!(
                    "SELECT ?f ?v WHERE {{ ?f a {tc_t} ; {path_t} ?v . \
                     FILTER(!isLiteral(?v) || datatype(?v) != <{dti}>) }} LIMIT {LIMIT}"
                );
                push_violations(
                    graph,
                    &mut violations,
                    &q,
                    "f",
                    Some("v"),
                    &path,
                    "datatype",
                    format!("value is not a {dt} literal"),
                )?;
            }
            if let Some(cl) = &pc.class {
                checked += 1;
                let cli = vocab::expand(cl, prefixes, base);
                let q = format!(
                    "SELECT ?f ?v WHERE {{ ?f a {tc_t} ; {path_t} ?v . \
                     FILTER((!isIRI(?v) && !isBlank(?v)) || NOT EXISTS {{ ?v a <{cli}> }}) }} LIMIT {LIMIT}"
                );
                push_violations(
                    graph,
                    &mut violations,
                    &q,
                    "f",
                    Some("v"),
                    &path,
                    "class",
                    format!("value is not a {cl}"),
                )?;
            }
            if let Some(nk) = &pc.node_kind {
                // Includes SHACL's three combined kinds. An unrecognized value is
                // NOT silently passed — it flags every node so the mistake surfaces.
                let filt = match nk.as_str() {
                    "IRI" => "!isIRI(?v)",
                    "Literal" => "!isLiteral(?v)",
                    "BlankNode" => "!isBlank(?v)",
                    "BlankNodeOrIRI" => "!(isBlank(?v) || isIRI(?v))",
                    "BlankNodeOrLiteral" => "!(isBlank(?v) || isLiteral(?v))",
                    "IRIOrLiteral" => "!(isIRI(?v) || isLiteral(?v))",
                    _ => "true",
                };
                checked += 1;
                let q = format!(
                    "SELECT ?f ?v WHERE {{ ?f a {tc_t} ; {path_t} ?v . FILTER({filt}) }} LIMIT {LIMIT}"
                );
                let msg = if filt == "true" {
                    format!("unknown nodeKind '{nk}'")
                } else {
                    format!("value is not a {nk}")
                };
                push_violations(
                    graph,
                    &mut violations,
                    &q,
                    "f",
                    Some("v"),
                    &path,
                    "nodeKind",
                    msg,
                )?;
            }
            if let Some(min) = pc.min_inclusive {
                checked += 1;
                // A non-numeric literal cannot satisfy a numeric bound → violation
                // (without `!isNumeric`, the comparison type-errors and the row is
                // silently dropped, letting bad data pass).
                let q = format!(
                    "SELECT ?f ?v WHERE {{ ?f a {tc_t} ; {path_t} ?v . \
                     FILTER(isLiteral(?v) && (!isNumeric(?v) || ?v < {min})) }} LIMIT {LIMIT}"
                );
                push_violations(
                    graph,
                    &mut violations,
                    &q,
                    "f",
                    Some("v"),
                    &path,
                    "minInclusive",
                    format!("value below minimum {min} (or not numeric)"),
                )?;
            }
            if let Some(max) = pc.max_inclusive {
                checked += 1;
                let q = format!(
                    "SELECT ?f ?v WHERE {{ ?f a {tc_t} ; {path_t} ?v . \
                     FILTER(isLiteral(?v) && (!isNumeric(?v) || ?v > {max})) }} LIMIT {LIMIT}"
                );
                push_violations(
                    graph,
                    &mut violations,
                    &q,
                    "f",
                    Some("v"),
                    &path,
                    "maxInclusive",
                    format!("value above maximum {max} (or not numeric)"),
                )?;
            }
            if let Some(pat) = &pc.pattern {
                checked += 1;
                let esc = vocab::escape_literal(pat);
                let q = format!(
                    "SELECT ?f ?v WHERE {{ ?f a {tc_t} ; {path_t} ?v . \
                     FILTER(!REGEX(STR(?v), \"{esc}\")) }} LIMIT {LIMIT}"
                );
                push_violations(
                    graph,
                    &mut violations,
                    &q,
                    "f",
                    Some("v"),
                    &path,
                    "pattern",
                    format!("value does not match /{pat}/"),
                )?;
            }
        }
    }

    Ok(serde_json::json!({
        "conforms": violations.is_empty(),
        "violationCount": violations.len(),
        "checked": checked,
        "violations": violations,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Graph, HashMap<String, String>) {
        let g = Graph::new().unwrap();
        // Two products: one valid, one with a negative price and no label.
        g.update(
            "INSERT DATA { GRAPH <urn:d> {
               <http://ex/p/1> a <http://ex/Product> ; <http://ex/price> \"150\"^^<http://www.w3.org/2001/XMLSchema#decimal> ; <http://www.w3.org/2000/01/rdf-schema#label> \"Widget\" .
               <http://ex/p/2> a <http://ex/Product> ; <http://ex/price> \"-5\"^^<http://www.w3.org/2001/XMLSchema#decimal> .
             } }",
        )
        .unwrap();
        let mut pfx = HashMap::new();
        pfx.insert("ex".into(), "http://ex/".into());
        (g, pfx)
    }

    #[test]
    fn detects_violations() {
        let (g, pfx) = fixture();
        let shapes: Shapes = serde_json::from_value(serde_json::json!({
            "nodeShapes": [{
                "targetClass": "ex:Product",
                "properties": [
                    { "path": "ex:price", "datatype": "xsd:decimal", "minCount": 1, "minInclusive": 0 },
                    { "path": "rdfs:label", "minCount": 1 }
                ]
            }]
        })).unwrap();
        let rep = validate(&g, "http://ex/", &pfx, &shapes).unwrap();
        assert_eq!(rep["conforms"], false);
        // p/2: negative price (minInclusive) + missing label (minCount) = 2 violations.
        assert_eq!(rep["violationCount"], 2);
    }
}
