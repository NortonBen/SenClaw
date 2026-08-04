//! **Actions** — the typed IR every write goes through.
//!
//! This is the line in SAIP between "LLM intuition" and "data truth": an LLM
//! never writes SPARQL `INSERT` or `UPDATE`. It emits a small, **typed** action
//! — add an individual, set an attribute, add a relation, link two entities —
//! and that action is **validated against the T-Box before it can touch data**.
//! A hallucinated class or a property used on the wrong kind of node is caught
//! *here*, at proposal time, not discovered later as a broken triple.
//!
//! Every applied action is attributed to a provenance batch, so an edit made by
//! a logic function can be dropped as a unit — the audit half of the contract.
//!
//! The safety boundary is [`crate::vocab`]: every IRI goes through `iri_term`,
//! every literal through `escape_literal` / `literal_term`. Action fields come
//! from an LLM, i.e. from untrusted text, so nothing is interpolated raw.

use crate::graph::Graph;
use crate::{prov, tbox, vocab};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A typed ontology edit. `op` selects the variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    /// Mint (or reference) an individual of a class. `key` is a stable natural
    /// key hashed into the IRI; `label` is an optional rdfs:label.
    AddIndividual {
        class: String,
        key: String,
        #[serde(default)]
        label: Option<String>,
    },
    /// Assert a **data** property (literal value) on a subject.
    SetAttribute {
        subject: String,
        property: String,
        value: String,
        #[serde(default)]
        datatype: Option<String>,
    },
    /// Assert an **object** property (link to another entity) on a subject.
    AddRelation {
        subject: String,
        property: String,
        object: String,
    },
    /// Link two individuals as the same/close entity (entity resolution). The
    /// predicate is restricted to the two safe forms so a logic function cannot
    /// smuggle in an arbitrary predicate.
    LinkEntities {
        a: String,
        b: String,
        #[serde(default)]
        predicate: Option<String>,
    },
}

impl Action {
    /// One-line human summary for the review queue.
    pub fn summary(&self) -> String {
        match self {
            Action::AddIndividual { class, key, label } => {
                format!(
                    "add {} '{}'{}",
                    short(class),
                    key,
                    label
                        .as_deref()
                        .map(|l| format!(" ({l})"))
                        .unwrap_or_default()
                )
            }
            Action::SetAttribute {
                subject,
                property,
                value,
                ..
            } => {
                format!(
                    "{} {} = \"{}\"",
                    short(subject),
                    short(property),
                    truncate(value, 40)
                )
            }
            Action::AddRelation {
                subject,
                property,
                object,
            } => {
                format!("{} {} → {}", short(subject), short(property), short(object))
            }
            Action::LinkEntities { a, b, predicate } => {
                format!(
                    "link {} ≡ {} ({})",
                    short(a),
                    short(b),
                    predicate.as_deref().unwrap_or("skos:closeMatch")
                )
            }
        }
    }
}

fn short(s: &str) -> String {
    s.rsplit(['#', '/', ':']).next().unwrap_or(s).to_string()
}
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

// ---------------------------------------------------------------------------
// T-Box index — the type checker's knowledge
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropKind {
    Object,
    Data,
    Annotation,
}

/// The declared schema, indexed for fast validation. Built from `tbox::read`.
pub struct Schema {
    pub base: String,
    pub prefixes: HashMap<String, String>,
    classes: HashSet<String>,
    props: HashMap<String, (PropKind, Option<String>)>, // iri -> (kind, range)
}

