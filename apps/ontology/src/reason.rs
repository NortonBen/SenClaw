//! Stage [7] Reasoning (materialize). A pragmatic **OWL-RL / RDFS subset**
//! expressed as SPARQL `INSERT` rules run to a fixpoint. Inferred triples land
//! in a dedicated `INFERRED_GRAPH` so they are never confused with asserted
//! data and can be recomputed by clearing that one graph.
//!
//! This is not a full OWL-DL reasoner — it covers subclass/subproperty
//! propagation, domain/range typing, inverse properties, and the `owl:sameAs`
//! equivalence closure (symmetry + transitivity, not full substitution), which
//! answers the bulk of practical competency questions while staying in-process.

use crate::graph::Graph;
use crate::vocab;
use anyhow::Result;

pub const INFERRED_GRAPH: &str = "urn:senclaw:ontology:inferred";

/// Each rule reads across ALL named graphs (`GRAPH ?gN`) — T-Box, data batches,
/// and previously-inferred triples — and writes into `INFERRED_GRAPH`.
fn rules() -> Vec<String> {
    let inf = INFERRED_GRAPH;
    vec![
        // subClassOf transitivity.
        format!("INSERT {{ GRAPH <{inf}> {{ ?c rdfs:subClassOf ?e }} }} WHERE {{ \
                 GRAPH ?g1 {{ ?c rdfs:subClassOf ?d }} GRAPH ?g2 {{ ?d rdfs:subClassOf ?e }} FILTER(?c != ?e) }}"),
        // type propagation up the class hierarchy.
        format!("INSERT {{ GRAPH <{inf}> {{ ?x a ?d }} }} WHERE {{ \
                 GRAPH ?g1 {{ ?x a ?c }} GRAPH ?g2 {{ ?c rdfs:subClassOf ?d }} FILTER(?c != ?d) }}"),
        // subPropertyOf.
        format!("INSERT {{ GRAPH <{inf}> {{ ?x ?q ?y }} }} WHERE {{ \
                 GRAPH ?g1 {{ ?x ?p ?y }} GRAPH ?g2 {{ ?p rdfs:subPropertyOf ?q }} FILTER(?p != ?q) }}"),
        // rdfs:domain typing.
        format!("INSERT {{ GRAPH <{inf}> {{ ?x a ?c }} }} WHERE {{ \
                 GRAPH ?g1 {{ ?x ?p ?y }} GRAPH ?g2 {{ ?p rdfs:domain ?c }} }}"),
        // rdfs:range typing (only for IRI objects).
        format!("INSERT {{ GRAPH <{inf}> {{ ?y a ?c }} }} WHERE {{ \
                 GRAPH ?g1 {{ ?x ?p ?y }} GRAPH ?g2 {{ ?p rdfs:range ?c }} FILTER(isIRI(?y)) }}"),
        // owl:inverseOf (both directions of the declaration).
        format!("INSERT {{ GRAPH <{inf}> {{ ?y ?q ?x }} }} WHERE {{ \
                 GRAPH ?g1 {{ ?x ?p ?y }} GRAPH ?g2 {{ {{ ?p owl:inverseOf ?q }} UNION {{ ?q owl:inverseOf ?p }} }} }}"),
        // owl:sameAs symmetry + transitivity (equivalence closure). We do NOT do
        // full indiscernibility/substitution (copying every triple across equal
        // terms) — that can blow up the graph — so `sameAs` clusters identities
        // rather than merging all their assertions.
        format!("INSERT {{ GRAPH <{inf}> {{ ?b owl:sameAs ?a }} }} WHERE {{ \
                 GRAPH ?g {{ ?a owl:sameAs ?b }} FILTER(?a != ?b) }}"),
        format!("INSERT {{ GRAPH <{inf}> {{ ?a owl:sameAs ?c }} }} WHERE {{ \
                 GRAPH ?g1 {{ ?a owl:sameAs ?b }} GRAPH ?g2 {{ ?b owl:sameAs ?c }} FILTER(?a != ?c) }}"),
    ]
}

/// Recompute the inferred graph from scratch. Returns
/// `{ inferred, iterations }`.
pub fn materialize(graph: &Graph) -> Result<serde_json::Value> {
    graph.update(&format!("DROP SILENT GRAPH <{INFERRED_GRAPH}>"))?;
    let rules = rules();
    let mut iterations = 0;
    let mut prev = graph.len();
    for _ in 0..20 {
        iterations += 1;
        for rule in &rules {
            graph.update(&format!("{}{}", vocab::PREFIXES, rule))?;
        }
        let now = graph.len();
        if now == prev {
            break;
        }
        prev = now;
    }
    // Inferred triples that merely restate asserted ones (e.g. a domain rule
    // re-deriving an already-asserted rdf:type) must not linger in a second
    // graph, or the union-default query would double-count them.
    graph.dedup_graph(INFERRED_GRAPH)?;
    let inferred = graph.graph_len(INFERRED_GRAPH).unwrap_or(0);
    Ok(serde_json::json!({ "inferred": inferred, "iterations": iterations }))
}

/// Clear inferred triples.
pub fn clear(graph: &Graph) -> Result<()> {
    graph.update(&format!("DROP SILENT GRAPH <{INFERRED_GRAPH}>"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subclass_and_domain_inference() {
        let g = Graph::new().unwrap();
        // T-Box: Dog ⊑ Animal ; owns domain Person.
        g.update(
            "INSERT DATA { GRAPH <urn:senclaw:ontology:tbox> {
               <http://ex/Dog> <http://www.w3.org/2000/01/rdf-schema#subClassOf> <http://ex/Animal> .
               <http://ex/owns> <http://www.w3.org/2000/01/rdf-schema#domain> <http://ex/Person> .
             } }",
        )
        .unwrap();
        // Data: rex a Dog ; alice owns rex.
        g.update(
            "INSERT DATA { GRAPH <urn:d> {
               <http://ex/rex> a <http://ex/Dog> .
               <http://ex/alice> <http://ex/owns> <http://ex/rex> .
             } }",
        )
        .unwrap();
        let rep = materialize(&g).unwrap();
        assert!(rep["inferred"].as_u64().unwrap() >= 2);
        // rex is now an Animal; alice is a Person.
        let ask = g
            .query_json("ASK { <http://ex/rex> a <http://ex/Animal> . <http://ex/alice> a <http://ex/Person> }")
            .unwrap();
        assert_eq!(ask["boolean"], true);
    }
}
