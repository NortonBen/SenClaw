use crate::db::Db;
use crate::query;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const WIKI_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS wiki_pages (
    slug       TEXT PRIMARY KEY,
    title      TEXT NOT NULL,
    parent     TEXT,
    content    TEXT NOT NULL,
    ord        INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL
);

-- Saved "Hỏi AI" question/answer history (with the investigation graph).
CREATE TABLE IF NOT EXISTS ask_history (
    id         INTEGER PRIMARY KEY,
    question   TEXT NOT NULL,
    model      TEXT,
    focus      TEXT,
    answer     TEXT NOT NULL,
    data       TEXT NOT NULL DEFAULT '{}',
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ask_history_created ON ask_history(created_at DESC);
"#;

pub fn migrate(db: &Db) -> Result<()> {
    db.with_conn(|c| {
        c.execute_batch(WIKI_SCHEMA)?;
        Ok(())
    })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiPage {
    pub slug: String,
    pub title: String,
    pub parent: Option<String>,
    pub content: String,
    pub ord: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct PageInput {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub parent: Option<String>,
    pub content: String,
    #[serde(default)]
    pub ord: i64,
}

pub fn save_page(db: &Db, p: &PageInput) -> Result<()> {
    db.with_conn(|c| {
        c.execute(
            "INSERT INTO wiki_pages(slug,title,parent,content,ord,updated_at) VALUES(?1,?2,?3,?4,?5,?6) \
             ON CONFLICT(slug) DO UPDATE SET title=excluded.title, parent=excluded.parent, \
             content=excluded.content, ord=excluded.ord, updated_at=excluded.updated_at",
            rusqlite::params![p.slug, p.title, p.parent, p.content, p.ord, now()],
        )?;
        Ok(())
    })
}

pub fn delete_page(db: &Db, slug: &str) -> Result<()> {
    db.with_conn(|c| {
        c.execute("DELETE FROM wiki_pages WHERE slug=?1", [slug])?;
        Ok(())
    })
}

/// All pages, ordered for sidebar rendering (title + slug + parent only).
pub fn list_pages(db: &Db) -> Result<Vec<WikiPage>> {
    db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT slug,title,parent,'',ord,updated_at FROM wiki_pages ORDER BY ord, title",
        )?;
        let rows = stmt
            .query_map([], map_page)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

pub fn get_page(db: &Db, slug: &str) -> Result<Option<WikiPage>> {
    db.with_conn(|c| {
        let page = c
            .query_row(
                "SELECT slug,title,parent,content,ord,updated_at FROM wiki_pages WHERE slug=?1",
                [slug],
                map_page,
            )
            .ok();
        Ok(page)
    })
}

fn map_page(r: &rusqlite::Row) -> rusqlite::Result<WikiPage> {
    Ok(WikiPage {
        slug: r.get(0)?,
        title: r.get(1)?,
        parent: r.get(2)?,
        content: r.get(3)?,
        ord: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

pub fn page_count(db: &Db) -> Result<i64> {
    db.with_conn(|c| Ok(c.query_row("SELECT COUNT(*) FROM wiki_pages", [], |r| r.get(0))?))
}

// ===== Ask history =====

/// Persist a Q&A. `data` is the full API payload ({answer, model, focus, matches, graph}).
pub fn save_ask(
    db: &Db,
    question: &str,
    answer: &str,
    model: Option<&str>,
    focus: Option<&str>,
    data: &Value,
) -> Result<i64> {
    db.with_conn(|c| {
        c.execute(
            "INSERT INTO ask_history(question,model,focus,answer,data,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![question, model, focus, answer, data.to_string(), now()],
        )?;
        Ok(c.last_insert_rowid())
    })
}

/// Lightweight list of past questions (newest first).
pub fn list_ask(db: &Db, limit: u32) -> Result<Value> {
    db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT id, question, model, focus, created_at FROM ask_history ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "question": r.get::<_, String>(1)?,
                    "model": r.get::<_, Option<String>>(2)?,
                    "focus": r.get::<_, Option<String>>(3)?,
                    "created_at": r.get::<_, i64>(4)?,
                }))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(json!(rows))
    })
}

/// Full saved record by id (re-hydrates the stored payload + columns).
pub fn get_ask(db: &Db, id: i64) -> Result<Option<Value>> {
    db.with_conn(|c| {
        let row = c
            .query_row(
                "SELECT id, question, answer, model, focus, data, created_at FROM ask_history WHERE id=?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, i64>(6)?,
                    ))
                },
            )
            .ok();
        Ok(row.map(|(id, question, answer, model, focus, data, created_at)| {
            let mut v: Value = serde_json::from_str(&data).unwrap_or_else(|_| json!({}));
            if let Value::Object(ref mut m) = v {
                m.insert("id".into(), json!(id));
                m.insert("question".into(), json!(question));
                m.insert("answer".into(), json!(answer));
                m.insert("model".into(), json!(model));
                m.insert("focus".into(), json!(focus));
                m.insert("created_at".into(), json!(created_at));
            }
            v
        }))
    })
}

