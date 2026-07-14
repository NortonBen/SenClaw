//! LLM assists — every call goes through the SenClaw daemon's Space-App bridge
//! (`SpaceClient`), so the app never touches provider keys. The LLM *drafts*
//! (roles, T-Box, mapping, SHACL shapes, SPARQL, extracted triples); a human
//! reviews and the deterministic pipeline does the actual lifting. Keeping the
//! mapping declarative + human-approved is what keeps the KG auditable.

use app_space_sdk::SpaceClient;
use serde_json::Value;

fn client() -> SpaceClient {
    SpaceClient::from_env()
}

/// Pull the first JSON object/array out of an LLM reply (tolerates ``` fences
/// and surrounding prose).
pub fn extract_json(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return Some(v);
    }
    // Strip code fences.
    let cleaned = t
        .trim_start_matches("```json")
        .trim_start_matches("```JSON")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(v) = serde_json::from_str::<Value>(cleaned) {
        return Some(v);
    }
    // Find the first balanced { } or [ ] span.
    let bytes = cleaned.as_bytes();
    for (open, close) in [(b'{', b'}'), (b'[', b']')] {
        if let Some(start) = bytes.iter().position(|&b| b == open) {
            let mut depth = 0i32;
            let mut in_str = false;
            let mut esc = false;
            for i in start..bytes.len() {
                let b = bytes[i];
                if in_str {
                    if esc {
                        esc = false;
                    } else if b == b'\\' {
                        esc = true;
                    } else if b == b'"' {
                        in_str = false;
                    }
                    continue;
                }
                match b {
                    b'"' => in_str = true,
                    x if x == open => depth += 1,
                    x if x == close => {
                        depth -= 1;
                        if depth == 0 {
                            if let Ok(v) = serde_json::from_str::<Value>(&cleaned[start..=i]) {
                                return Some(v);
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

async fn ask_json(system: &str, prompt: &str, max_tokens: u32) -> Result<(Value, String), String> {
    let (text, model) = client()
        .llm_request(system, prompt, max_tokens)
        .await
        .map_err(|e| e.to_string())?;
    match extract_json(&text) {
        Some(v) => Ok((v, model)),
        None => Err(format!("LLM did not return valid JSON. Raw reply:\n{text}")),
    }
}

const ROLES_SYS: &str = "You are a data-to-ontology profiler. Given a table's column profiles, \
classify each column's ONTOLOGY ROLE. Return ONLY JSON, no prose, shape: \
{\"columns\":[{\"name\":\"...\",\"role\":\"identifier|relation|attribute|enum|ignore\",\"suggestedClass\":\"\",\"note\":\"short reason\"}]}. \
Rules: a unique key column => identifier; a foreign-key-like column => relation; a low-cardinality categorical => enum \
(model as SKOS individuals, NOT subclasses); free numeric/text/date => attribute; junk/derived => ignore. \
Reply notes in the same language as the column names.";

pub async fn profile_roles(columns: &Value) -> Result<(Value, String), String> {
    let prompt = format!(
        "Column profiles (JSON):\n{}\n\nReturn the roles JSON.",
        serde_json::to_string_pretty(columns).unwrap_or_default()
    );
    ask_json(ROLES_SYS, &prompt, 1200).await
}

const TBOX_SYS: &str = "You are an ontology engineer. Design a T-Box (schema) that answers the given \
COMPETENCY QUESTIONS over the profiled data. Return ONLY JSON, shape: \
{\"prefixes\":{\"ex\":\"http://senclaw.local/onto/shop#\"},\
\"classes\":[{\"iri\":\"ex:Product\",\"label\":\"Product\",\"subClassOf\":\"\"}],\
\"properties\":[{\"iri\":\"ex:hasPrice\",\"kind\":\"data|object|annotation\",\"label\":\"has price\",\"domain\":\"ex:Product\",\"range\":\"xsd:decimal\"}]}. \
Principles: one row can hold SEVERAL entities — model each as a class; a column that repeats a value is ONE individual, \
not many; an enum column becomes SKOS individuals, not classes; a relation that itself has attributes (price/date on a \
link) needs an intermediate REIFICATION class. Prefer object properties between classes and data properties (xsd ranges) \
for literals. Use the 'ex' prefix for domain terms.";

pub async fn draft_tbox(competency: &[String], columns: &Value, base: &str) -> Result<(Value, String), String> {
    let prompt = format!(
        "Base IRI: {base}\nCompetency questions:\n- {}\n\nProfiled columns (JSON):\n{}\n\nReturn the T-Box JSON.",
        competency.join("\n- "),
        serde_json::to_string_pretty(columns).unwrap_or_default()
    );
    ask_json(TBOX_SYS, &prompt, 2000).await
}

const MAPPING_SYS: &str = "You author declarative RML-lite mappings for the SenClaw Ontology app. \
Return ONLY JSON in exactly this DSL shape: \
{\"base\":\"...\",\"prefixes\":{\"ex\":\"...\"},\"triplesMaps\":[{\"name\":\"ProductMap\",\"source\":\"<sourceName>\",\
\"subject\":{\"template\":\"product/{sku}\",\"class\":\"ex:Product\"},\
\"predicateObjectMaps\":[{\"predicate\":\"rdfs:label\",\"object\":{\"column\":\"name\"}},\
{\"predicate\":\"ex:hasPrice\",\"object\":{\"column\":\"price\",\"datatype\":\"xsd:decimal\"}},\
{\"predicate\":\"ex:hasSupplier\",\"object\":{\"template\":\"supplier/{supplier_id}\"}}]}]}. \
Object forms: {column,datatype?,lang?} literal; {template} IRI to another entity by key; {parentHash:[cols],parentSeg} \
IRI for a keyless referenced entity; {constant,iri?}. Subject forms: {template} with a natural key, or {hash:[cols],seg} \
when there is no stable key. Use the SAME source name as given. Do NOT invent columns that are not in the profile.";

pub async fn draft_mapping(
    columns: &Value,
    tbox: &Value,
    source_name: &str,
    base: &str,
) -> Result<(Value, String), String> {
    let prompt = format!(
        "Base IRI: {base}\nSource name: {source_name}\nProfiled columns (JSON):\n{}\n\nT-Box (JSON):\n{}\n\nReturn the mapping JSON.",
        serde_json::to_string_pretty(columns).unwrap_or_default(),
        serde_json::to_string_pretty(tbox).unwrap_or_default()
    );
    ask_json(MAPPING_SYS, &prompt, 2000).await
}

const SPARQL_SYS: &str = "You translate a natural-language question into ONE SPARQL 1.1 query over the given \
ontology. Return ONLY the SPARQL query text — no prose, no markdown fences. Assume the default graph is the union of all \
data. These prefixes are already declared, do not redeclare rdf/rdfs/owl/xsd/skos/prov/sh, but DO add domain prefixes you \
use. Keep it a SELECT with a small LIMIT unless a COUNT/ASK fits better.";

pub async fn nl_to_sparql(question: &str, tbox: &Value) -> Result<(String, String), String> {
    let prompt = format!(
        "Ontology T-Box (JSON):\n{}\n\nQuestion: {question}\n\nSPARQL:",
        serde_json::to_string_pretty(tbox).unwrap_or_default()
    );
    let (text, model) = client()
        .llm_request(SPARQL_SYS, &prompt, 800)
        .await
        .map_err(|e| e.to_string())?;
    // Strip accidental fences.
    let q = text
        .trim()
        .trim_start_matches("```sparql")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string();
    Ok((q, model))
}

const SHAPES_SYS: &str = "You author SHACL-lite shapes for the SenClaw Ontology validator. Return ONLY JSON: \
{\"nodeShapes\":[{\"targetClass\":\"ex:Product\",\"properties\":[{\"path\":\"ex:hasPrice\",\"datatype\":\"xsd:decimal\",\
\"minCount\":1,\"maxCount\":1,\"minInclusive\":0},{\"path\":\"ex:hasSupplier\",\"class\":\"ex:Supplier\",\"maxCount\":1,\
\"nodeKind\":\"IRI\"}]}]}. Supported per-property keys: datatype, class, nodeKind(IRI|Literal|BlankNode), minCount, \
maxCount, minInclusive, maxInclusive, pattern (a regex string). Derive constraints from the T-Box: required properties \
=> minCount 1; functional-looking => maxCount 1; xsd ranges => datatype; object ranges => class + nodeKind IRI.";

pub async fn draft_shapes(tbox: &Value) -> Result<(Value, String), String> {
    let prompt = format!(
        "T-Box (JSON):\n{}\n\nReturn the SHACL-lite shapes JSON.",
        serde_json::to_string_pretty(tbox).unwrap_or_default()
    );
    ask_json(SHAPES_SYS, &prompt, 1600).await
}

const EXTRACT_SYS: &str = "You extract RDF triples from unstructured text, mapping to the given ontology where possible. \
Return ONLY JSON: {\"triples\":[{\"s\":\"subjectLabelOrIri\",\"p\":\"ex:relation\",\"o\":\"objectLabelOrIri\",\
\"oIsLiteral\":true,\"confidence\":0.0}]}. Use ontology predicates when they fit, else a reasonable ex: predicate. Set \
oIsLiteral true for attribute values (numbers, names, dates), false when the object is another entity. NEVER invent facts \
not stated in the text. Give a confidence 0..1 per triple.";

pub async fn extract_triples(text: &str, tbox: &Value) -> Result<(Value, String), String> {
    let prompt = format!(
        "Ontology T-Box (JSON):\n{}\n\nSource text:\n\"\"\"\n{}\n\"\"\"\n\nReturn the triples JSON.",
        serde_json::to_string_pretty(tbox).unwrap_or_default(),
        text.chars().take(6000).collect::<String>()
    );
    ask_json(EXTRACT_SYS, &prompt, 2000).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_extraction() {
        assert_eq!(extract_json("{\"a\":1}").unwrap()["a"], 1);
        assert_eq!(extract_json("```json\n{\"a\":2}\n```").unwrap()["a"], 2);
        assert_eq!(
            extract_json("Sure! Here it is:\n{\"a\": {\"b\": 3}}\nHope that helps").unwrap()["a"]["b"],
            3
        );
        assert!(extract_json("[1,2,3]").unwrap().is_array());
        assert!(extract_json("no json here").is_none());
    }
}
