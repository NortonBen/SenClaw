//! Stage [7] Provenance. Every lift/import goes into its own **named graph**
//! (a "batch"), and a PROV-O record describing it is written to `PROV_GRAPH`.
//! Because each batch is isolated, one import can be dropped and reloaded
//! (`DROP GRAPH`) without touching any other data — the thing a pipeline
//! *without* provenance cannot do.

use crate::graph::{Graph, PROV_GRAPH};
use crate::vocab;
use anyhow::{anyhow, Result};

/// Unique batch graph IRI for an import at `ts` from `label`. A process-global
/// monotonic counter is appended so two imports in the same second (or with the
/// same label) never collide into one named graph — which would merge their data
/// and make dropping one remove both.
pub fn batch_iri(ts: i64, label: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("urn:senclaw:ontology:batch:{ts}-{}-{n}", vocab::encode_segment(label))
}

const GENERATED_AT: &str = "urn:senclaw:ontology:prop:generatedAt"; // epoch secs (xsd:integer)
const TRIPLE_COUNT: &str = "urn:senclaw:ontology:prop:tripleCount";
const ACTIVITY: &str = "urn:senclaw:ontology:prop:activity";

/// Convert Unix seconds (UTC) to an `xsd:dateTime` string, no chrono dependency.
pub fn epoch_to_iso(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, mi, s) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    // days since 1970-01-01 → civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Write a PROV-O record for a completed batch.
#[allow(clippy::too_many_arguments)]
pub fn record_batch(
    graph: &Graph,
    batch: &str,
    label: &str,
    source: &str,
    activity: &str,
    triple_count: usize,
    ts: i64,
) -> Result<()> {
    let b = vocab::iri_term(batch).ok_or_else(|| anyhow!("bad batch iri"))?;
    let mut body = String::new();
    body.push_str(&format!("{b} <{}type> <{}Entity> .\n", vocab::RDF, vocab::PROV));
    body.push_str(&format!("{b} <{}label> \"{}\" .\n", vocab::RDFS, vocab::escape_literal(label)));
    if !source.is_empty() {
        body.push_str(&format!("{b} <{}wasDerivedFrom> \"{}\" .\n", vocab::PROV, vocab::escape_literal(source)));
    }
    body.push_str(&format!("{b} <{ACTIVITY}> \"{}\" .\n", vocab::escape_literal(activity)));
    body.push_str(&format!(
        "{b} <{}generatedAtTime> \"{}\"^^<{}dateTime> .\n",
        vocab::PROV,
        epoch_to_iso(ts),
        vocab::XSD
    ));
    body.push_str(&format!("{b} <{GENERATED_AT}> \"{ts}\"^^<{}integer> .\n", vocab::XSD));
    body.push_str(&format!("{b} <{TRIPLE_COUNT}> \"{triple_count}\"^^<{}integer> .\n", vocab::XSD));
    graph.update(&format!("INSERT DATA {{ GRAPH <{PROV_GRAPH}> {{\n{body}}} }}"))?;
    Ok(())
}

/// List all batches with live triple counts.
pub fn list_batches(graph: &Graph) -> Result<serde_json::Value> {
    let q = format!(
        "{}SELECT ?b ?label ?src ?act ?at WHERE {{ GRAPH <{PROV_GRAPH}> {{ \
         ?b a prov:Entity . OPTIONAL {{ ?b rdfs:label ?label }} \
         OPTIONAL {{ ?b prov:wasDerivedFrom ?src }} OPTIONAL {{ ?b <{ACTIVITY}> ?act }} \
         OPTIONAL {{ ?b prov:generatedAtTime ?at }} }} }} ORDER BY DESC(?at)",
        vocab::PREFIXES
    );
    let res = graph.query_json(&q)?;
    let mut out = Vec::new();
    if let Some(rows) = res["rows"].as_array() {
        for r in rows {
            let iri = r["b"]["value"].as_str().unwrap_or("").to_string();
            if iri.is_empty() {
                continue;
            }
            let live = graph.graph_len(&iri).unwrap_or(0);
            out.push(serde_json::json!({
                "iri": iri,
                "label": r["label"]["value"].as_str().unwrap_or(""),
                "source": r["src"]["value"].as_str().unwrap_or(""),
                "activity": r["act"]["value"].as_str().unwrap_or(""),
                "generatedAt": r["at"]["value"].as_str().unwrap_or(""),
                "tripleCount": live,
            }));
        }
    }
    Ok(serde_json::Value::Array(out))
}

/// Drop a batch's data graph and its PROV record.
pub fn drop_batch(graph: &Graph, batch: &str) -> Result<()> {
    let b = vocab::iri_term(batch).ok_or_else(|| anyhow!("bad batch iri"))?;
    graph.update(&format!("DROP SILENT GRAPH <{batch}>"))?;
    graph.update(&format!("DELETE WHERE {{ GRAPH <{PROV_GRAPH}> {{ {b} ?p ?o }} }}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_format() {
        // 2021-01-01T00:00:00Z = 1609459200
        assert_eq!(epoch_to_iso(1_609_459_200), "2021-01-01T00:00:00Z");
        // 2026-07-14T00:00:00Z = 1783987200
        assert_eq!(epoch_to_iso(1_783_987_200), "2026-07-14T00:00:00Z");
    }

    #[test]
    fn record_list_drop() {
        let g = Graph::new().unwrap();
        let b = batch_iri(1_784_937_600, "products.csv");
        g.update(&format!("INSERT DATA {{ GRAPH <{b}> {{ <urn:x> <urn:p> \"v\" }} }}")).unwrap();
        record_batch(&g, &b, "products.csv", "products.csv", "lift", 1, 1_784_937_600).unwrap();
        let list = list_batches(&g).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
        assert_eq!(list[0]["tripleCount"], 1);
        drop_batch(&g, &b).unwrap();
        assert_eq!(g.graph_len(&b).unwrap(), 0);
        assert_eq!(list_batches(&g).unwrap().as_array().unwrap().len(), 0);
    }
}
