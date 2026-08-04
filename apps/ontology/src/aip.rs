//! **AIP Assist** — context-aware RAG over the platform's *metadata*.
//!
//! A sidebar you can open from any tab, whose answers change depending on where
//! you are. The flow is:
//!
//! ```text
//!   question ─┐
//!             ├─► retrieve (BM25 over the metadata index)
//!   session  ─┘        │  ▲ boosted by the tab you are on + the source you have open
//!   context            ▼
//!                  top-k chunks ──► LLM ──► answer + citations (each citable chunk
//!                                            names the tab it came from)
//! ```
//!
//! # The one rule: metadata, never data
//!
//! The index holds *descriptions of* things — source names, column names and
//! their profiled types/roles, class and property IRIs, the mapping's shape,
//! SHACL constraints, competency questions, lineage batches, activity logs, and
//! the app's own documentation. It must never hold a **cell value, a sample, a
//! literal or an entity IRI minted from user data**.
//!
//! This is not a style preference. Assist is answerable from a sidebar with no
//! per-row access control in front of it, so anything that reaches the index is
//! effectively public to anyone who can open the sidebar. Keeping values out is
//! what makes that safe — and it is enforced here in [`redact`] rather than
//! hoped for in a prompt, with [`tests::index_never_contains_data_values`]
//! standing guard.
//!
//! When a question actually needs values ("how many products are there?"), the
//! answer is a **hand-off** to Ask/SPARQL, which do read the data and do go
//! through the query path. Assist says so instead of guessing.

use crate::api::AppState;
use crate::{llm, shacl, tbox};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// documents
// ---------------------------------------------------------------------------

/// One retrievable chunk of metadata or documentation.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Doc {
    pub id: String,
    /// `concept | project | source | class | property | schema | mapping | shape | competency | lineage | activity`
    pub kind: String,
    pub title: String,
    /// Metadata text only — see the module docs.
    pub body: String,
    /// Tabs this chunk is relevant to; a match with the session context boosts it.
    pub tabs: Vec<String>,
    /// The thing this chunk is *about* — a source name, a class IRI, a batch.
    ///
    /// Session context is matched against this rather than against the body
    /// text, because word overlap is not aboutness: a source with a `supplier`
    /// column contains the word "supplier" without being about the `suppliers`
    /// source at all, and boosting it would push the right chunk down.
    pub subject: Option<String>,
}

impl Doc {
    fn new(id: &str, kind: &str, title: &str, body: String, tabs: &[&str]) -> Self {
        Self {
            id: id.to_string(),
            kind: kind.to_string(),
            title: title.to_string(),
            body: redact(&body),
            tabs: tabs.iter().map(|t| t.to_string()).collect(),
            subject: None,
        }
    }

    fn about(mut self, subject: &str) -> Self {
        self.subject = Some(subject.to_string());
        self
    }
}