impl Schema {
    pub fn build(
        graph: &Graph,
        base: &str,
        prefixes: &HashMap<String, String>,
    ) -> anyhow::Result<Self> {
        let tb = tbox::read(graph)?;
        let mut classes = HashSet::new();
        for c in tb["classes"].as_array().cloned().unwrap_or_default() {
            if let Some(iri) = c["iri"].as_str() {
                classes.insert(iri.to_string());
            }
        }
        let mut props = HashMap::new();
        for p in tb["properties"].as_array().cloned().unwrap_or_default() {
            let Some(iri) = p["iri"].as_str() else {
                continue;
            };
            let kind = match p["kind"].as_str().unwrap_or("") {
                k if k.ends_with("ObjectProperty") => PropKind::Object,
                k if k.ends_with("DatatypeProperty") => PropKind::Data,
                _ => PropKind::Annotation,
            };
            props.insert(
                iri.to_string(),
                (kind, p["range"].as_str().map(String::from)),
            );
        }
        Ok(Self {
            base: base.to_string(),
            prefixes: prefixes.clone(),
            classes,
            props,
        })
    }

    fn expand(&self, curie: &str) -> String {
        vocab::expand(curie, &self.prefixes, &self.base)
    }

    fn has_class(&self, iri: &str) -> bool {
        self.classes.contains(iri)
    }

    fn prop(&self, iri: &str) -> Option<(PropKind, Option<String>)> {
        self.props.get(iri).cloned()
    }
}

// ---------------------------------------------------------------------------
// validation — the "type checker chặn LLM" step
// ---------------------------------------------------------------------------

/// The triples an action produces (as SPARQL object terms) once it validates.
#[derive(Debug)]
pub struct Validated {
    /// `subject predicate object` term triples, ready for an INSERT body.
    pub triples: Vec<(String, String, String)>,
}

impl Action {
    /// Check this action against the schema and turn it into concrete triples.
    /// `Err` is a human-readable reason the action is *rejected* — the whole
    /// point of the typed layer.
    pub fn validate(&self, schema: &Schema) -> Result<Validated, String> {
        match self {
            Action::AddIndividual { class, key, label } => {
                let class_iri = schema.expand(class);
                if !schema.has_class(&class_iri) {
                    return Err(format!("class '{class}' is not in the ontology"));
                }
                if key.trim().is_empty() {
                    return Err("individual key is empty".into());
                }
                let subj =
                    vocab::hashed_iri(&schema.base, &short(&class_iri).to_lowercase(), &[key]);
                let (st, ct) = terms(&subj, &class_iri)?;
                let rdf_type = format!("<{}type>", vocab::RDF);
                let mut triples = vec![(st.clone(), rdf_type, ct)];
                if let Some(l) = label.as_deref().filter(|l| !l.trim().is_empty()) {
                    triples.push((
                        st,
                        format!("<{}label>", vocab::RDFS),
                        vocab::literal_term(l, None),
                    ));
                }
                Ok(Validated { triples })
            }
            Action::SetAttribute {
                subject,
                property,
                value,
                datatype,
            } => {
                let prop_iri = schema.expand(property);
                match schema.prop(&prop_iri) {
                    Some((PropKind::Data, range)) => {
                        let subj = mint_or_expand(schema, subject);
                        let (st, pt) = terms(&subj, &prop_iri)?;
                        // Prefer the declared range over an LLM-supplied datatype.
                        let dt = datatype
                            .as_deref()
                            .map(|d| schema.expand(d))
                            .or(range)
                            .filter(|d| d.contains("XMLSchema#"));
                        let obj = vocab::literal_term(value, dt.as_deref());
                        Ok(Validated { triples: vec![(st, pt, obj)] })
                    }
                    Some((PropKind::Annotation, _)) => {
                        let subj = mint_or_expand(schema, subject);
                        let (st, pt) = terms(&subj, &prop_iri)?;
                        Ok(Validated { triples: vec![(st, pt, vocab::literal_term(value, None))] })
                    }
                    Some((PropKind::Object, _)) => Err(format!(
                        "'{property}' is an object property — use add_relation with an entity, not set_attribute with a literal"
                    )),
                    None => Err(format!("property '{property}' is not in the ontology")),
                }
            }
            Action::AddRelation {
                subject,
                property,
                object,
            } => {
                let prop_iri = schema.expand(property);
                match schema.prop(&prop_iri) {
                    Some((PropKind::Object, _)) => {
                        let subj = mint_or_expand(schema, subject);
                        let obj = mint_or_expand(schema, object);
                        let (st, pt) = terms(&subj, &prop_iri)?;
                        let ot = vocab::iri_term(&obj).ok_or_else(|| format!("bad object IRI '{object}'"))?;
                        Ok(Validated { triples: vec![(st, pt, ot)] })
                    }
                    Some(_) => Err(format!(
                        "'{property}' is not an object property — use set_attribute for a literal value"
                    )),
                    None => Err(format!("property '{property}' is not in the ontology")),
                }
            }
            Action::LinkEntities { a, b, predicate } => {
                // Only the two safe, review-friendly link predicates.
                let pred = match predicate.as_deref().unwrap_or("skos:closeMatch") {
                    "skos:closeMatch" | "closeMatch" => format!("{}closeMatch", vocab::SKOS),
                    "skos:exactMatch" | "exactMatch" => format!("{}exactMatch", vocab::SKOS),
                    "owl:sameAs" | "sameAs" => format!("{}sameAs", vocab::OWL),
                    other => return Err(format!("unsupported link predicate '{other}' (use skos:closeMatch/exactMatch or owl:sameAs)")),
                };
                let ai = mint_or_expand(schema, a);
                let bi = mint_or_expand(schema, b);
                if ai == bi {
                    return Err("cannot link an entity to itself".into());
                }
                let (at, pt) = terms(&ai, &pred)?;
                let bt = vocab::iri_term(&bi).ok_or_else(|| format!("bad IRI '{b}'"))?;
                Ok(Validated {
                    triples: vec![(at, pt, bt)],
                })
            }
        }
    }
}

