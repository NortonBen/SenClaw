//! IRI helpers, well-known namespaces, stable IRI minting, and Turtle/SPARQL
//! literal escaping. Writes into the store go through SPARQL `INSERT` strings
//! built here, so correct escaping is the safety boundary against malformed
//! IRIs / injection from raw data.

use sha2::{Digest, Sha256};

/// Standard namespaces every project gets for free.
pub const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
pub const RDFS: &str = "http://www.w3.org/2000/01/rdf-schema#";
pub const OWL: &str = "http://www.w3.org/2002/07/owl#";
pub const XSD: &str = "http://www.w3.org/2001/XMLSchema#";
pub const SKOS: &str = "http://www.w3.org/2004/02/skos/core#";
pub const PROV: &str = "http://www.w3.org/ns/prov#";
pub const SH: &str = "http://www.w3.org/ns/shacl#";

/// Prefix block prepended to every SPARQL query/update the app issues.
pub const PREFIXES: &str = concat!(
    "PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>\n",
    "PREFIX rdfs: <http://www.w3.org/2000/01/rdf-schema#>\n",
    "PREFIX owl: <http://www.w3.org/2002/07/owl#>\n",
    "PREFIX xsd: <http://www.w3.org/2001/XMLSchema#>\n",
    "PREFIX skos: <http://www.w3.org/2004/02/skos/core#>\n",
    "PREFIX prov: <http://www.w3.org/ns/prov#>\n",
    "PREFIX sh: <http://www.w3.org/ns/shacl#>\n",
);

/// Whether a token is an absolute IRI (so it must NOT be treated as a curie or
/// base-relative). Recognizes any `scheme://…` plus common scheme-only IRIs.
/// Domain curies (`ex:Product`) fall through to prefix resolution.
pub fn is_absolute_iri(c: &str) -> bool {
    if c.contains("://") {
        return true;
    }
    match c.split_once(':') {
        Some((scheme, rest)) => {
            !rest.is_empty()
                && matches!(
                    scheme,
                    "urn"
                        | "mailto"
                        | "tag"
                        | "did"
                        | "doi"
                        | "info"
                        | "geo"
                        | "tel"
                        | "data"
                        | "file"
                        | "ftp"
                        | "ftps"
                        | "ws"
                        | "wss"
                )
        }
        None => false,
    }
}

/// Validate a BCP47-ish language tag (`en`, `en-US`, `vi`). Prevents injection
/// through a mapping's `lang` field, which lands unescaped after `@`.
pub fn valid_langtag(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 35
        && s.split('-').all(|part| {
            !part.is_empty() && part.len() <= 8 && part.chars().all(|c| c.is_ascii_alphanumeric())
        })
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
}

/// Validate a SPARQL prefix name (`PN_PREFIX`, conservative subset) so it can't
/// break out of a `PREFIX name: <ns>` prologue declaration.
pub fn valid_prefix_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Expand a possibly-prefixed name (`ex:Product`, `rdfs:label`) or a bare IRI
/// (`http://…`) into a full IRI, using the supplied prefix map. A value with no
/// colon is treated as relative to `base`.
pub fn expand(
    curie: &str,
    prefixes: &std::collections::HashMap<String, String>,
    base: &str,
) -> String {
    let c = curie.trim();
    if is_absolute_iri(c) {
        return c.to_string();
    }
    if let Some((pfx, local)) = c.split_once(':') {
        // Built-in prefixes always available.
        let ns = match pfx {
            "rdf" => Some(RDF),
            "rdfs" => Some(RDFS),
            "owl" => Some(OWL),
            "xsd" => Some(XSD),
            "skos" => Some(SKOS),
            "prov" => Some(PROV),
            "sh" => Some(SH),
            _ => prefixes.get(pfx).map(|s| s.as_str()),
        };
        if let Some(ns) = ns {
            return format!("{ns}{local}");
        }
        // Unknown prefix but looks like scheme-less curie → fall through.
    }
    format!(
        "{}{}",
        base.trim_end_matches(['/', '#']).to_string() + "/",
        encode_segment(c)
    )
}

/// Make one path segment safe inside an IRI: keep unreserved chars, percent-ish
/// replace the rest with `_`. Deterministic → stable IRIs.
pub fn encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '.' | '_' | '~') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

/// Stable IRI for a keyless entity: `sha256(part1..)[..16]` under `base + seg`.
pub fn hashed_iri(base: &str, seg: &str, parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.trim().to_lowercase().as_bytes());
        h.update([0x1f]);
    }
    let digest = hex::encode(&h.finalize()[..8]);
    format!(
        "{}/{}/{}",
        base.trim_end_matches('/'),
        encode_segment(seg),
        digest
    )
}

/// `<iri>` for SPARQL, validating there is no `>` / whitespace that would break
/// the term. Returns None if the IRI is unusable.
pub fn iri_term(iri: &str) -> Option<String> {
    let t = iri.trim();
    if t.is_empty()
        || t.contains(['<', '>', '"', '{', '}', '|', '^', '`', '\\'])
        || t.chars().any(|c| c.is_whitespace())
    {
        return None;
    }
    Some(format!("<{t}>"))
}

/// Escape a string as a SPARQL/Turtle quoted literal body (no surrounding quotes).
pub fn escape_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// Render a typed/plain literal as a SPARQL object term. `datatype` is a full
/// IRI or None (→ plain string). The datatype IRI is validated through
/// `iri_term` (like every other IRI) so it cannot break out of the `^^<…>` term
/// and inject SPARQL; an invalid datatype falls back to a plain string literal.
pub fn literal_term(value: &str, datatype: Option<&str>) -> String {
    let v = value.trim();
    match datatype.and_then(iri_term) {
        None => format!("\"{}\"", escape_literal(value)),
        Some(dt_term) => format!("\"{}\"^^{}", escape_literal(v), dt_term),
    }
}

