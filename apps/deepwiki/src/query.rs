use crate::db::Db;
use crate::model::{CallLink, Edge, FileRec, IndexStats, Symbol};
use anyhow::Result;
use rusqlite::Row;
use serde::Serialize;
use serde_json::Value;

fn row_to_symbol(r: &Row) -> rusqlite::Result<Symbol> {
    Ok(Symbol {
        id: r.get("id")?,
        file_id: r.get("file_id")?,
        path: r.get("path")?,
        name: r.get("name")?,
        kind: r.get("kind")?,
        parent: r.get("parent")?,
        start_line: r.get("start_line")?,
        end_line: r.get("end_line")?,
        signature: r.get("signature")?,
        doc: r.get("doc")?,
    })
}

const SYM_SELECT: &str = "SELECT s.id, s.file_id, f.path, s.name, s.kind, s.parent, \
    s.start_line, s.end_line, s.signature, s.doc \
    FROM symbols s JOIN files f ON f.id = s.file_id";

pub fn stats(db: &Db) -> Result<IndexStats> {
    db.with_conn(|c| {
        let files: i64 = c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let symbols: i64 = c.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
        let edges: i64 = c.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))?;
        let mut stmt = c.prepare("SELECT lang, COUNT(*) FROM files GROUP BY lang ORDER BY 2 DESC")?;
        let by_lang = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let last_indexed: i64 = c
            .query_row("SELECT value FROM meta WHERE key='last_indexed'", [], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok(IndexStats { files, symbols, edges, by_lang, last_indexed })
    })
}

/// Common words that carry no signal when matching against symbol names.
const STOPWORDS: &[&str] = &[
    "how", "does", "do", "is", "are", "the", "a", "an", "of", "to", "in", "on", "for", "and",
    "or", "what", "where", "when", "why", "this", "that", "it", "work", "works", "use", "used",
    "using", "get", "set", "with", "by", "from", "into",
];

/// Tokenize a query into prefix terms (`foo*`), dropping stopwords.
fn fts_tokens(q: &str) -> Vec<String> {
    q.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() >= 2 && !STOPWORDS.contains(&t.to_ascii_lowercase().as_str()))
        .map(|t| format!("{t}*"))
        .collect()
}

fn run_fts(c: &rusqlite::Connection, expr: &str, limit: u32) -> Vec<Symbol> {
    let sql = format!(
        "{SYM_SELECT} JOIN symbols_fts fts ON fts.symbol_id = s.id \
         WHERE symbols_fts MATCH ?1 ORDER BY rank LIMIT ?2"
    );
    c.prepare(&sql)
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![expr, limit], row_to_symbol)?
                .collect::<std::result::Result<Vec<_>, _>>()
        })
        .unwrap_or_default()
}