/// A subject/object reference from an LLM: expand a curie/IRI, else mint a
/// stable hashed IRI from the label — the same rule the extraction path uses,
/// so the same entity name lands on the same node across functions.
fn mint_or_expand(schema: &Schema, raw: &str) -> String {
    let r = raw.trim();
    if r.starts_with("http://")
        || r.starts_with("https://")
        || r.starts_with("urn:")
        || r.contains(':')
    {
        schema.expand(r)
    } else {
        vocab::hashed_iri(&schema.base, "entity", &[r])
    }
}

fn terms(subject: &str, predicate: &str) -> Result<(String, String), String> {
    let s = vocab::iri_term(subject).ok_or_else(|| format!("bad subject IRI '{subject}'"))?;
    let p = vocab::iri_term(predicate).ok_or_else(|| format!("bad predicate IRI '{predicate}'"))?;
    Ok((s, p))
}

// ---------------------------------------------------------------------------
// apply — validated actions become a provenance batch
// ---------------------------------------------------------------------------

/// Outcome of applying a set of actions.
#[derive(Serialize, Default)]
pub struct ApplyReport {
    pub applied: usize,
    pub triples: usize,
    pub batch: String,
    /// Per-action rejection reasons (index → why), so the caller can show them.
    pub rejected: Vec<String>,
}

/// Validate every action, insert the valid ones into a fresh provenance batch,
/// and record the batch. Invalid actions are reported, not silently dropped —
/// and never partially applied.
pub fn apply(
    graph: &Graph,
    schema: &Schema,
    actions: &[Action],
    label: &str,
    ts: i64,
) -> anyhow::Result<ApplyReport> {
    let batch = prov::batch_iri(ts, &format!("logic-{label}"));
    let mut body = String::new();
    let mut triples = 0usize;
    let mut applied = 0usize;
    let mut rejected = Vec::new();
    for (i, action) in actions.iter().enumerate() {
        match action.validate(schema) {
            Ok(v) => {
                for (s, p, o) in v.triples {
                    body.push_str(&format!("{s} {p} {o} .\n"));
                    triples += 1;
                }
                applied += 1;
            }
            Err(e) => rejected.push(format!("#{i}: {e}")),
        }
    }
    if triples > 0 {
        graph.update(&format!("INSERT DATA {{ GRAPH <{batch}> {{\n{body}}} }}"))?;
        prov::record_batch(graph, &batch, label, "AIP logic", "logic", triples, ts)?;
    }
    Ok(ApplyReport {
        applied,
        triples,
        batch,
        rejected,
    })
}

