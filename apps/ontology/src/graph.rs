//! Thin wrapper over an in-memory Oxigraph `Store`: the app's RDF triple layer.
//!
//! One `Graph` == one ontology project's dataset. Triples live in **named
//! graphs** (one per import batch) so a batch can be dropped/reloaded in
//! isolation (provenance). The T-Box (schema) lives in a dedicated
//! `urn:senclaw:ontology:tbox` graph; PROV-O metadata in
//! `urn:senclaw:ontology:prov`.

use anyhow::{anyhow, Result};
use oxigraph::io::RdfFormat;
use oxigraph::model::{GraphNameRef, NamedNodeRef, QuadRef, Term};
use oxigraph::sparql::{Query, QueryResults, QuerySolution};
use oxigraph::store::Store;
use std::sync::Arc;

pub const TBOX_GRAPH: &str = "urn:senclaw:ontology:tbox";
pub const PROV_GRAPH: &str = "urn:senclaw:ontology:prov";

/// A project's RDF store. Cheap to clone (shared `Store`).
#[derive(Clone)]
pub struct Graph {
    store: Arc<Store>,
}

impl Graph {
    /// Fresh in-memory store.
    pub fn new() -> Result<Self> {
        Ok(Self {
            store: Arc::new(Store::new()?),
        })
    }

    /// Load a Turtle/TriG dump (whole dataset, with named graphs) into the store.
    pub fn load_trig(&self, data: &str) -> Result<()> {
        self.store
            .load_from_reader(RdfFormat::TriG, data.as_bytes())
            .map_err(|e| anyhow!("load trig: {e}"))?;
        Ok(())
    }

    /// Serialize the whole dataset (all named graphs) as TriG for persistence.
    pub fn dump_trig(&self) -> Result<String> {
        let mut buf = Vec::new();
        self.store
            .dump_to_writer(RdfFormat::TriG, &mut buf)
            .map_err(|e| anyhow!("dump trig: {e}"))?;
        Ok(String::from_utf8(buf)?)
    }

    /// Insert one triple into a named graph (typed-model path; most writes go
    /// through SPARQL `INSERT` strings instead).
    #[allow(dead_code)]
    pub fn insert(
        &self,
        subject: NamedNodeRef,
        predicate: NamedNodeRef,
        object: impl Into<Term>,
        graph: GraphNameRef,
    ) -> Result<()> {
        let obj = object.into();
        let quad = QuadRef::new(subject, predicate, &obj, graph);
        self.store
            .insert(quad)
            .map_err(|e| anyhow!("insert: {e}"))?;
        Ok(())
    }

    /// Run a SPARQL SELECT/ASK/CONSTRUCT update-free query. Returns row objects
    /// (`[{var: value, ...}]`) for SELECT, or `[{"result": bool}]` for ASK.
    pub fn query_json(&self, sparql: &str) -> Result<serde_json::Value> {
        // Parse first so we can present the UNION of all named graphs as the
        // default graph — data is lifted into per-batch named graphs, but users
        // (and competency questions) write plain `?s ?p ?o` queries.
        let mut query = Query::parse(sparql, None).map_err(|e| anyhow!("sparql parse: {e}"))?;
        query.dataset_mut().set_default_graph_as_union();
        let results = self
            .store
            .query(query)
            .map_err(|e| anyhow!("sparql query: {e}"))?;
        match results {
            QueryResults::Solutions(solutions) => {
                let vars: Vec<String> = solutions
                    .variables()
                    .iter()
                    .map(|v| v.as_str().to_string())
                    .collect();
                let mut rows = Vec::new();
                for sol in solutions {
                    let sol = sol.map_err(|e| anyhow!("row: {e}"))?;
                    rows.push(solution_to_json(&sol));
                }
                Ok(serde_json::json!({ "head": vars, "rows": rows }))
            }
            QueryResults::Boolean(b) => Ok(serde_json::json!({ "boolean": b })),
            QueryResults::Graph(triples) => {
                let mut out = Vec::new();
                for t in triples {
                    let t = t.map_err(|e| anyhow!("triple: {e}"))?;
                    out.push(serde_json::json!({
                        "s": t.subject.to_string(),
                        "p": t.predicate.to_string(),
                        "o": term_to_json(&t.object.into()),
                    }));
                }
                Ok(serde_json::json!({ "triples": out }))
            }
        }
    }

    /// Run a SPARQL 1.1 Update (INSERT/DELETE/DROP...).
    pub fn update(&self, sparql: &str) -> Result<()> {
        self.store
            .update(sparql)
            .map_err(|e| anyhow!("sparql update: {e}"))?;
        Ok(())
    }