/// Full-text search over symbol names/signatures/docs. Tries a precise AND
/// match first, then a high-recall OR match (good for natural-language
/// questions), then a LIKE fallback.
pub fn search(db: &Db, q: &str, limit: u32) -> Result<Vec<Symbol>> {
    let tokens = fts_tokens(q);
    db.with_conn(|c| {
        if !tokens.is_empty() {
            let and_hits = run_fts(c, &tokens.join(" "), limit);
            if !and_hits.is_empty() {
                return Ok(and_hits);
            }
            if tokens.len() > 1 {
                let or_hits = run_fts(c, &tokens.join(" OR "), limit);
                if !or_hits.is_empty() {
                    return Ok(or_hits);
                }
            }
        }
        // Fallback: LIKE on the name using the longest token.
        let needle = q
            .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .filter(|t| !STOPWORDS.contains(&t.to_ascii_lowercase().as_str()))
            .max_by_key(|t| t.len())
            .unwrap_or(q);
        let like = format!("%{}%", needle.replace('%', ""));
        let sql = format!("{SYM_SELECT} WHERE s.name LIKE ?1 ORDER BY length(s.name) LIMIT ?2");
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params![like, limit], row_to_symbol)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn symbols_by_name(db: &Db, name: &str) -> Result<Vec<Symbol>> {
    db.with_conn(|c| {
        let sql = format!("{SYM_SELECT} WHERE s.name = ?1 ORDER BY f.path");
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt
            .query_map([name], row_to_symbol)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn file_outline(db: &Db, path: &str) -> Result<Vec<Symbol>> {
    db.with_conn(|c| {
        let sql = format!("{SYM_SELECT} WHERE f.path = ?1 ORDER BY s.start_line");
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt
            .query_map([path], row_to_symbol)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn list_files(db: &Db) -> Result<Vec<FileRec>> {
    db.with_conn(|c| {
        let mut stmt = c.prepare("SELECT id,path,lang,hash,mtime,loc FROM files ORDER BY path")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(FileRec {
                    id: r.get(0)?,
                    path: r.get(1)?,
                    lang: r.get(2)?,
                    hash: r.get(3)?,
                    mtime: r.get(4)?,
                    loc: r.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// Symbols that call `name` (resolved by name).
pub fn callers(db: &Db, name: &str, limit: u32) -> Result<Vec<CallLink>> {
    db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT DISTINCT f.path, e.src_symbol, e.line FROM edges e \
             JOIN files f ON f.id = e.src_file_id \
             WHERE e.kind='call' AND e.target=?1 LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![name, limit], |r| {
                let path: String = r.get(0)?;
                let sym: Option<String> = r.get(1)?;
                let line: i64 = r.get(2)?;
                Ok((path, sym, line))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows
            .into_iter()
            .map(|(path, sym, line)| CallLink {
                name: sym.unwrap_or_else(|| "<file scope>".into()),
                path,
                start_line: line,
                kind: "caller".into(),
            })
            .collect())
    })
}

/// Functions called by `name` (resolved by name to known symbols).
pub fn callees(db: &Db, name: &str, limit: u32) -> Result<Vec<CallLink>> {
    db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT DISTINCT e.target, e.line FROM edges e \
             WHERE e.kind='call' AND e.src_symbol=?1 LIMIT ?2",
        )?;
        let targets = stmt
            .query_map(rusqlite::params![name, limit], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut out = Vec::new();
        let mut resolver = c.prepare(
            "SELECT f.path, s.kind, s.start_line FROM symbols s JOIN files f ON f.id=s.file_id WHERE s.name=?1 LIMIT 1",
        )?;
        for (target, line) in targets {
            let resolved = resolver
                .query_row([&target], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
                })
                .ok();
            match resolved {
                Some((path, kind, start)) => out.push(CallLink { name: target, path, start_line: start, kind }),
                None => out.push(CallLink { name: target, path: "<external>".into(), start_line: line, kind: "external".into() }),
            }
        }
        Ok(out)
    })
}

/// Transitive set of callers up to `depth` — the "blast radius" of a change.
pub fn blast_radius(db: &Db, name: &str, depth: u32) -> Result<Vec<CallLink>> {
    use std::collections::HashSet;
    let mut seen: HashSet<String> = HashSet::new();
    let mut frontier = vec![name.to_string()];
    let mut out: Vec<CallLink> = Vec::new();
    seen.insert(name.to_string());

    for _ in 0..depth.max(1) {
        let mut next = Vec::new();
        for n in &frontier {
            for c in callers(db, n, 200)? {
                if seen.insert(c.name.clone()) {
                    next.push(c.name.clone());
                    out.push(c);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    Ok(out)
}

pub fn imports_of_file(db: &Db, path: &str) -> Result<Vec<Edge>> {
    db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT e.kind, f.path, e.src_symbol, e.target, e.line FROM edges e \
             JOIN files f ON f.id=e.src_file_id WHERE e.kind='import' AND f.path=?1 ORDER BY e.line",
        )?;
        let rows = stmt
            .query_map([path], |r| {
                Ok(Edge {
                    kind: r.get(0)?,
                    src_path: r.get(1)?,
                    src_symbol: r.get(2)?,
                    target: r.get(3)?,
                    line: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// Read a source snippet from the indexed repo by file path + 1-based line range.
/// `context` adds extra lines above/below for readability.
pub fn snippet(db: &Db, path: &str, start: i64, end: i64, context: i64) -> Result<Value> {
    let root = db
        .get_meta("root")?
        .ok_or_else(|| anyhow::anyhow!("no repo indexed yet"))?;
    let full = std::path::Path::new(&root).join(path);
    let text = std::fs::read_to_string(&full)
        .map_err(|e| anyhow::anyhow!("cannot read {}: {e}", full.display()))?;
    let lines: Vec<&str> = text.lines().collect();
    let from = (start - 1 - context).max(0) as usize;
    let to = ((end + context).max(start) as usize).min(lines.len());
    let body = lines.get(from..to).map(|s| s.join("\n")).unwrap_or_default();
    Ok(serde_json::json!({
        "path": path,
        "start_line": from as i64 + 1,
        "end_line": to as i64,
        "code": body,
    }))
}

/// Read the source of the first symbol matching `name` (with a little context).
pub fn symbol_source(db: &Db, name: &str, context: i64) -> Result<Value> {
    let mut defs = symbols_by_name(db, name)?;
    // Prefer a substantive definition (function/type/...) over a bare module or
    // re-export declaration that merely shares the name.
    fn kind_rank(kind: &str) -> u8 {
        match kind {
            "function" | "method" => 0,
            "class" | "struct" | "trait" | "interface" | "enum" => 1,
            "type" | "const" | "macro" => 2,
            _ => 3, // module, impl, etc.
        }
    }
    defs.sort_by_key(|s| kind_rank(&s.kind));
    let sym = defs
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no symbol named '{name}' in the index"))?;
    let mut snip = snippet(db, &sym.path, sym.start_line, sym.end_line, context)?;
    if let Value::Object(ref mut m) = snip {
        m.insert("name".into(), serde_json::json!(sym.name));
        m.insert("kind".into(), serde_json::json!(sym.kind));
        m.insert("signature".into(), serde_json::json!(sym.signature));
    }
    Ok(snip)
}

/// A previously-indexed repo root (for the UI's quick-pick list).
#[derive(Debug, Clone, Serialize)]
pub struct RootInfo {
    pub path: String,
    pub last_indexed: i64,
    pub files: i64,
    pub symbols: i64,
}

/// All repo roots that have been indexed, most-recent first.
pub fn recent_roots(db: &Db) -> Result<Vec<RootInfo>> {
    db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT path, last_indexed, files, symbols FROM indexed_roots ORDER BY last_indexed DESC",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(RootInfo {
                    path: r.get(0)?,
                    last_indexed: r.get(1)?,
                    files: r.get(2)?,
                    symbols: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// The composite result returned by codegraph_explore.
#[derive(Debug, Serialize)]
pub struct Exploration {
    pub query: String,
    pub matches: Vec<Symbol>,
    pub callers: Vec<CallLink>,
    pub callees: Vec<CallLink>,
    pub blast_radius: Vec<CallLink>,
}

/// One-shot structural context for an agent: find symbols matching `query`,
/// then attach the call graph and blast radius of the best match.
pub fn explore(db: &Db, query: &str, depth: u32) -> Result<Exploration> {
    let mut matches = search(db, query, 12)?;
    // Prefer an exact-name match if present.
    if let Some(pos) = matches.iter().position(|s| s.name == query) {
        matches.swap(0, pos);
    }
    let (callers_v, callees_v, blast) = match matches.first() {
        Some(top) => (
            callers(db, &top.name, 50)?,
            callees(db, &top.name, 50)?,
            blast_radius(db, &top.name, depth)?,
        ),
        None => (vec![], vec![], vec![]),
    };
    Ok(Exploration {
        query: query.to_string(),
        matches,
        callers: callers_v,
        callees: callees_v,
        blast_radius: blast,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::index_repo;

    #[test]
    fn explore_finds_callgraph() {
        let dir = std::env::temp_dir().join(format!("codeindex_q_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("m.rs"),
            "fn helper() {}\nfn run(){ helper(); }\nfn main(){ run(); }\n",
        )
        .unwrap();
        let db = Db::open_memory().unwrap();
        index_repo(&db, &dir).unwrap();

        let ex = explore(&db, "helper", 3).unwrap();
        assert_eq!(ex.matches[0].name, "helper");
        assert!(ex.callers.iter().any(|c| c.name == "run"));
        // blast radius reaches main transitively
        assert!(ex.blast_radius.iter().any(|c| c.name == "main"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
