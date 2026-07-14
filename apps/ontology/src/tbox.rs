//! Stage [2] Ontology design (T-Box). Classes and properties are RDFS/OWL
//! triples kept in a dedicated `TBOX_GRAPH` so the schema stays separate from
//! instance data (A-Box) and provenance. Writes go through SPARQL `INSERT
//! DATA`; reads project the graph back into a compact JSON shape for the editor.

use crate::graph::{Graph, TBOX_GRAPH};
use crate::vocab;
use anyhow::{anyhow, Result};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct ClassDef {
    /// Curie or IRI, e.g. `ex:Product`.
    pub iri: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub comment: Option<String>,
    #[serde(default, rename = "subClassOf")]
    pub sub_class_of: Option<String>,
}

#[derive(Deserialize)]
pub struct PropertyDef {
    pub iri: String,
    /// object | data | annotation
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    #[serde(default)]
    pub range: Option<String>,
    #[serde(default, rename = "subPropertyOf")]
    pub sub_property_of: Option<String>,
    #[serde(default, rename = "inverseOf")]
    pub inverse_of: Option<String>,
}

fn default_kind() -> String {
    "object".to_string()
}

/// A full T-Box draft (e.g. from the LLM) applied in one shot.
#[derive(Deserialize, Default)]
pub struct TboxDraft {
    #[serde(default)]
    pub prefixes: HashMap<String, String>,
    #[serde(default)]
    pub classes: Vec<ClassDef>,
    #[serde(default)]
    pub properties: Vec<PropertyDef>,
}

fn triple(s: &str, p: &str, o: &str) -> Option<String> {
    let st = vocab::iri_term(s)?;
    let pt = vocab::iri_term(p)?;
    let ot = vocab::iri_term(o)?;
    Some(format!("{st} {pt} {ot} .\n"))
}

fn triple_lit(s: &str, p: &str, lit: &str) -> Option<String> {
    let st = vocab::iri_term(s)?;
    let pt = vocab::iri_term(p)?;
    Some(format!("{st} {pt} \"{}\" .\n", vocab::escape_literal(lit)))
}

pub fn add_class(
    graph: &Graph,
    base: &str,
    prefixes: &HashMap<String, String>,
    c: &ClassDef,
) -> Result<String> {
    let iri = vocab::expand(&c.iri, prefixes, base);
    let mut body = String::new();
    body.push_str(&triple(&iri, &format!("{}type", vocab::RDF), &format!("{}Class", vocab::OWL)).ok_or_else(|| anyhow!("bad class iri: {}", c.iri))?);
    if let Some(l) = &c.label {
        if let Some(t) = triple_lit(&iri, &format!("{}label", vocab::RDFS), l) {
            body.push_str(&t);
        }
    }
    if let Some(cm) = &c.comment {
        if let Some(t) = triple_lit(&iri, &format!("{}comment", vocab::RDFS), cm) {
            body.push_str(&t);
        }
    }
    if let Some(sup) = &c.sub_class_of {
        let supi = vocab::expand(sup, prefixes, base);
        if let Some(t) = triple(&iri, &format!("{}subClassOf", vocab::RDFS), &supi) {
            body.push_str(&t);
        }
    }
    graph.update(&format!("INSERT DATA {{ GRAPH <{TBOX_GRAPH}> {{\n{body}}} }}"))?;
    Ok(iri)
}

pub fn add_property(
    graph: &Graph,
    base: &str,
    prefixes: &HashMap<String, String>,
    p: &PropertyDef,
) -> Result<String> {
    let iri = vocab::expand(&p.iri, prefixes, base);
    let ty = match p.kind.as_str() {
        "data" => format!("{}DatatypeProperty", vocab::OWL),
        "annotation" => format!("{}AnnotationProperty", vocab::OWL),
        _ => format!("{}ObjectProperty", vocab::OWL),
    };
    let mut body = String::new();
    body.push_str(&triple(&iri, &format!("{}type", vocab::RDF), &ty).ok_or_else(|| anyhow!("bad property iri: {}", p.iri))?);
    if let Some(l) = &p.label {
        if let Some(t) = triple_lit(&iri, &format!("{}label", vocab::RDFS), l) {
            body.push_str(&t);
        }
    }
    for (pred, val) in [
        ("domain", &p.domain),
        ("range", &p.range),
    ] {
        if let Some(v) = val {
            let vi = vocab::expand(v, prefixes, base);
            if let Some(t) = triple(&iri, &format!("{}{}", vocab::RDFS, pred), &vi) {
                body.push_str(&t);
            }
        }
    }
    if let Some(sp) = &p.sub_property_of {
        let spi = vocab::expand(sp, prefixes, base);
        if let Some(t) = triple(&iri, &format!("{}subPropertyOf", vocab::RDFS), &spi) {
            body.push_str(&t);
        }
    }
    if let Some(inv) = &p.inverse_of {
        let ii = vocab::expand(inv, prefixes, base);
        if let Some(t) = triple(&iri, &format!("{}inverseOf", vocab::OWL), &ii) {
            body.push_str(&t);
        }
    }
    graph.update(&format!("INSERT DATA {{ GRAPH <{TBOX_GRAPH}> {{\n{body}}} }}"))?;
    Ok(iri)
}

