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

/// Process-wide LLM profile this app composes with — a config **id** or its
/// **label** from SenClaw's Settings → Models. Empty = follow the daemon's
/// active model, which the agent and every other Space App share.
///
/// Held in a cell (not read from env each call) so the UI can change it at
/// runtime with no restart; it is seeded once at startup from the persisted
/// `llm_profile` setting — see [`crate::api::make_state`] — which itself falls
/// back to `ONTOLOGY_LLM_PROFILE` on first run. This mirrors moltbook /
/// rewrite-story so every Space App picks a model the same way.
fn profile_cell() -> &'static std::sync::RwLock<String> {
    static CELL: std::sync::OnceLock<std::sync::RwLock<String>> = std::sync::OnceLock::new();
    CELL.get_or_init(|| std::sync::RwLock::new(String::new()))
}

pub fn set_profile(p: &str) {
    if let Ok(mut w) = profile_cell().write() {
        *w = p.trim().to_string();
    }
}

pub fn profile() -> Option<String> {
    profile_cell()
        .read()
        .ok()
        .map(|r| r.clone())
        .filter(|s| !s.is_empty())
}

/// Configured LLMs in the daemon (`GET /api/llm-config`) so the UI can offer a
/// picker. Returns the raw daemon shape `{ activeId, configs: [{id,label,…}] }`
/// verbatim — the SDK's typed `list_models` drops the `label`, which is exactly
/// what a human-facing picker needs, so this reads the endpoint directly.
pub async fn list_models() -> Result<Value, String> {
    let url = format!("{}/api/llm-config", client().base_url.trim_end_matches('/'));
    reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| format!("cannot reach daemon: {e}"))?
        .json::<Value>()
        .await
        .map_err(|e| format!("parse llm-config: {e}"))
}