pub fn delete_ask(db: &Db, id: i64) -> Result<()> {
    db.with_conn(|c| {
        c.execute("DELETE FROM ask_history WHERE id=?1", [id])?;
        Ok(())
    })
}

/// A high-level structural summary of the indexed repo — the planning input an
/// agent uses to decide which wiki pages to write.
pub fn outline(db: &Db) -> Result<Value> {
    let stats = query::stats(db)?;
    let root = db.get_meta("root")?;

    // Top files by symbol count.
    let top_files: Vec<Value> = db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT f.path, f.lang, COUNT(s.id) AS n FROM files f \
             LEFT JOIN symbols s ON s.file_id=f.id GROUP BY f.id ORDER BY n DESC LIMIT 25",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(json!({ "path": r.get::<_, String>(0)?, "lang": r.get::<_, String>(1)?, "symbols": r.get::<_, i64>(2)? }))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;

    // Architectural types (classes/structs/traits/interfaces/enums).
    let types: Vec<Value> = db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT s.name, s.kind, f.path FROM symbols s JOIN files f ON f.id=s.file_id \
             WHERE s.kind IN ('class','struct','trait','interface','enum') ORDER BY s.name LIMIT 80",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(json!({ "name": r.get::<_, String>(0)?, "kind": r.get::<_, String>(1)?, "path": r.get::<_, String>(2)? }))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;

    // Likely entry points: most-called symbols that are actually defined in the
    // repo (filters out stdlib/builtin calls like unwrap/map/Ok).
    let entry_points: Vec<Value> = db.with_conn(|c| {
        let mut stmt = c.prepare(
            "SELECT e.target, COUNT(*) AS n FROM edges e \
             WHERE e.kind='call' AND e.target IN (SELECT name FROM symbols) \
             GROUP BY e.target ORDER BY n DESC LIMIT 20",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(json!({ "name": r.get::<_, String>(0)?, "called": r.get::<_, i64>(1)? }))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;

    // Top-level directories.
    let mut dirs: std::collections::BTreeMap<String, i64> = std::collections::BTreeMap::new();
    for f in query::list_files(db)? {
        let top = f.path.split('/').next().unwrap_or("").to_string();
        *dirs.entry(top).or_insert(0) += 1;
    }
    let directories: Vec<Value> = dirs
        .into_iter()
        .map(|(name, files)| json!({ "name": name, "files": files }))
        .collect();

    Ok(json!({
        "root": root,
        "stats": stats,
        "directories": directories,
        "top_files": top_files,
        "architectural_types": types,
        "hot_symbols": entry_points,
    }))
}

/// Source-grounded context for a topic/question — the evidence an agent uses to
/// write a page or answer a question without hallucinating.
pub fn context(db: &Db, query_str: &str, depth: u32) -> Result<Value> {
    let ex = query::explore(db, query_str, depth)?;

    // For the strongest matches, include the file outline so the agent sees
    // sibling symbols and structure.
    let mut file_outlines: Vec<Value> = Vec::new();
    let mut seen_files = std::collections::HashSet::new();
    for sym in ex.matches.iter().take(4) {
        if seen_files.insert(sym.path.clone()) {
            let outline = query::file_outline(db, &sym.path)?;
            let imports = query::imports_of_file(db, &sym.path)?;
            file_outlines.push(json!({
                "path": sym.path,
                "imports": imports,
                "symbols": outline,
            }));
        }
    }

    Ok(json!({
        "query": query_str,
        "matches": ex.matches,
        "callers": ex.callers,
        "callees": ex.callees,
        "files": file_outlines,
        "instruction": "Answer ONLY from the evidence above. Cite file paths and line numbers. If the evidence is insufficient, say so and suggest deepwiki_context queries to run next.",
    }))
}