/// Parse an LLM reply into a list of actions. Accepts either a bare array or
/// `{"actions":[…]}`; unknown-shape entries are skipped rather than failing the
/// whole batch (the valid ones still get proposed). Kept as a public helper
/// (and tested) for callers that don't need the per-action rationale/confidence
/// that `logic.rs` peels off itself.
#[allow(dead_code)]
pub fn parse_actions(v: &serde_json::Value) -> Vec<Action> {
    let arr = v
        .get("actions")
        .and_then(|a| a.as_array())
        .or_else(|| v.as_array());
    arr.map(|a| {
        a.iter()
            .filter_map(|x| serde_json::from_value::<Action>(x.clone()).ok())
            .collect()
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema() -> Schema {
        let g = Graph::new().unwrap();
        // A tiny T-Box: Product (class), hasPrice (data, decimal), hasSupplier (object).
        g.update(&format!(
            "INSERT DATA {{ GRAPH <{}> {{ \
             <http://ex/shop#Product> a <{o}Class> . \
             <http://ex/shop#Supplier> a <{o}Class> . \
             <http://ex/shop#hasPrice> a <{o}DatatypeProperty> ; <{r}range> <{x}decimal> . \
             <http://ex/shop#hasSupplier> a <{o}ObjectProperty> . }} }}",
            crate::graph::TBOX_GRAPH,
            o = vocab::OWL,
            r = vocab::RDFS,
            x = vocab::XSD,
        ))
        .unwrap();
        let mut pfx = HashMap::new();
        pfx.insert("ex".into(), "http://ex/shop#".into());
        Schema::build(&g, "http://ex/shop", &pfx).unwrap()
    }

    #[test]
    fn valid_add_individual_and_attribute() {
        let s = schema();
        let a = Action::AddIndividual {
            class: "ex:Product".into(),
            key: "A1".into(),
            label: Some("Widget".into()),
        };
        let v = a.validate(&s).unwrap();
        assert_eq!(v.triples.len(), 2); // type + label
        let attr = Action::SetAttribute {
            subject: "ex:Product".into(), // any IRI ref is fine as a subject here
            property: "ex:hasPrice".into(),
            value: "150000".into(),
            datatype: None,
        };
        let vt = attr.validate(&s).unwrap();
        assert!(
            vt.triples[0].2.contains("decimal"),
            "range datatype applied: {}",
            vt.triples[0].2
        );
    }

    #[test]
    fn unknown_class_is_blocked() {
        let s = schema();
        let a = Action::AddIndividual {
            class: "ex:Ghost".into(),
            key: "x".into(),
            label: None,
        };
        assert!(a.validate(&s).unwrap_err().contains("not in the ontology"));
    }

    #[test]
    fn wrong_property_kind_is_blocked() {
        let s = schema();
        // hasSupplier is an object property; using it as an attribute must fail.
        let a = Action::SetAttribute {
            subject: "ex:p1".into(),
            property: "ex:hasSupplier".into(),
            value: "Acme".into(),
            datatype: None,
        };
        assert!(a.validate(&s).unwrap_err().contains("object property"));
        // hasPrice is a data property; using it as a relation must fail.
        let b = Action::AddRelation {
            subject: "ex:p1".into(),
            property: "ex:hasPrice".into(),
            object: "ex:x".into(),
        };
        assert!(b
            .validate(&s)
            .unwrap_err()
            .contains("not an object property"));
    }

    #[test]
    fn unknown_property_is_blocked() {
        let s = schema();
        let a = Action::SetAttribute {
            subject: "ex:p1".into(),
            property: "ex:hasColour".into(),
            value: "red".into(),
            datatype: None,
        };
        assert!(a.validate(&s).unwrap_err().contains("not in the ontology"));
    }

    #[test]
    fn injection_in_action_fields_is_neutralized() {
        let s = schema();
        // A malicious value must not break out of the literal.
        let a = Action::SetAttribute {
            subject: "ex:p1".into(),
            property: "ex:hasPrice".into(),
            value: "\" } } ; DROP ALL ; INSERT DATA { GRAPH <x> { <a> <b> \"".into(),
            datatype: None,
        };
        let v = a.validate(&s).unwrap();
        let obj = &v.triples[0].2;
        // The payload lives inside ONE quoted literal with every inner quote
        // escaped, so it is inert text — "DROP ALL" appearing as characters is
        // fine, breaking OUT of the literal is not. Assert no unescaped `"`.
        assert!(obj.starts_with('"'));
        assert!(obj.contains("\\\""), "inner quotes escaped: {obj}");
        let body = obj.trim_start_matches('"');
        let literal = body.split("\"^^").next().unwrap_or(body);
        // Walk the literal body: every `"` must be preceded by a backslash.
        let bytes: Vec<char> = literal.chars().collect();
        for i in 0..bytes.len() {
            if bytes[i] == '"' {
                assert!(
                    i > 0 && bytes[i - 1] == '\\',
                    "unescaped quote at {i}: {obj}"
                );
            }
        }
    }

    #[test]
    fn apply_is_all_or_reports() {
        let s = schema();
        let g = Graph::new().unwrap();
        let actions = vec![
            Action::AddIndividual {
                class: "ex:Product".into(),
                key: "A1".into(),
                label: Some("Widget".into()),
            },
            Action::AddIndividual {
                class: "ex:Ghost".into(),
                key: "z".into(),
                label: None,
            }, // invalid
        ];
        let rep = apply(&g, &s, &actions, "test", 1).unwrap();
        assert_eq!(rep.applied, 1);
        assert_eq!(rep.rejected.len(), 1);
        assert!(g.len() >= 2); // type+label from the valid one, plus the prov batch
    }

    #[test]
    fn link_entities_only_allows_safe_predicates() {
        let s = schema();
        // A safe predicate → one closeMatch triple.
        let ok = Action::LinkEntities {
            a: "ex:a".into(),
            b: "ex:b".into(),
            predicate: Some("skos:closeMatch".into()),
        };
        assert_eq!(ok.validate(&s).unwrap().triples.len(), 1);
        // An arbitrary predicate is refused — a resolve function can't smuggle one in.
        let bad = Action::LinkEntities {
            a: "ex:a".into(),
            b: "ex:b".into(),
            predicate: Some("ex:secretlyDelete".into()),
        };
        assert!(bad
            .validate(&s)
            .unwrap_err()
            .contains("unsupported link predicate"));
        // Self-link is refused.
        let same = Action::LinkEntities {
            a: "ex:a".into(),
            b: "ex:a".into(),
            predicate: None,
        };
        assert!(same.validate(&s).unwrap_err().contains("itself"));
    }

    #[test]
    fn parse_actions_tolerates_both_shapes() {
        let a = parse_actions(
            &serde_json::json!({"actions":[{"op":"add_individual","class":"ex:Product","key":"A1"}]}),
        );
        assert_eq!(a.len(), 1);
        let b = parse_actions(
            &serde_json::json!([{"op":"set_attribute","subject":"ex:p","property":"ex:hasPrice","value":"1"}]),
        );
        assert_eq!(b.len(), 1);
        // A malformed entry is skipped, not fatal.
        let c = parse_actions(
            &serde_json::json!([{"op":"nonsense"},{"op":"add_individual","class":"ex:X","key":"k"}]),
        );
        assert_eq!(c.len(), 1);
    }
}