/// Every completion in this app goes through here, so the profile is applied
/// once rather than remembered at eight call sites. Returns
/// `(text, model, finish_reason)`.
async fn complete_full(
    system: &str,
    prompt: &str,
    max_tokens: u32,
) -> Result<(String, String, String), String> {
    client()
        .llm_request_full(system, prompt, max_tokens, profile().as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Prose completions, where a reply cut short is degraded but still useful.
async fn complete(system: &str, prompt: &str, max_tokens: u32) -> Result<(String, String), String> {
    complete_full(system, prompt, max_tokens)
        .await
        .map(|(t, m, _)| (t, m))
}

/// Token budgets are sized for **reasoning** models, which spend the same cap
/// on a hidden trace before emitting a single visible character. A budget that
/// is generous for a plain chat model returns JSON chopped mid-string from a
/// reasoning one — so these are deliberately several times what the visible
/// output needs.
const fn budget(visible: u32) -> u32 {
    visible * 4
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

/// Public JSON completion for other modules (AIP Logic). On an **explicit**
/// profile when `Some`, else the app's selected model. Evals use the explicit
/// form to run a case on a specific model *without* mutating
/// the process-wide profile cell — mutating it would race with every other
/// request while an eval is switching models, and a crash mid-run would leave
/// the whole app pointed at the wrong model.
pub async fn ask_json_on(
    system: &str,
    prompt: &str,
    max_tokens: u32,
    profile_override: Option<&str>,
) -> Result<(Value, String), String> {
    match profile_override {
        Some(p) => {
            let (text, model, finish) = client()
                .llm_request_full(system, prompt, budget(max_tokens), Some(p))
                .await
                .map_err(|e| e.to_string())?;
            finish_or_json(text, model, finish)
        }
        None => ask_json(system, prompt, max_tokens).await,
    }
}

/// Shared JSON-or-truncation handling for a raw completion.
fn finish_or_json(text: String, model: String, finish: String) -> Result<(Value, String), String> {
    if let Some(v) = extract_json(&text) {
        return Ok((v, model));
    }
    if finish == "length" {
        return Err(format!(
            "{model} hit the token limit before finishing its JSON (a reasoning model spends the cap on its \
             hidden trace first). Truncated reply:\n{text}"
        ));
    }
    Err(format!(
        "{model} did not return valid JSON. Raw reply:\n{text}"
    ))
}

/// Cap source text put into a prompt (extraction operates chunk-by-chunk; this
/// is the per-call ceiling).
pub fn truncate_for_prompt(text: &str) -> String {
    text.chars().take(CHUNK_CHARS).collect()
}

async fn ask_json(system: &str, prompt: &str, max_tokens: u32) -> Result<(Value, String), String> {
    let (text, model, finish) = complete_full(system, prompt, budget(max_tokens)).await?;
    if let Some(v) = extract_json(&text) {
        return Ok((v, model));
    }
    // Distinguish "the model was cut off" from "the model answered badly".
    // They look identical in the raw text but need opposite fixes, and the
    // truncation case is silent unless the finish reason is checked.
    if finish == "length" {
        return Err(format!(
            "{model} hit the token limit before finishing its JSON — raise the budget, or point \
             ONTOLOGY_LLM_PROFILE at a non-reasoning model (a reasoning model spends the same cap \
             on its hidden trace first). Truncated reply:\n{text}"
        ));
    }
    Err(format!(
        "{model} did not return valid JSON. Raw reply:\n{text}"
    ))
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

pub async fn draft_tbox(
    competency: &[String],
    columns: &Value,
    base: &str,
) -> Result<(Value, String), String> {
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
use. Keep it a SELECT with a small LIMIT unless a COUNT/ASK fits better. \
The LIVE SCHEMA lists the classes and predicates that actually occur in the data, with counts — prefer those exact IRIs \
over anything inferred from the T-Box alone, and never use a predicate that is not listed. When you project an entity, \
also project its rdfs:label if one exists.";

/// Strip the markdown fence the model sometimes wraps a query in.
fn strip_fences(text: &str) -> String {
    text.trim()
        .trim_start_matches("```sparql")
        .trim_start_matches("```SPARQL")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim()
        .to_string()
}

/// NL → SPARQL. `live` is the *observed* schema (classes/predicates actually
/// present, with counts). Grounding on it, not only on the designed T-Box, is
/// what stops the model querying predicates no triple actually uses.
pub async fn nl_to_sparql(
    question: &str,
    tbox: &Value,
    live: &Value,
) -> Result<(String, String), String> {
    let prompt = format!(
        "Ontology T-Box (JSON):\n{}\n\nLIVE SCHEMA (what the data really contains):\n{}\n\nQuestion: {question}\n\nSPARQL:",
        serde_json::to_string_pretty(tbox).unwrap_or_default(),
        serde_json::to_string_pretty(live).unwrap_or_default()
    );
    let (text, model) = complete(SPARQL_SYS, &prompt, budget(800)).await?;
    Ok((strip_fences(&text), model))
}

/// Second chance for a query that failed to parse or came back empty: the exact
/// problem is fed back so the model corrects itself instead of the user having
/// to read SPARQL.
pub async fn repair_sparql(
    question: &str,
    live: &Value,
    bad: &str,
    problem: &str,
) -> Result<(String, String), String> {
    let prompt = format!(
        "LIVE SCHEMA (classes/predicates actually in the data):\n{}\n\nQuestion: {question}\n\n\
         This query was wrong:\n{bad}\n\nProblem: {problem}\n\n\
         Return a corrected SPARQL query using ONLY IRIs from the live schema. SPARQL:",
        serde_json::to_string_pretty(live).unwrap_or_default()
    );
    let (text, model) = complete(SPARQL_SYS, &prompt, budget(800)).await?;
    Ok((strip_fences(&text), model))
}

const ANSWER_SYS: &str = "You answer a question from SPARQL result rows over a knowledge graph. Reply in the SAME \
LANGUAGE as the question, in 1-4 short sentences of plain prose (a compact list is fine when the rows are a list). Quote \
the numbers exactly as they appear — never round, never invent a row that is not there. If there are no rows, say the \
graph holds no matching data and suggest what to load. Do not mention SPARQL, IRIs or 'rows' unless asked.";

const NO_DATA_SYS: &str = "Say, in the SAME LANGUAGE as the question you are given, that the knowledge graph holds no \
data matching it, and suggest loading the relevant source. ONE short sentence. This is a translation task: do NOT answer \
the question, do NOT guess at what the answer might be, do NOT invent any entity, number or name.";

/// The empty-result case is handled separately and never as an "answer".
///
/// An LLM handed zero rows and asked to answer a question is being invited to
/// fabricate — so it is not asked to. It only renders a fixed statement in the
/// user's language, and if even that fails we fall back to plain English.
pub async fn no_data_answer(question: &str) -> String {
    match complete(NO_DATA_SYS, question, budget(200)).await {
        Ok((text, _)) if !text.trim().is_empty() => text.trim().to_string(),
        _ => "The knowledge graph holds no data matching that question yet — load the relevant source and build it first.".to_string(),
    }
}

/// Turn result rows into a human answer — the last mile that makes the graph
/// usable by someone who does not know SPARQL exists. Callers must not reach
/// here with an empty row set; use [`no_data_answer`] instead.
pub async fn answer_from_rows(
    question: &str,
    rows: &Value,
    total: usize,
) -> Result<(String, String), String> {
    let sample = match rows.as_array() {
        Some(a) => Value::Array(a.iter().take(60).cloned().collect()),
        None => rows.clone(),
    };
    let prompt = format!(
        "Question: {question}\n\nResult rows ({total} total{}):\n{}\n\nAnswer:",
        if total > 60 { ", first 60 shown" } else { "" },
        serde_json::to_string(&sample).unwrap_or_default()
    );
    complete(ANSWER_SYS, &prompt, budget(700)).await
}

/// One raw completion for AIP Assist. The system prompt and the retrieved
/// passages are assembled in `aip.rs` — this is only the transport.
pub async fn assist_answer(system: &str, prompt: &str) -> Result<(String, String), String> {
    complete(system, prompt, budget(900)).await
}

const CQ_SYS: &str = "You are an ontology engineer starting from raw data. Write the COMPETENCY QUESTIONS the knowledge \
graph must be able to answer — the acceptance test the ontology is designed against. Return ONLY JSON: \
{\"questions\":[\"...\"]}. Give 5 to 8 questions, each answerable from the columns shown and nothing else. Mix lookup, \
aggregation and relationship questions. Write them in the SAME LANGUAGE as the column names and sample values.";

/// Derive competency questions from the profiled sources, so a user who just
/// dropped a file still gets the design-from-questions discipline instead of a
/// schema transliterated from column names.
pub async fn draft_competency(sources: &Value) -> Result<(Vec<String>, String), String> {
    let prompt = format!(
        "Profiled sources (JSON):\n{}\n\nReturn the questions JSON.",
        serde_json::to_string_pretty(sources).unwrap_or_default()
    );
    let (v, model) = ask_json(CQ_SYS, &prompt, 900).await?;
    let qs = v["questions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .filter(|s| !s.trim().is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok((qs, model))
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
        truncate_chars(text, CHUNK_CHARS)
    );
    ask_json(EXTRACT_SYS, &prompt, 2000).await
}

/// Characters of source text per extraction call. Deliberately small: the
/// bridge returns a roughly fixed amount of output regardless of input size, so
/// an oversized chunk does not produce more triples — it produces a *summary*
/// of the chunk instead, silently dropping facts.
pub const CHUNK_CHARS: usize = 3000;

fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Split text into extraction-sized chunks on paragraph boundaries, falling
/// back to sentence and then character boundaries. Char-based throughout, so
/// Vietnamese (and any other multi-byte text) can never be cut mid-codepoint.
pub fn chunk_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0usize;
    let push = |chunks: &mut Vec<String>, cur: &mut String| {
        let t = cur.trim().to_string();
        if !t.is_empty() {
            chunks.push(t);
        }
        cur.clear();
    };
    for para in text.split("\n\n") {
        let p = para.trim();
        if p.is_empty() {
            continue;
        }
        let plen = p.chars().count();
        if plen > max_chars {
            push(&mut chunks, &mut cur);
            cur_len = 0;
            // Long paragraph: break on sentence enders, then hard-cut.
            let mut piece = String::new();
            let mut piece_len = 0usize;
            for sentence in split_sentences(p, max_chars) {
                let slen = sentence.chars().count();
                if piece_len + slen > max_chars && piece_len > 0 {
                    chunks.push(std::mem::take(&mut piece).trim().to_string());
                    piece_len = 0;
                }
                piece.push_str(&sentence);
                piece.push(' ');
                piece_len += slen + 1;
            }
            let tail = piece.trim().to_string();
            if !tail.is_empty() {
                chunks.push(tail);
            }
            continue;
        }
        if cur_len + plen > max_chars && cur_len > 0 {
            push(&mut chunks, &mut cur);
            cur_len = 0;
        }
        cur.push_str(p);
        cur.push_str("\n\n");
        cur_len += plen + 2;
    }
    push(&mut chunks, &mut cur);
    chunks
}

/// Sentence-ish split that never returns a piece longer than `max_chars`.
fn split_sentences(p: &str, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut len = 0usize;
    for c in p.chars() {
        cur.push(c);
        len += 1;
        if matches!(c, '.' | '!' | '?' | '。' | '\n') || len >= max_chars {
            out.push(std::mem::take(&mut cur));
            len = 0;
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// Extract triples from a whole document by chunking it. Returns the merged
/// triple list (deduplicated on subject/predicate/object), the model used, and
/// how many chunks were processed. A chunk that fails does not sink the run —
/// its error is collected and the rest still lands.
pub async fn extract_triples_chunked(
    text: &str,
    tbox: &Value,
    max_chunks: usize,
) -> Result<(Vec<Value>, String, usize, Vec<String>), String> {
    let chunks = chunk_text(text, CHUNK_CHARS);
    let total = chunks.len().min(max_chunks);
    if total == 0 {
        return Err("no text to extract from".into());
    }
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut merged: Vec<Value> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut model = String::new();
    for (i, chunk) in chunks.into_iter().take(total).enumerate() {
        match extract_triples(&chunk, tbox).await {
            Ok((v, m)) => {
                if model.is_empty() {
                    model = m;
                }
                if let Some(arr) = v["triples"].as_array() {
                    for t in arr {
                        let key = format!(
                            "{}\u{1}{}\u{1}{}",
                            t["s"].as_str().unwrap_or(""),
                            t["p"].as_str().unwrap_or(""),
                            t["o"].as_str().unwrap_or("")
                        );
                        if seen.insert(key) {
                            merged.push(t.clone());
                        }
                    }
                }
            }
            Err(e) => errors.push(format!("chunk {}: {e}", i + 1)),
        }
    }
    if merged.is_empty() && !errors.is_empty() {
        return Err(errors.join("; "));
    }
    Ok((merged, model, total, errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_on_paragraph_boundaries() {
        let doc = (0..10)
            .map(|i| format!("Đoạn số {i}. ").repeat(20))
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk_text(&doc, 400);
        assert!(chunks.len() > 1);
        for c in &chunks {
            // The cap is per-paragraph-group; a single paragraph may exceed it
            // only after the sentence split, never by an unbounded amount.
            assert!(
                c.chars().count() <= 500,
                "chunk too long: {}",
                c.chars().count()
            );
            assert!(!c.trim().is_empty());
        }
        // Nothing is lost: every paragraph index still appears somewhere.
        let joined = chunks.join(" ");
        for i in 0..10 {
            assert!(
                joined.contains(&format!("Đoạn số {i}.")),
                "lost paragraph {i}"
            );
        }
    }

    #[test]
    fn chunking_never_splits_a_codepoint() {
        // One giant paragraph of multi-byte text with no sentence enders.
        let doc = "à".repeat(5000);
        let chunks = chunk_text(&doc, 300);
        assert!(chunks.len() >= 10);
        assert_eq!(
            chunks.iter().map(|c| c.chars().count()).sum::<usize>(),
            5000
        );
    }

    #[test]
    fn short_text_is_one_chunk() {
        assert_eq!(
            chunk_text("một câu ngắn.", CHUNK_CHARS),
            vec!["một câu ngắn."]
        );
        assert!(chunk_text("   \n\n  ", CHUNK_CHARS).is_empty());
    }

    #[test]
    fn json_extraction() {
        assert_eq!(extract_json("{\"a\":1}").unwrap()["a"], 1);
        assert_eq!(extract_json("```json\n{\"a\":2}\n```").unwrap()["a"], 2);
        assert_eq!(
            extract_json("Sure! Here it is:\n{\"a\": {\"b\": 3}}\nHope that helps").unwrap()["a"]
                ["b"],
            3
        );
        assert!(extract_json("[1,2,3]").unwrap().is_array());
        assert!(extract_json("no json here").is_none());
    }
}