pub fn apply_draft(
    graph: &Graph,
    base: &str,
    project_prefixes: &HashMap<String, String>,
    draft: &TboxDraft,
) -> Result<(usize, usize)> {
    let mut prefixes = project_prefixes.clone();
    prefixes.extend(draft.prefixes.clone());
    let mut nc = 0;
    let mut np = 0;
    for c in &draft.classes {
        add_class(graph, base, &prefixes, c)?;
        nc += 1;
    }
    for p in &draft.properties {
        add_property(graph, base, &prefixes, p)?;
        np += 1;
    }
    Ok((nc, np))
}

/// Delete a class or property (all triples with it as subject) from the T-Box.
pub fn remove_term(graph: &Graph, base: &str, prefixes: &HashMap<String, String>, iri: &str) -> Result<()> {
    let full = vocab::expand(iri, prefixes, base);
    let term = vocab::iri_term(&full).ok_or_else(|| anyhow!("bad iri"))?;
    graph.update(&format!(
        "DELETE WHERE {{ GRAPH <{TBOX_GRAPH}> {{ {term} ?p ?o }} }}"
    ))?;
    Ok(())
}

/// Read the T-Box back as `{ classes: [...], properties: [...] }`.
pub fn read(graph: &Graph) -> Result<serde_json::Value> {
    let classes = graph.query_json(&format!(
        "{}SELECT ?c ?label ?super WHERE {{ GRAPH <{TBOX_GRAPH}> {{ \
         ?c a owl:Class . OPTIONAL {{ ?c rdfs:label ?label }} OPTIONAL {{ ?c rdfs:subClassOf ?super }} }} }} ORDER BY ?c",
        vocab::PREFIXES
    ))?;
    let props = graph.query_json(&format!(
        "{}SELECT ?p ?kind ?label ?domain ?range WHERE {{ GRAPH <{TBOX_GRAPH}> {{ \
         ?p a ?kind . FILTER(?kind IN (owl:ObjectProperty, owl:DatatypeProperty, owl:AnnotationProperty)) \
         OPTIONAL {{ ?p rdfs:label ?label }} OPTIONAL {{ ?p rdfs:domain ?domain }} OPTIONAL {{ ?p rdfs:range ?range }} }} }} ORDER BY ?p",
        vocab::PREFIXES
    ))?;
    Ok(serde_json::json!({
        "classes": collapse(&classes, "c", &["label", "super"]),
        "properties": collapse(&props, "p", &["kind", "label", "domain", "range"]),
    }))
}

/// Collapse SPARQL rows (which may repeat a subject across optional bindings)
/// into one object per subject with the requested value columns.
fn collapse(result: &serde_json::Value, key: &str, cols: &[&str]) -> serde_json::Value {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, serde_json::Map<String, serde_json::Value>> = BTreeMap::new();
    if let Some(rows) = result["rows"].as_array() {
        for row in rows {
            let iri = row[key]["value"].as_str().unwrap_or("").to_string();
            if iri.is_empty() {
                continue;
            }
            let entry = map.entry(iri.clone()).or_insert_with(|| {
                let mut m = serde_json::Map::new();
                m.insert("iri".into(), iri.clone().into());
                m
            });
            for col in cols {
                if let Some(v) = row[*col]["value"].as_str() {
                    if !v.is_empty() {
                        entry.insert((*col).into(), v.into());
                    }
                }
            }
        }
    }
    serde_json::Value::Array(map.into_values().map(serde_json::Value::Object).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_read_tbox() {
        let g = Graph::new().unwrap();
        let mut pfx = HashMap::new();
        pfx.insert("ex".into(), "http://ex/shop#".into());
        add_class(&g, "http://ex/shop", &pfx, &ClassDef {
            iri: "ex:Product".into(),
            label: Some("Product".into()),
            comment: None,
            sub_class_of: None,
        }).unwrap();
        add_property(&g, "http://ex/shop", &pfx, &PropertyDef {
            iri: "ex:hasPrice".into(),
            kind: "data".into(),
            label: Some("has price".into()),
            domain: Some("ex:Product".into()),
            range: Some("xsd:decimal".into()),
            sub_property_of: None,
            inverse_of: None,
        }).unwrap();
        let t = read(&g).unwrap();
        assert_eq!(t["classes"].as_array().unwrap().len(), 1);
        assert_eq!(t["classes"][0]["label"], "Product");
        assert_eq!(t["properties"].as_array().unwrap().len(), 1);
        assert_eq!(t["properties"][0]["range"], "http://www.w3.org/2001/XMLSchema#decimal");
    }
}