    /// Remove from `target` every triple that also exists in some *other* named
    /// graph. Used ONLY for the inferred graph: Oxigraph's union-default-graph is
    /// a BAG across graphs, so an inferred triple that merely restates an asserted
    /// one (e.g. a domain rule re-deriving an existing `rdf:type`) would produce
    /// duplicate solutions. Inferred restatements carry no information, so pruning
    /// them is safe. This is deliberately NOT applied to data batches — doing so
    /// would let dropping one batch delete triples another batch also asserts,
    /// breaking provenance isolation.
    pub fn dedup_graph(&self, target: &str) -> Result<()> {
        self.update(&format!(
            "DELETE {{ GRAPH <{target}> {{ ?s ?p ?o }} }} WHERE {{ \
             GRAPH <{target}> {{ ?s ?p ?o }} GRAPH ?g {{ ?s ?p ?o }} FILTER(?g != <{target}>) }}"
        ))
    }

    /// Count all triples across all graphs.
    pub fn len(&self) -> usize {
        self.store.len().unwrap_or(0)
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of triples in a specific named graph.
    pub fn graph_len(&self, graph: &str) -> Result<usize> {
        let q = format!("SELECT (COUNT(*) AS ?n) WHERE {{ GRAPH <{graph}> {{ ?s ?p ?o }} }}");
        let v = self.query_json(&q)?;
        Ok(v["rows"][0]["n"]["value"]
            .as_str()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    }
}

/// Convert a SPARQL solution row to `{var: {type, value, datatype?}}`.
fn solution_to_json(sol: &QuerySolution) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (var, term) in sol.iter() {
        map.insert(var.as_str().to_string(), term_to_json(term));
    }
    serde_json::Value::Object(map)
}

/// SPARQL-JSON-ish binding for one term.
fn term_to_json(term: &Term) -> serde_json::Value {
    match term {
        Term::NamedNode(n) => serde_json::json!({ "type": "uri", "value": n.as_str() }),
        Term::BlankNode(b) => serde_json::json!({ "type": "bnode", "value": b.as_str() }),
        Term::Literal(l) => {
            let mut o = serde_json::Map::new();
            o.insert("type".into(), "literal".into());
            o.insert("value".into(), l.value().into());
            if let Some(lang) = l.language() {
                o.insert("lang".into(), lang.into());
            }
            o.insert("datatype".into(), l.datatype().as_str().into());
            serde_json::Value::Object(o)
        }
        Term::Triple(_) => serde_json::json!({ "type": "triple", "value": term.to_string() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxigraph::model::{vocab::rdf, Literal, NamedNode};

    #[test]
    fn dedup_makes_union_a_set() {
        let g = Graph::new().unwrap();
        g.update("INSERT DATA { GRAPH <urn:a> { <urn:s> a <urn:C> } }")
            .unwrap();
        g.update("INSERT DATA { GRAPH <urn:b> { <urn:s> a <urn:C> } }")
            .unwrap();
        // Before dedup the union-default query double-counts (BAG semantics).
        let before = g.query_json("SELECT ?s WHERE { ?s a <urn:C> }").unwrap();
        assert_eq!(before["rows"].as_array().unwrap().len(), 2);
        g.dedup_graph("urn:b").unwrap();
        let after = g.query_json("SELECT ?s WHERE { ?s a <urn:C> }").unwrap();
        assert_eq!(after["rows"].as_array().unwrap().len(), 1);
        // urn:a keeps the triple; urn:b lost its duplicate.
        assert_eq!(g.graph_len("urn:a").unwrap(), 1);
        assert_eq!(g.graph_len("urn:b").unwrap(), 0);
    }

    #[test]
    fn insert_query_roundtrip() {
        let g = Graph::new().unwrap();
        let s = NamedNode::new("http://ex/prod/1").unwrap();
        let ty = NamedNode::new("http://ex/Product").unwrap();
        let price = NamedNode::new("http://ex/price").unwrap();
        let data = GraphNameRef::DefaultGraph;
        g.insert(s.as_ref(), rdf::TYPE, ty.as_ref(), data).unwrap();
        g.insert(
            s.as_ref(),
            price.as_ref(),
            Literal::new_simple_literal("150000"),
            data,
        )
        .unwrap();
        assert_eq!(g.len(), 2);
        let v = g
            .query_json("SELECT ?p ?o WHERE { <http://ex/prod/1> ?p ?o }")
            .unwrap();
        assert_eq!(v["rows"].as_array().unwrap().len(), 2);
    }
}