/// Prepend any missing standard + project prefixes to a user SPARQL query so
/// `rdfs:`, `owl:`, `xsd:`, and the project's `ex:` etc. work without the user
/// having to declare them. Prefixes already declared in the query are left
/// untouched (no duplicate declarations).
pub fn ensure_prefixes(query: &str, extra: &std::collections::HashMap<String, String>) -> String {
    let mut declared = std::collections::HashSet::new();
    let toks: Vec<&str> = query.split_whitespace().collect();
    for i in 0..toks.len() {
        if toks[i].eq_ignore_ascii_case("prefix") {
            if let Some(n) = toks.get(i + 1) {
                declared.insert(n.trim_end_matches(':').to_lowercase());
            }
        }
    }
    let mut prologue = String::new();
    for (name, ns) in [
        ("rdf", RDF),
        ("rdfs", RDFS),
        ("owl", OWL),
        ("xsd", XSD),
        ("skos", SKOS),
        ("prov", PROV),
        ("sh", SH),
    ] {
        if !declared.contains(name) {
            prologue.push_str(&format!("PREFIX {name}: <{ns}>\n"));
        }
    }
    for (name, ns) in extra {
        // Only emit project prefixes whose name and namespace are safe — an
        // unvalidated name/namespace would inject into the query prologue.
        if !declared.contains(&name.to_lowercase()) && valid_prefix_name(name) {
            if let Some(ns_term) = iri_term(ns) {
                prologue.push_str(&format!("PREFIX {name}: {ns_term}\n"));
            }
        }
    }
    format!("{prologue}{query}")
}

/// Substitute `{col}` tokens in a template with row values (already IRI-encoded).
/// Returns None if any referenced column is missing/empty (→ skip the triple).
pub fn apply_template(
    template: &str,
    row: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut col = String::new();
            for c in chars.by_ref() {
                if c == '}' {
                    break;
                }
                col.push(c);
            }
            let val = row.get(col.trim())?;
            if val.trim().is_empty() {
                return None;
            }
            out.push_str(&encode_segment(val));
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn template_and_encode() {
        let mut row = HashMap::new();
        row.insert("sku".to_string(), "AB 12/x".to_string());
        assert_eq!(
            apply_template("product/{sku}", &row).unwrap(),
            "product/AB_12_x"
        );
        row.insert("empty".to_string(), "".to_string());
        assert!(apply_template("x/{empty}", &row).is_none());
        assert!(apply_template("x/{missing}", &row).is_none());
    }

    #[test]
    fn hashed_is_stable() {
        let a = hashed_iri("http://ex", "supplier", &["Acme Ltd"]);
        let b = hashed_iri("http://ex", "supplier", &["  acme ltd "]);
        assert_eq!(a, b);
    }

    #[test]
    fn expand_curie() {
        let mut p = HashMap::new();
        p.insert("ex".to_string(), "http://example.org/shop#".to_string());
        assert_eq!(
            expand("ex:Product", &p, "http://b/"),
            "http://example.org/shop#Product"
        );
        assert_eq!(
            expand("rdfs:label", &p, "http://b/"),
            format!("{RDFS}label")
        );
        assert_eq!(expand("http://x/y", &p, "http://b/"), "http://x/y");
        // absolute non-http schemes are preserved, not mangled to base-relative.
        assert_eq!(expand("urn:isbn:123", &p, "http://b/"), "urn:isbn:123");
        assert_eq!(expand("mailto:a@b.com", &p, "http://b/"), "mailto:a@b.com");
    }

    #[test]
    fn datatype_injection_is_neutralized() {
        // A datatype IRI that tries to break out of `^^<...>` and inject SPARQL
        // fails iri_term validation → falls back to a plain string literal.
        let evil = "http://x> } } ; DROP ALL ; INSERT DATA { GRAPH <urn:x> { <a> <b> <c";
        let term = literal_term("100", Some(evil));
        assert_eq!(term, "\"100\"");
        assert!(!term.contains("DROP"));
        assert!(!term.contains("^^"));
        // a valid datatype still types the literal.
        assert_eq!(
            literal_term("100", Some(XSD_DECIMAL)),
            format!("\"100\"^^<{XSD_DECIMAL}>")
        );
    }

    const XSD_DECIMAL: &str = "http://www.w3.org/2001/XMLSchema#decimal";

    #[test]
    fn langtag_and_prefix_validation() {
        assert!(valid_langtag("en"));
        assert!(valid_langtag("en-US"));
        assert!(valid_langtag("vi"));
        // injection attempt via a langtag is rejected.
        assert!(!valid_langtag(
            "en } } ; DROP ALL ; INSERT DATA { GRAPH <x> {"
        ));
        assert!(!valid_langtag("en\"@"));
        assert!(!valid_langtag(""));

        assert!(valid_prefix_name("ex"));
        assert!(valid_prefix_name("my-ns_2"));
        assert!(!valid_prefix_name("bad name"));
        assert!(!valid_prefix_name("x> <y"));

        // ensure_prefixes drops an unsafe project prefix instead of injecting it.
        let mut bad = HashMap::new();
        bad.insert(
            "evil> <urn:x".to_string(),
            "http://x> } DROP ALL".to_string(),
        );
        let out = ensure_prefixes("SELECT * WHERE {?s ?p ?o}", &bad);
        assert!(!out.contains("DROP"));
        assert!(out.contains("PREFIX rdfs:"));
    }

    #[test]
    fn iri_term_rejects_backslash() {
        assert!(iri_term("http://x/a\\b").is_none());
        assert!(iri_term("http://x/ok").is_some());
    }
}
