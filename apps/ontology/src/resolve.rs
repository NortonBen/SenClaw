//! Stage [5] Entity resolution. After lifting, the same real-world entity can
//! end up with several IRIs (names spelled slightly differently). We block
//! candidates by a normalized label prefix, score pairs with Jaro-Winkler, and
//! propose links. The safe default link is `skos:closeMatch` (NOT `owl:sameAs`,
//! which is transitive+symmetric — one bad link contaminates a whole cluster,
//! as the research notes). Links land in a dedicated resolution graph.

use crate::graph::Graph;
use crate::vocab;
use anyhow::{anyhow, Result};
use std::collections::HashMap;

pub const RESOLUTION_GRAPH: &str = "urn:senclaw:ontology:resolution";

fn normalize(s: &str) -> String {
    s.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Find likely-duplicate pairs of individuals of `class` compared on
/// `label_prop`. Returns pairs sorted by descending similarity.
pub fn candidates(
    graph: &Graph,
    base: &str,
    prefixes: &HashMap<String, String>,
    class: &str,
    label_prop: &str,
    threshold: f64,
) -> Result<serde_json::Value> {
    let cls = vocab::expand(class, prefixes, base);
    let lp = vocab::expand(label_prop, prefixes, base);
    let (ct, lt) = (
        vocab::iri_term(&cls).ok_or_else(|| anyhow!("bad class iri"))?,
        vocab::iri_term(&lp).ok_or_else(|| anyhow!("bad label property iri"))?,
    );
    let q = format!("SELECT ?e ?label WHERE {{ ?e a {ct} ; {lt} ?label }}");
    let res = graph.query_json(&q)?;

    // Gather (iri, label) and block by first two normalized chars.
    let mut items: Vec<(String, String)> = Vec::new();
    if let Some(rows) = res["rows"].as_array() {
        for r in rows {
            let iri = r["e"]["value"].as_str().unwrap_or("").to_string();
            let label = r["label"]["value"].as_str().unwrap_or("").to_string();
            if !iri.is_empty() && !label.trim().is_empty() {
                items.push((iri, label));
            }
        }
    }
    let mut blocks: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, (_, label)) in items.iter().enumerate() {
        let norm = normalize(label);
        let key: String = norm.chars().take(2).collect();
        blocks.entry(key).or_default().push(i);
    }

    let mut pairs: Vec<serde_json::Value> = Vec::new();
    for idxs in blocks.values() {
        for a in 0..idxs.len() {
            for b in (a + 1)..idxs.len() {
                let (ia, ib) = (idxs[a], idxs[b]);
                let (iri_a, la) = &items[ia];
                let (iri_b, lb) = &items[ib];
                if iri_a == iri_b {
                    continue;
                }
                let score = strsim::jaro_winkler(&normalize(la), &normalize(lb));
                if score >= threshold {
                    let (x, y) = if iri_a <= iri_b { (iri_a, iri_b) } else { (iri_b, iri_a) };
                    pairs.push(serde_json::json!({
                        "a": x, "b": y,
                        "labelA": if iri_a <= iri_b { la } else { lb },
                        "labelB": if iri_a <= iri_b { lb } else { la },
                        "score": (score * 1000.0).round() / 1000.0,
                    }));
                }
            }
        }
    }
    pairs.sort_by(|x, y| {
        y["score"].as_f64().unwrap_or(0.0).partial_cmp(&x["score"].as_f64().unwrap_or(0.0)).unwrap()
    });
    pairs.truncate(200);
    Ok(serde_json::json!({ "count": pairs.len(), "pairs": pairs }))
}

/// Apply `a <predicate> b` links into the resolution graph. `predicate` is a
/// curie (default `skos:closeMatch`).
pub fn apply(
    graph: &Graph,
    base: &str,
    prefixes: &HashMap<String, String>,
    predicate: &str,
    pairs: &[(String, String)],
) -> Result<usize> {
    let pred = vocab::expand(predicate, prefixes, base);
    let pt = vocab::iri_term(&pred).ok_or_else(|| anyhow!("bad predicate iri"))?;
    let mut body = String::new();
    let mut n = 0;
    for (a, b) in pairs {
        if let (Some(at), Some(bt)) = (vocab::iri_term(a), vocab::iri_term(b)) {
            body.push_str(&format!("{at} {pt} {bt} .\n"));
            n += 1;
        }
    }
    if n > 0 {
        graph.update(&format!("INSERT DATA {{ GRAPH <{RESOLUTION_GRAPH}> {{\n{body}}} }}"))?;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_near_duplicates() {
        let g = Graph::new().unwrap();
        g.update(
            "INSERT DATA { GRAPH <urn:d> {
               <http://ex/s/1> a <http://ex/Supplier> ; <http://www.w3.org/2000/01/rdf-schema#label> \"Acme Ltd\" .
               <http://ex/s/2> a <http://ex/Supplier> ; <http://www.w3.org/2000/01/rdf-schema#label> \"Acme Ltd.\" .
               <http://ex/s/3> a <http://ex/Supplier> ; <http://www.w3.org/2000/01/rdf-schema#label> \"Globex\" .
             } }",
        )
        .unwrap();
        let mut pfx = HashMap::new();
        pfx.insert("ex".into(), "http://ex/".into());
        let cand = candidates(&g, "http://ex/", &pfx, "ex:Supplier", "rdfs:label", 0.9).unwrap();
        assert_eq!(cand["count"], 1);
        assert_eq!(cand["pairs"][0]["a"], "http://ex/s/1");
        let n = apply(&g, "http://ex/", &pfx, "skos:closeMatch",
            &[("http://ex/s/1".into(), "http://ex/s/2".into())]).unwrap();
        assert_eq!(n, 1);
    }
}