/// Last-ditch scrub before anything enters the index.
///
/// Every builder below is already written to select metadata fields only; this
/// is the belt to that pair of braces, catching a future field that carries a
/// value through. It drops quoted literals and collapses any IRI minted under a
/// project's data namespace down to its shape.
fn redact(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            // Skip to the closing quote — a quoted run is a value, not a name.
            let mut inner = String::new();
            for c2 in chars.by_ref() {
                if c2 == '"' {
                    break;
                }
                inner.push(c2);
            }
            // Keep short bare identifiers (column names are often quoted);
            // anything long or sentence-like is a value.
            if inner.len() <= 40 && !inner.contains(' ') {
                out.push('"');
                out.push_str(&inner);
                out.push('"');
            } else {
                out.push_str("‹value›");
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// the platform documentation half of the index
// ---------------------------------------------------------------------------

/// Static documentation: what each stage is for and how to drive it. This is
/// what makes Assist answer "how do I…" rather than only "what is in here".
fn concept_docs() -> Vec<Doc> {
    let d = |id: &str, title: &str, tabs: &[&str], body: &str| {
        Doc::new(id, "concept", title, body.to_string(), tabs)
    };
    vec![
        d("concept:pipeline", "The pipeline, end to end", &["studio", "sources"],
          "SenClaw Ontology turns raw files into an RDF knowledge graph in seven stages: \
           1 Sources (ingest and profile), 2 Ontology / T-Box (design classes and properties), \
           3 Mapping (declarative RML-lite rules from columns to triples), 4 Explore (SPARQL and graph view), \
           5 Competency (the acceptance test suite), 6 Validate (SHACL-lite), \
           7 Reason and Provenance (inference, lineage batches, entity resolution, extraction). \
           The Studio tab runs all of it in one click via Auto-build. The discipline that matters: \
           the T-Box is designed from competency questions, the A-Box is generated by the mapping and \
           re-run whenever sources change. Never let the column layout dictate the ontology."),
        d("concept:ingest", "Which file formats can I load?", &["studio", "sources"],
          "Drop any file on the Studio tab. Detection is by magic bytes and structure, never the file extension. \
           Supported: CSV, TSV and pipe-delimited (separator auto-detected); Excel xlsx, xlsm, xls and ods \
           (one source per sheet, dates decoded); JSON whether nested, wrapped in an envelope, or JSON Lines; \
           YAML; XML (attributes become @name columns); Markdown tables; HTML tables; Word docx; PDF text layer; \
           and plain prose. Nested structures are flattened to dotted column paths such as customer.name or \
           items.0.sku, because the mapping DSL addresses columns by name. Prose becomes a text source that the \
           AI extracts triples from instead of a table that gets mapped."),
        d("concept:autobuild", "What does Auto-build actually do?", &["studio"],
          "Auto-build is a background job running nine steps: profile the sources, draft competency questions, \
           design the T-Box, author the mapping and mechanically repair it, lift rows into the graph, extract \
           triples from text documents, draft SHACL shapes and validate, write SPARQL for each competency \
           question and run the suite, and materialize inferences. The AI only ever produces drafts. Every \
           drafted mapping is checked against the real column names before it can run: a predicate-object map \
           naming a column that does not exist is dropped, an unresolvable subject template becomes a stable \
           hashed IRI, a hallucinated source name is re-pointed, and a triples map with nothing usable left is \
           discarded. The repairs are reported in the job result, not hidden."),
        d("concept:tbox", "T-Box: classes and properties", &["tbox"],
          "The T-Box is the schema: owl:Class declarations and object, data or annotation properties with \
           rdfs:domain and rdfs:range. It lives in its own named graph, separate from instance data and from \
           provenance. Four transforms separate a real ontology from a database schema in RDF clothing: \
           one row usually holds several entities, so model each as its own class; a repeated value is one \
           individual, not many; an enum column becomes SKOS individuals rather than subclasses; and a \
           relation that carries its own attributes needs an intermediate reification class rather than a \
           single object property."),
        d("concept:mapping", "The RML-lite mapping DSL", &["mapping"],
          "The mapping is data, not code: JSON with base, prefixes and triplesMaps. Each triples map names a \
           source, a subject and a list of predicateObjectMaps. Subject forms: template such as product/{sku} \
           when a natural key exists, or hash over identifying columns with a seg path segment when it does \
           not, plus an optional class to rdf:type the subject. Object forms: column with optional datatype or \
           lang for a literal; template for an IRI reference to another entity; parentHash with parentSeg for a \
           keyless referenced entity; or constant. Mint stable IRIs — never a row number. Preview before you \
           lift; lifting is idempotent because RDF has set semantics."),
        d("concept:sparql", "Querying: SPARQL and Ask", &["explore", "studio"],
          "The Explore tab runs SPARQL 1.1. Standard prefixes and the project's ex: prefix are declared for you, \
           and the default graph is the union of every data batch plus inferred triples, so a plain ?s ?p ?o \
           pattern sees everything. Ground a hand-written query on the live schema — the classes and predicates \
           that actually occur, with counts — rather than on the declared T-Box alone; querying a \
           declared-but-unused predicate is the usual cause of a confidently empty result. The Studio tab's Ask \
           box does this for you: it translates the question, repairs the query once if it fails or matches \
           nothing, and returns an answer together with the query and the rows."),
        d("concept:competency", "Competency questions as a test suite", &["competency"],
          "A competency question is a question the ontology must be able to answer, paired with the one SPARQL \
           query that answers it. Together they are the ontology's acceptance test: run them all and each \
           reports pass or fail against an expectation of nonempty, empty or boolean. Design the T-Box from \
           these questions rather than from the shape of the file, and re-run the suite after every change."),
        d("concept:shacl", "SHACL-lite validation", &["validate"],
          "Validation is closed-world: missing data is a violation, which makes it the complement of OWL \
           reasoning rather than a competitor. Shapes are JSON: nodeShapes, each with a targetClass and a list \
           of property constraints. Supported per-property constraints are datatype, class, nodeKind of IRI or \
           Literal or BlankNode, minCount, maxCount, minInclusive, maxInclusive, and a regex pattern. Use it to \
           gate data quality; do not expect it to infer anything."),
        d("concept:reasoning", "Reasoning and materialization", &["governance"],
          "Materialization applies an RDFS and OWL-RL subset — subclass, subproperty, domain, range, inverse, \
           and owl:sameAs symmetry and transitivity — to a fixpoint, writing into a dedicated inferred graph so \
           inferred triples are never confused with asserted ones. Reasoning is open-world: it only ever adds \
           facts. Run it before any competency question that depends on inference, such as one querying a \
           superclass."),
        d("concept:provenance", "Provenance batches and lineage", &["governance"],
          "Every import lands in its own named graph recorded as a PROV entity with a label, the source it was \
           derived from, the activity that produced it, a generation timestamp and a live triple count. That is \
           the lineage: which load produced which triples, and when. Because each batch is isolated you can drop \
           exactly one lot and reload it without disturbing the rest. Extraction from text goes into its own \
           batch too, so lower-confidence AI-derived triples can be removed as a unit."),
        d("concept:resolution", "Entity resolution", &["governance"],
          "Duplicate individuals of a class are found by Jaro-Winkler similarity over a label property, above a \
           threshold you choose. Review the candidate pairs before linking. The default link is skos:closeMatch \
           rather than owl:sameAs, because sameAs is transitive and one wrong link can contaminate an entire \
           cluster."),
        d("concept:assist", "What AIP Assist can and cannot see", &["studio", "sources", "tbox", "mapping", "explore", "competency", "validate", "governance"],
          "Assist indexes metadata only: source and column names with their profiled types and roles, class and \
           property IRIs, the shape of the mapping, SHACL constraints, competency questions, lineage batches, \
           activity logs, and this documentation. It deliberately holds no cell values, no samples and no \
           literals from your data. Questions about what the data says — counts, totals, which supplier, which \
           customer — are answered by the Ask box or a SPARQL query, which do read the data. Assist will point \
           you there rather than guess."),
    ]
}

// ---------------------------------------------------------------------------
// the project-metadata half of the index
// ---------------------------------------------------------------------------

/// Build the whole index for a project: documentation plus everything the
/// platform knows *about* this project.
pub fn build_index(state: &Arc<AppState>, pid: i64) -> anyhow::Result<Vec<Doc>> {
    let mut docs = concept_docs();

    let project = state.db.get_project(pid)?;
    let Some(p) = project else {
        return Ok(docs);
    };
    let prefixes = p
        .prefixes
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(k, v)| format!("{k}: {}", v.as_str().unwrap_or("")))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    docs.push(Doc::new(
        "project",
        "project",
        &format!("Project: {}", p.name),
        format!(
            "Ontology project {} with base IRI {}. Declared prefixes: {}. {}",
            p.name, p.base_iri, prefixes, p.description
        ),
        &["studio"],
    ));

    // ---- sources & column profiles (names and stats; never sample values) ---
    for s in state.db.list_sources(pid)? {
        let cols = s.columns.as_array().cloned().unwrap_or_default();
        let col_lines: Vec<String> = cols
            .iter()
            .map(|c| {
                format!(
                    "{} ({}, role {}{}{}, {}% null, {} distinct)",
                    c["name"].as_str().unwrap_or(""),
                    c["datatype"].as_str().unwrap_or(""),
                    c["role"].as_str().unwrap_or(""),
                    if c["isUnique"].as_bool().unwrap_or(false) {
                        ", candidate key"
                    } else {
                        ""
                    },
                    if c["isEnum"].as_bool().unwrap_or(false) {
                        ", enum"
                    } else {
                        ""
                    },
                    (c["nullRatio"].as_f64().unwrap_or(0.0) * 100.0).round(),
                    c["distinctCount"].as_u64().unwrap_or(0),
                )
            })
            .collect();
        // The origin is a machine token (`xlsx`, `csv-semicolon`); people search
        // for the common name ("excel", "spreadsheet", "word"). Seed the aliases
        // so retrieval on the human word reaches the source that has it.
        let aliases = origin_aliases(&s.origin);
        let body = if s.kind == "text" {
            format!(
                "Unstructured text source {} ingested from {} format{}. {} It holds {} block(s) of prose and is \
                 processed by AI triple extraction rather than by the mapping.",
                s.name, s.origin, aliases, s.note, s.row_count
            )
        } else {
            format!(
                "Tabular source {} ingested from {} format{}. {} It has {} rows and {} columns. \
                 Columns: {}.",
                s.name,
                s.origin,
                aliases,
                s.note,
                s.row_count,
                cols.len(),
                col_lines.join("; ")
            )
        };
        docs.push(
            Doc::new(
                &format!("source:{}", s.name),
                "source",
                &format!("Source: {}", s.name),
                body,
                &["sources", "studio", "mapping"],
            )
            .about(&s.name),
        );
    }

    // ---- T-Box terms -------------------------------------------------------
    let g = state.graph_for(pid)?;
    let tb = tbox::read(&g)?;
    for c in tb["classes"].as_array().cloned().unwrap_or_default() {
        let iri = c["iri"].as_str().unwrap_or("");
        docs.push(
            Doc::new(
                &format!("class:{iri}"),
                "class",
                &format!("Class {}", short(iri)),
                format!(
                    "owl:Class {iri}{}{}. It is part of this project's T-Box (schema).",
                    c["label"]
                        .as_str()
                        .map(|l| format!(", labelled {l}"))
                        .unwrap_or_default(),
                    c["super"]
                        .as_str()
                        .map(|s| format!(", a subclass of {s}"))
                        .unwrap_or_default(),
                ),
                &["tbox", "studio"],
            )
            .about(iri),
        );
    }
    for pr in tb["properties"].as_array().cloned().unwrap_or_default() {
        let iri = pr["iri"].as_str().unwrap_or("");
        docs.push(
            Doc::new(
                &format!("property:{iri}"),
                "property",
                &format!("Property {}", short(iri)),
                format!(
                    "{} {iri}{}{}{}. It is part of this project's T-Box (schema).",
                    short(pr["kind"].as_str().unwrap_or("property")),
                    pr["label"]
                        .as_str()
                        .map(|l| format!(", labelled {l}"))
                        .unwrap_or_default(),
                    pr["domain"]
                        .as_str()
                        .map(|d| format!(", domain {d}"))
                        .unwrap_or_default(),
                    pr["range"]
                        .as_str()
                        .map(|r| format!(", range {r}"))
                        .unwrap_or_default(),
                ),
                &["tbox", "mapping"],
            )
            .about(iri),
        );
    }

    // ---- live schema: shapes and counts, never the example values ----------
    if let Ok(live) = tbox::live_schema(&g) {
        let classes: Vec<String> = live["classes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|c| {
                format!(
                    "{} ({} instances)",
                    short(c["class"].as_str().unwrap_or("")),
                    c["count"].as_str().unwrap_or("0")
                )
            })
            .collect();
        let preds: Vec<String> = live["predicates"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            // `example` is a real object value — deliberately not read here.
            .map(|p| {
                format!(
                    "{} ({} uses)",
                    short(p["predicate"].as_str().unwrap_or("")),
                    p["count"].as_str().unwrap_or("0")
                )
            })
            .collect();
        if !classes.is_empty() || !preds.is_empty() {
            docs.push(Doc::new(
                "schema:live",
                "schema",
                "Live schema (what the data actually uses)",
                format!(
                    "Classes present in the data with instance counts: {}. Predicates in use with occurrence \
                     counts: {}. These are the IRIs a SPARQL query should be grounded on.",
                    if classes.is_empty() { "none".into() } else { classes.join(", ") },
                    if preds.is_empty() { "none".into() } else { preds.join(", ") },
                ),
                &["explore", "studio", "competency"],
            ));
        }
    }

    // ---- mapping shape -----------------------------------------------------
    let mapping = state.db.get_mapping(pid)?;
    if let Some(tms) = mapping["triplesMaps"].as_array() {
        for tm in tms {
            let name = tm["name"].as_str().unwrap_or("map");
            let src = tm["source"].as_str().unwrap_or("");
            let subject = &tm["subject"];
            let subj_desc = match (subject["template"].as_str(), subject["hash"].as_array()) {
                (Some(t), _) => format!("templated IRI {t}"),
                (None, Some(h)) => format!(
                    "IRI hashed from {}",
                    h.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
                _ => "unspecified subject".into(),
            };
            let poms: Vec<String> = tm["predicateObjectMaps"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|pom| {
                    let o = &pom["object"];
                    let from = if let Some(c) = o["column"].as_str() {
                        format!("column {c}")
                    } else if let Some(t) = o["template"].as_str() {
                        format!("reference {t}")
                    } else if let Some(h) = o["parentHash"].as_array() {
                        format!(
                            "parent hash of {}",
                            h.iter()
                                .filter_map(|x| x.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    } else {
                        "constant".into()
                    };
                    format!("{} from {}", pom["predicate"].as_str().unwrap_or(""), from)
                })
                .collect();
            docs.push(Doc::new(
                &format!("mapping:{name}"),
                "mapping",
                &format!("Mapping {name} ({src})"),
                format!(
                    "Triples map {name} reads source {src} and mints a subject as a {subj_desc}, typed as {}. \
                     It emits: {}.",
                    subject["class"].as_str().unwrap_or("no class"),
                    if poms.is_empty() { "nothing".into() } else { poms.join("; ") }
                ),
                &["mapping", "studio"],
            ).about(src));
        }
    }

    // ---- SHACL shapes ------------------------------------------------------
    if let Ok(shapes) = serde_json::from_value::<shacl::Shapes>(state.db.get_shapes(pid)?) {
        for ns in &shapes.node_shapes {
            let constraints: Vec<String> = ns
                .properties
                .iter()
                .map(|p| {
                    let mut parts = vec![p.path.clone()];
                    if let Some(d) = &p.datatype {
                        parts.push(format!("datatype {d}"));
                    }
                    if let Some(c) = &p.class {
                        parts.push(format!("class {c}"));
                    }
                    if let Some(n) = p.min_count {
                        parts.push(format!("minCount {n}"));
                    }
                    if let Some(n) = p.max_count {
                        parts.push(format!("maxCount {n}"));
                    }
                    parts.join(" ")
                })
                .collect();
            docs.push(
                Doc::new(
                    &format!("shape:{}", ns.target_class),
                    "shape",
                    &format!("Shape for {}", short(&ns.target_class)),
                    format!(
                        "SHACL-lite node shape targeting class {}. Constraints: {}.",
                        ns.target_class,
                        if constraints.is_empty() {
                            "none".into()
                        } else {
                            constraints.join("; ")
                        }
                    ),
                    &["validate"],
                )
                .about(&ns.target_class),
            );
        }
    }

    // ---- competency questions ----------------------------------------------
    for cq in state.db.list_cq(pid)? {
        docs.push(Doc::new(
            &format!("cq:{}", cq.id),
            "competency",
            "Competency question",
            format!(
                "The ontology must answer: {}. Expectation: {}. {}",
                cq.question,
                cq.expect,
                if cq.sparql.trim().is_empty() {
                    "No SPARQL has been written for it yet.".to_string()
                } else {
                    format!("Answered by the query: {}", cq.sparql.replace('\n', " "))
                }
            ),
            &["competency", "explore"],
        ));
    }

    // ---- lineage -----------------------------------------------------------
    if let Ok(batches) = crate::prov::list_batches(&g) {
        for b in batches.as_array().cloned().unwrap_or_default() {
            let iri = b["iri"].as_str().unwrap_or("");
            docs.push(Doc::new(
                &format!("lineage:{iri}"),
                "lineage",
                &format!("Lineage batch {}", b["label"].as_str().unwrap_or("")),
                format!(
                    "Import batch {} was derived from {} by the {} activity at {}, and currently holds {} triples. \
                     Dropping this batch removes exactly those triples and nothing else.",
                    iri,
                    b["source"].as_str().unwrap_or("an unnamed source"),
                    b["activity"].as_str().unwrap_or("unknown"),
                    b["generatedAt"].as_str().unwrap_or("an unrecorded time"),
                    b["tripleCount"].as_i64().unwrap_or(0),
                ),
                &["governance"],
            ).about(iri));
        }
    }

    // ---- recent activity ---------------------------------------------------
    let logs = state.db.list_logs(pid, 25)?;
    if !logs.is_empty() {
        let lines: Vec<String> = logs
            .iter()
            .map(|(kind, detail, _)| format!("{kind}: {detail}"))
            .collect();
        docs.push(Doc::new(
            "activity:recent",
            "activity",
            "Recent activity in this project",
            format!(
                "Most recent operations, newest first: {}.",
                lines.join("; ")
            ),
            &["governance", "studio"],
        ));
    }

    Ok(docs)
}

fn short(iri: &str) -> String {
    iri.rsplit(['#', '/']).next().unwrap_or(iri).to_string()
}

/// Human synonyms for an ingest origin token, so "which excel source…" finds an
/// `xlsx` source and "the word doc" finds a `docx` one. Returned as a
/// parenthetical the retriever tokenizes like any other body text.
fn origin_aliases(origin: &str) -> String {
    let words: &[&str] = match origin {
        "xlsx" | "xlsm" | "xls" | "ods" => &["excel", "spreadsheet", "workbook", "sheet"],
        o if o.starts_with("csv") => &["csv", "delimited", "spreadsheet"],
        "tsv" => &["tsv", "tab-separated", "delimited"],
        "psv" => &["pipe-separated", "delimited"],
        "json" | "jsonl" => &["json"],
        "xml" => &["xml"],
        "yaml" => &["yaml", "yml"],
        "docx" | "odt" => &["word", "document"],
        "pdf" | "pdf-table" => &["pdf", "document"],
        o if o.starts_with("html") => &["html", "web", "webpage"],
        o if o.starts_with("markdown") => &["markdown", "md"],
        "text" => &["text", "prose", "document"],
        _ => &[],
    };
    if words.is_empty() {
        String::new()
    } else {
        format!(" (also known as {})", words.join(", "))
    }
}

// ---------------------------------------------------------------------------
// retrieval
// ---------------------------------------------------------------------------

/// Where the user is when they ask. This is what makes the same question get a
/// different answer in the Mapping tab than in the Validate tab.
#[derive(Default, Clone, serde::Deserialize, Debug)]
pub struct Context {
    /// Current tab: `studio | sources | tbox | mapping | explore | competency | validate | governance`.
    #[serde(default)]
    pub tab: Option<String>,
    /// Logical name of the source the user has selected, if any.
    #[serde(default)]
    pub source: Option<String>,
    /// Free-form note about the current selection (a class IRI, a batch, …).
    #[serde(default)]
    pub selection: Option<String>,
}

/// Fold Vietnamese (and general Latin) diacritics so "san pham" retrieves
/// "sản phẩm" — users type without tone marks far more often than not.
fn fold_char(c: char) -> char {
    match c {
        'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ' | 'ấ' | 'ẩ'
        | 'ẫ' | 'ậ' | 'ä' | 'å' => 'a',
        'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' | 'ë' => {
            'e'
        }
        'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' | 'ï' => 'i',
        'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ' | 'ớ' | 'ở'
        | 'ỡ' | 'ợ' | 'ö' => 'o',
        'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' | 'ü' => {
            'u'
        }
        'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
        'đ' => 'd',
        'ñ' => 'n',
        'ç' => 'c',
        other => other,
    }
}

/// Function words carry no retrieval signal but are ubiquitous in the way people
/// phrase questions ("what does this do with…"). The document-frequency filter
/// in [`retrieve`] catches the ones that saturate the corpus; this list catches
/// the ones that happen to sit in only a few chunks — a doc titled "**What** AIP
/// Assist **can** and **cannot** see" would otherwise win every "what can I…"
/// question on pure function-word overlap. Both halves are needed.
const STOP_WORDS: &[&str] = &[
    // English
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "am", "do", "does", "did", "doing",
    "have", "has", "had", "can", "cannot", "could", "should", "would", "will", "shall", "may",
    "might", "must", "i", "you", "it", "its", "this", "that", "these", "those", "there", "here",
    "what", "which", "who", "whom", "whose", "when", "where", "why", "how", "and", "or", "but",
    "if", "then", "else", "of", "to", "in", "on", "at", "by", "for", "with", "from", "as", "so",
    "my", "me", "we", "our", "us", "they", "them", "their", "he", "she", "his", "her", "not", "no",
    "yes", "any", "some", "all", "get", "got", "make", "made", "want", "need", "thing", "things",
    "about", "into", "out", "up", "down", "over", "again", "just", "only", "very",
    // Vietnamese (diacritics already folded by `fold_char`)
    "la", "cua", "va", "hoac", "thi", "cho", "voi", "tu", "den", "trong", "tren", "duoi", "khi",
    "nao", "gi", "sao", "the", "nay", "do", "kia", "co", "khong", "duoc", "bi", "se", "da", "dang",
    "toi", "ban", "minh", "chung", "ho", "no", "ai", "dau", "lam", "can", "muon", "phai", "nen",
    "cai", "mot", "cac", "nhung", "moi", "ma", "ra", "vao", "len", "xuong", "ve", "hay", "cung",
];

fn is_stop_word(t: &str) -> bool {
    STOP_WORDS.contains(&t)
}

/// Conflate the inflections that matter for a documentation corpus, so a user
/// asking how to "validate" reaches a chunk about "validation", and "columns"
/// reaches "column". Deliberately not a full Porter stemmer — the rules below
/// are the handful that pay for themselves here, and each is applied to query
/// and document alike so an over-aggressive stem can only ever cost precision,
/// never correctness.
fn stem(t: &str) -> String {
    let mut s = t.to_string();
    let n = s.chars().count();
    // plurals
    if n > 4 && s.ends_with("ies") {
        s.truncate(s.len() - 3);
        s.push('y');
    } else if n > 4
        && s.ends_with("es")
        && s.chars()
            .nth(n - 3)
            .is_some_and(|c| matches!(c, 's' | 'x' | 'z' | 'h'))
    {
        s.truncate(s.len() - 2);
    } else if n > 3
        && s.ends_with('s')
        && !s.ends_with("ss")
        && !s.ends_with("us")
        && !s.ends_with("is")
    {
        s.truncate(s.len() - 1);
    }
    // nominalisation: validation -> validat, so it meets validate -> validat
    if s.chars().count() > 5 && s.ends_with("ion") {
        s.truncate(s.len() - 3);
    }
    if s.chars().count() > 4 && s.ends_with("ed") {
        s.truncate(s.len() - 2);
    }
    if s.chars().count() > 4 && s.ends_with('e') {
        s.truncate(s.len() - 1);
    }
    s
}

fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            let t = std::mem::take(cur);
            // Stop words are recognised before stemming, while they still look
            // like themselves.
            out.push(if is_stop_word(&t) { t } else { stem(&t) });
        }
    };
    for c in s.chars() {
        let lc = c.to_lowercase().next().unwrap_or(c);
        let f = fold_char(lc);
        if f.is_alphanumeric() {
            cur.push(f);
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Whether a doc is *about* the named thing. Exact on the declared subject
/// (case-insensitively, and tolerant of a curie vs full IRI), never a substring
/// or word-overlap match — see [`Doc::subject`].
fn is_about(d: &Doc, target: &str) -> bool {
    let Some(subject) = d.subject.as_deref() else {
        return false;
    };
    let t = target.trim();
    subject.eq_ignore_ascii_case(t) || short(subject).eq_ignore_ascii_case(&short(t))
}

/// A scored hit, carrying enough to cite it back to the user.
#[derive(Clone, serde::Serialize)]
pub struct Hit {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    /// Tab the citation links to, when there is an obvious one.
    pub tab: Option<String>,
    pub score: f64,
    /// Why this ranked where it did — makes the retrieval auditable.
    pub reason: String,
}

/// BM25 over the index, then a context boost.
///
/// BM25 rather than embeddings on purpose: the corpus is a few hundred short
/// chunks of largely proper nouns (column names, IRIs, source names), which is
/// exactly where lexical matching beats a similarity model — and it keeps the
/// app self-contained, with no embedding round-trip per question.
pub fn retrieve(docs: &[Doc], question: &str, ctx: &Context, k: usize) -> Vec<Hit> {
    const K1: f64 = 1.2;
    const B: f64 = 0.75;
    let q_terms = tokenize(question);
    if docs.is_empty() {
        return Vec::new();
    }

    let doc_tokens: Vec<Vec<String>> = docs
        .iter()
        .map(|d| tokenize(&format!("{} {} {}", d.title, d.kind, d.body)))
        .collect();
    let avg_len =
        doc_tokens.iter().map(|t| t.len()).sum::<usize>() as f64 / doc_tokens.len() as f64;
    let n = docs.len() as f64;

    // Document frequency per query term.
    let mut df: HashMap<&str, f64> = HashMap::new();
    for term in &q_terms {
        let count = doc_tokens
            .iter()
            .filter(|t| t.iter().any(|x| x == term))
            .count();
        df.insert(term.as_str(), count as f64);
    }

    // Drop terms that appear in more than half the corpus. On a corpus this
    // small, BM25's IDF damps a stop word without silencing it — and six
    // near-zero contributions from "what / does / this / with / and / do" still
    // outweigh one real term, handing every query to whichever chunk is most
    // chatty. Filtering by observed document frequency does this without a
    // hardcoded stop list, so it works for Vietnamese questions too.
    let cutoff = n * 0.5;
    // Function words go first and unconditionally: if nothing but stop words is
    // left — "what is this?" — the question carries no lexical intent at all,
    // and scoring it would rank by chattiness. Returning nothing lets the
    // context fallback below answer from where the user is standing, which is
    // the only real signal available.
    let non_stop: Vec<&String> = q_terms.iter().filter(|t| !is_stop_word(t)).collect();
    // The frequency filter is a tie-breaker *between* content terms, so it only
    // applies while something more discriminating survives. A one-word question
    // ("columns") is its own signal however common the word is.
    let discriminating: Vec<&String> = non_stop
        .iter()
        .copied()
        .filter(|t| df[t.as_str()] <= cutoff)
        .collect();
    let q_terms: Vec<&String> = if discriminating.is_empty() {
        non_stop
    } else {
        discriminating
    };

    let mut hits: Vec<Hit> = Vec::new();
    for (i, d) in docs.iter().enumerate() {
        let tokens = &doc_tokens[i];
        let len = tokens.len() as f64;
        let mut score = 0.0;
        for term in q_terms.iter().copied() {
            let tf = tokens.iter().filter(|x| *x == term).count() as f64;
            if tf == 0.0 {
                continue;
            }
            let n_q = *df.get(term.as_str()).unwrap_or(&0.0);
            let idf = ((n - n_q + 0.5) / (n_q + 0.5) + 1.0).ln();
            score += idf * (tf * (K1 + 1.0)) / (tf + K1 * (1.0 - B + B * len / avg_len));
        }

        // ---- context boost: this is the "context-aware" in context-aware RAG.
        let mut reasons: Vec<String> = Vec::new();
        if score > 0.0 {
            reasons.push("matches the question".into());
        }
        if let Some(tab) = ctx.tab.as_deref() {
            if d.tabs.iter().any(|t| t == tab) {
                // Multiplicative, so it re-ranks among docs the question already
                // matched rather than dragging in an irrelevant one that merely
                // lives on this tab.
                //
                // Tapered by how many tabs the doc claims: a chunk that is
                // specifically about *this* stage earns the full boost, while
                // one that declares itself relevant everywhere earns almost
                // none — otherwise a catch-all doc wins on every tab and the
                // context stops discriminating at all.
                let boost = 1.0 + 0.6 / d.tabs.len().max(1) as f64;
                score *= boost;
                reasons.push(format!("relevant to the {tab} tab you are on"));
            }
        }
        if let Some(src) = ctx.source.as_deref().filter(|s| !s.is_empty()) {
            if is_about(d, src) {
                score *= 1.8;
                reasons.push(format!("about the open source {src}"));
            }
        }
        if let Some(sel) = ctx.selection.as_deref().filter(|s| !s.is_empty()) {
            if is_about(d, sel) || d.id == *sel {
                score *= 1.5;
                reasons.push(format!("about your selection {}", short(sel)));
            }
        }
        if score <= 0.0 {
            continue;
        }
        hits.push(Hit {
            id: d.id.clone(),
            kind: d.kind.clone(),
            title: d.title.clone(),
            body: d.body.clone(),
            tab: d.tabs.first().cloned(),
            score,
            reason: reasons.join("; "),
        });
    }
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Context fallback. "What does this do?" carries almost no lexical signal,
    // yet it is the single most natural thing to type into a sidebar — and the
    // honest answer depends entirely on where the user is standing. So when the
    // question alone retrieves little, top up from the current tab, most
    // specific chunk first. This is the difference between a search box and a
    // context-aware assistant.
    if hits.len() < 3 {
        if let Some(tab) = ctx.tab.as_deref() {
            let mut fallback: Vec<&Doc> = docs
                .iter()
                .filter(|d| d.tabs.iter().any(|t| t == tab) && !hits.iter().any(|h| h.id == d.id))
                .collect();
            fallback.sort_by_key(|d| (d.tabs.len(), d.id.clone()));
            for d in fallback.into_iter().take(3 - hits.len()) {
                hits.push(Hit {
                    id: d.id.clone(),
                    kind: d.kind.clone(),
                    title: d.title.clone(),
                    body: d.body.clone(),
                    tab: d.tabs.first().cloned(),
                    score: 0.0,
                    reason: format!("shown because you are on the {tab} tab"),
                });
            }
        }
    }

    hits.truncate(k);
    hits
}

// ---------------------------------------------------------------------------
// the data/metadata boundary
// ---------------------------------------------------------------------------

/// Words that mean "tell me what the data says" rather than "tell me how this
/// works" or "what is defined here".
const DATA_QUESTION_HINTS: &[&str] = &[
    "how many",
    "how much",
    "total",
    "sum",
    "average",
    "count of",
    "list all",
    "which supplier",
    "which customer",
    "top ",
    "most ",
    "largest",
    "cheapest",
    "highest",
    "lowest",
    "bao nhieu",
    "tong ",
    "trung binh",
    "liet ke",
    "cao nhat",
    "thap nhat",
    "nhieu nhat",
    "gia tri cua",
];

/// Whether a question is really asking for values. Assist holds no values, so
/// the honest response is a hand-off, not an answer assembled from schema names
/// that happen to contain the right nouns.
pub fn is_data_question(question: &str) -> bool {
    let folded: String = question
        .chars()
        .map(|c| fold_char(c.to_lowercase().next().unwrap_or(c)))
        .collect();
    DATA_QUESTION_HINTS.iter().any(|h| folded.contains(h))
}

const ASSIST_SYS: &str = "You are AIP Assist inside the SenClaw Ontology app: a helpful guide to the platform and to \
the user's own ontology project. Answer ONLY from the numbered CONTEXT passages provided. They contain METADATA — \
documentation, source and column names, class and property definitions, mapping structure, constraints, lineage — and \
never any actual data values. \
Rules: (1) Cite the passages you used as [1], [2] inline. (2) If the passages do not contain the answer, say so plainly \
and suggest which tab to look at — never invent a column, class, source or number. (3) You cannot see the user's data; \
if the question needs actual values, say so and point to the Ask box or a SPARQL query. (4) The user's current tab is \
given — prefer what is relevant to where they are. (5) Reply in the SAME LANGUAGE as the question, in a few short \
sentences or a compact list.";

/// Run the context-aware RAG flow: retrieve, then answer with citations.
pub async fn assist(
    state: &Arc<AppState>,
    pid: i64,
    question: &str,
    ctx: &Context,
) -> Result<Value, String> {
    if question.trim().is_empty() {
        return Err("question is required".into());
    }
    let docs = build_index(state, pid).map_err(|e| e.to_string())?;
    let hits = retrieve(&docs, question, ctx, 6);

    // A values question gets a hand-off, plus whatever metadata is still useful
    // (which class holds it, which source it came from) — that part Assist does know.
    let data_question = is_data_question(question);

    if hits.is_empty() {
        // Nothing retrieved. If the question wanted values, the hand-off is
        // still the correct answer — falling through to a generic "not found"
        // would hide the one useful thing Assist has to say here.
        let answer = if data_question {
            "That question asks what your data says, and AIP Assist only indexes metadata — source and column \
             names, classes, properties, constraints, lineage — never values. Use the Ask box on the Studio tab, \
             or run a SPARQL query on Explore; both read the data itself."
        } else {
            "I could not find anything about that in this project's metadata or in the app documentation. \
             Try naming a source, column, class, or a stage of the pipeline."
        };
        return Ok(json!({
            "question": question,
            "answer": answer,
            "citations": [],
            "context": ctx_summary(ctx),
            "dataQuestion": data_question,
            "model": "",
        }));
    }

    let passages = hits
        .iter()
        .enumerate()
        .map(|(i, h)| format!("[{}] ({}) {}\n{}", i + 1, h.kind, h.title, h.body))
        .collect::<Vec<_>>()
        .join("\n\n");
    let prompt = format!(
        "USER CONTEXT: {}\n\nCONTEXT PASSAGES:\n{passages}\n\n{}QUESTION: {question}\n\nAnswer:",
        ctx_summary(ctx),
        if data_question {
            "NOTE: this question appears to ask for actual data values, which are not in the passages. \
             Explain what the metadata says about where that answer would come from (which class, property or \
             source), then tell the user to use the Ask box on the Studio tab or a SPARQL query on Explore. \
             Do NOT state any count or value.\n\n"
        } else {
            ""
        }
    );

    let (answer, model) = llm::assist_answer(ASSIST_SYS, &prompt).await?;
    Ok(json!({
        "question": question,
        "answer": answer,
        "citations": hits.iter().enumerate().map(|(i, h)| json!({
            "n": i + 1, "id": h.id, "kind": h.kind, "title": h.title,
            "tab": h.tab, "reason": h.reason, "score": (h.score * 100.0).round() / 100.0,
        })).collect::<Vec<_>>(),
        "context": ctx_summary(ctx),
        "dataQuestion": data_question,
        "model": model,
    }))
}

fn ctx_summary(ctx: &Context) -> String {
    let mut parts = Vec::new();
    if let Some(t) = ctx.tab.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("on the {t} tab"));
    }
    if let Some(s) = ctx.source.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("with source {s} open"));
    }
    if let Some(s) = ctx.selection.as_deref().filter(|s| !s.is_empty()) {
        parts.push(format!("having selected {s}"));
    }
    if parts.is_empty() {
        "no particular tab".into()
    } else {
        parts.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> Vec<Doc> {
        let mut d = concept_docs();
        d.push(
            Doc::new(
                "source:products",
                "source",
                "Source: products",
                "Tabular source products ingested from xlsx format. It has 3 rows and 5 columns. \
                 Columns: sku (string, role identifier, candidate key, 0% null, 3 distinct); price (integer, role attribute)."
                    .into(),
                &["sources", "studio", "mapping"],
            )
            .about("products"),
        );
        d
    }

    #[test]
    fn folds_vietnamese_diacritics_for_retrieval() {
        assert_eq!(tokenize("sản phẩm Đường"), vec!["san", "pham", "duong"]);
        // A question typed without tone marks still reaches the toned text.
        assert_eq!(tokenize("san pham duong"), tokenize("sản phẩm Đường"));
    }

    #[test]
    fn retrieval_finds_the_right_concept() {
        let hits = retrieve(
            &docs(),
            "how do I validate my data with SHACL?",
            &Context::default(),
            3,
        );
        assert_eq!(
            hits[0].id,
            "concept:shacl",
            "{:?}",
            hits.iter().map(|h| &h.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_same_question_answers_differently_per_tab() {
        // "class" is discussed by the T-Box docs (declaring one), the mapping
        // docs (typing a subject) and the SHACL docs (a class constraint) alike.
        // Where the user is standing is what should break the tie.
        let d = docs();
        let q = "class";
        // Absent ranks worst — note `Option`'s own ordering puts `None` first,
        // which would silently invert every comparison below.
        let rank =
            |hits: &[Hit], id: &str| hits.iter().position(|h| h.id == id).unwrap_or(usize::MAX);
        let on_tbox = retrieve(
            &d,
            q,
            &Context {
                tab: Some("tbox".into()),
                ..Default::default()
            },
            9,
        );
        let on_validate = retrieve(
            &d,
            q,
            &Context {
                tab: Some("validate".into()),
                ..Default::default()
            },
            9,
        );

        assert!(
            rank(&on_validate, "concept:shacl") < rank(&on_tbox, "concept:shacl"),
            "SHACL docs must rank higher while standing on Validate: {:?} vs {:?}",
            ids(&on_validate),
            ids(&on_tbox)
        );
        assert!(
            rank(&on_tbox, "concept:tbox") < rank(&on_validate, "concept:tbox"),
            "T-Box docs must rank higher while standing on Ontology: {:?} vs {:?}",
            ids(&on_tbox),
            ids(&on_validate)
        );
        assert!(
            on_tbox[0].reason.contains("tbox tab"),
            "{}",
            on_tbox[0].reason
        );
    }

    #[test]
    fn stop_words_do_not_decide_the_ranking() {
        // The content term must win over a pile of function words.
        let d = docs();
        let bare = retrieve(&d, "lineage", &Context::default(), 3);
        let padded = retrieve(
            &d,
            "so what is the thing that this does with lineage here",
            &Context::default(),
            3,
        );
        assert_eq!(bare[0].id, "concept:provenance");
        assert_eq!(padded[0].id, bare[0].id, "{:?}", ids(&padded));
    }

    fn ids(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.id.as_str()).collect()
    }

    #[test]
    fn an_open_source_boosts_its_own_metadata() {
        let q = "columns";
        let score_of = |hits: &[Hit]| {
            hits.iter()
                .find(|h| h.id == "source:products")
                .map(|h| h.score)
        };
        let plain = score_of(&retrieve(&docs(), q, &Context::default(), 9)).unwrap();
        let with_src = retrieve(
            &docs(),
            q,
            &Context {
                source: Some("products".into()),
                tab: Some("sources".into()),
                ..Default::default()
            },
            9,
        );
        assert_eq!(with_src[0].id, "source:products");
        assert!(
            score_of(&with_src).unwrap() > plain,
            "having the source open must raise its own metadata"
        );
        assert!(
            with_src[0].reason.contains("products"),
            "{}",
            with_src[0].reason
        );
    }

    #[test]
    fn origin_aliases_make_common_names_retrievable() {
        // The word "excel" never appears in an xlsx origin token; the alias does.
        assert!(origin_aliases("xlsx").contains("excel"));
        assert!(origin_aliases("csv-semicolon").contains("csv"));
        assert!(origin_aliases("docx").contains("word"));
        assert!(origin_aliases("weird").is_empty());
        // And it actually reaches the doc: a source doc built with an xlsx origin
        // must retrieve on "excel".
        let d = vec![
            Doc::new(
                "source:sales",
                "source",
                "Source: sales",
                format!(
                    "Tabular source sales ingested from xlsx format{}. 3 rows.",
                    origin_aliases("xlsx")
                ),
                &["sources"],
            )
            .about("sales"),
            Doc::new(
                "source:notes",
                "source",
                "Source: notes",
                format!(
                    "Text source notes ingested from text format{}.",
                    origin_aliases("text")
                ),
                &["sources"],
            )
            .about("notes"),
        ];
        let hits = retrieve(&d, "which excel source do I have", &Context::default(), 3);
        assert_eq!(hits[0].id, "source:sales", "{:?}", ids(&hits));
    }

    #[test]
    fn aboutness_is_identity_not_word_overlap() {
        // `orders` merely *has* a column called supplier; `suppliers` IS the
        // source in question. Opening suppliers must not boost orders.
        let mut d = docs();
        d.push(
            Doc::new(
                "source:orders",
                "source",
                "Source: orders",
                "Tabular source orders with 4 columns. Columns: id; supplier (string, role relation); total.".into(),
                &["sources"],
            )
            .about("orders"),
        );
        d.push(
            Doc::new(
                "source:suppliers",
                "source",
                "Source: suppliers",
                "Tabular source suppliers with 2 columns. Columns: code; name.".into(),
                &["sources"],
            )
            .about("suppliers"),
        );
        let ctx = Context {
            tab: Some("sources".into()),
            source: Some("suppliers".into()),
            ..Default::default()
        };
        let hits = retrieve(&d, "supplier columns", &ctx, 5);
        let boosted: Vec<&str> = hits
            .iter()
            .filter(|h| h.reason.contains("about the open source"))
            .map(|h| h.id.as_str())
            .collect();
        assert_eq!(
            boosted,
            vec!["source:suppliers"],
            "only the suppliers doc is *about* suppliers"
        );
    }

    #[test]
    fn a_vague_question_still_gets_the_current_tab() {
        // No content words at all — retrieval must fall back to where the user is.
        let d = docs();
        let hits = retrieve(
            &d,
            "what is this",
            &Context {
                tab: Some("validate".into()),
                ..Default::default()
            },
            5,
        );
        assert!(
            !hits.is_empty(),
            "a vague question on a tab must not come back empty"
        );
        assert!(
            hits.iter().any(|h| h.id == "concept:shacl"),
            "expected the Validate tab's own docs, got {:?}",
            ids(&hits)
        );
        assert!(hits.iter().any(|h| h.reason.contains("because you are on")));
    }

    #[test]
    fn a_catch_all_doc_does_not_win_every_tab() {
        // concept:assist declares itself relevant to all eight tabs. Its boost
        // must therefore be far weaker than a doc specific to the current tab.
        let assist = docs()
            .into_iter()
            .find(|d| d.id == "concept:assist")
            .unwrap();
        let shacl = docs()
            .into_iter()
            .find(|d| d.id == "concept:shacl")
            .unwrap();
        assert_eq!(shacl.tabs.len(), 1);
        assert!(assist.tabs.len() >= 8);
        let boost = |d: &Doc| 1.0 + 0.6 / d.tabs.len().max(1) as f64;
        assert!(
            boost(&shacl) > boost(&assist) * 1.4,
            "specific must clearly beat catch-all"
        );
    }

    #[test]
    fn values_questions_are_recognized_in_both_languages() {
        assert!(is_data_question("How many products are there?"));
        assert!(is_data_question("Có bao nhiêu sản phẩm?"));
        assert!(is_data_question("Tổng giá trị các đơn hàng"));
        assert!(!is_data_question("What is a T-Box?"));
        assert!(!is_data_question(
            "Cột nào là khoá chính của source products?"
        ));
    }

    /// The load-bearing test for this whole module: build a real index over a
    /// project whose data is full of a distinctive value, and prove the value
    /// never reaches a document. If this fails, the sidebar leaks data.
    #[test]
    fn index_never_contains_data_values() {
        use crate::api::AppState;
        use crate::db::Db;
        use std::collections::HashMap as Map;
        use std::sync::Mutex;

        const SECRET: &str = "Zzqqxx Confidential Client Name";

        let db = Db::open(":memory:").unwrap();
        let (tx, _rx) = tokio::sync::broadcast::channel(4);
        let state = Arc::new(AppState {
            db,
            graphs: Mutex::new(Map::new()),
            mcp_tx: tx,
            jobs: Mutex::new(Map::new()),
        });
        let pid = state
            .db
            .create_project("Leaky", "", "http://ex/leaky/")
            .unwrap();

        // A source whose profiled samples contain the secret…
        let csv = format!("id,client\n1,{SECRET}\n2,{SECRET}\n");
        let table = crate::profile::parse("csv", &csv).unwrap();
        let columns = json!(crate::profile::profile(&table));
        assert!(
            columns.to_string().contains(SECRET),
            "the profile really does carry samples"
        );
        state
            .db
            .add_source(
                pid,
                "clients",
                "csv",
                &csv,
                &columns,
                2,
                "csv",
                "test fixture",
            )
            .unwrap();

        // …and lifted triples that contain it as a literal and inside an IRI.
        let g = state.graph_for(pid).unwrap();
        g.update(&format!(
            "INSERT DATA {{ GRAPH <urn:senclaw:ontology:batch:1> {{ \
             <http://ex/leaky/client/Zzqqxx> a <http://ex/leaky#Client> ; \
             <http://www.w3.org/2000/01/rdf-schema#label> \"{SECRET}\" . }} }}"
        ))
        .unwrap();
        state.persist(pid, &g).unwrap();

        let docs = build_index(&state, pid).unwrap();
        assert!(docs.len() > 12, "index should hold docs + project metadata");
        assert!(
            docs.iter().any(|d| d.id == "source:clients"),
            "the source metadata must be indexed — otherwise this test proves nothing"
        );
        for d in &docs {
            let hay = format!("{} {}", d.title, d.body);
            assert!(
                !hay.contains(SECRET),
                "leaked a data value in doc {}: {}",
                d.id,
                d.body
            );
            assert!(
                !hay.contains("Zzqqxx"),
                "leaked a data-derived IRI in doc {}: {}",
                d.id,
                d.body
            );
        }
        // But it does know the column exists and what kind of column it is.
        let src = docs.iter().find(|d| d.id == "source:clients").unwrap();
        assert!(src.body.contains("client"), "{}", src.body);
        assert!(src.body.contains("2 rows"), "{}", src.body);
    }

    #[test]
    fn redaction_strips_values_but_keeps_identifiers() {
        // A short unquoted-looking identifier survives; a sentence-like literal does not.
        let d = Doc::new(
            "x",
            "source",
            "t",
            "column \"sku\" sample \"Widget Pro Max 2024 edition\"".into(),
            &[],
        );
        assert!(d.body.contains("\"sku\""));
        assert!(!d.body.contains("Widget Pro Max"), "{}", d.body);
        assert!(d.body.contains("‹value›"));
    }
}
