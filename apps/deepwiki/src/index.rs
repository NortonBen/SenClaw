use crate::{db::Db, lang, parse};
use anyhow::Result;
use ignore::WalkBuilder;
use serde::Serialize;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default, Serialize)]
pub struct IndexReport {
    pub root: String,
    pub scanned: usize,
    pub indexed: usize,
    pub skipped: usize,
    pub removed: usize,
    pub symbols: usize,
    pub edges: usize,
    pub errors: Vec<String>,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn hash_str(s: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Index (or re-index) a repository rooted at `root`. Incremental: files whose
/// mtime and content hash are unchanged are skipped; deleted files are pruned.
pub fn index_repo(db: &Db, root: &Path) -> Result<IndexReport> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let root_str = root.to_string_lossy().to_string();

    // If the indexed root changed, wipe the previous index.
    if db.get_meta("root")?.as_deref() != Some(root_str.as_str()) {
        db.with_conn(|c| {
            c.execute_batch("DELETE FROM files; DELETE FROM symbols; DELETE FROM edges; DELETE FROM symbols_fts;")?;
            Ok(())
        })?;
        db.set_meta("root", &root_str)?;
    }

    let mut report = IndexReport { root: root_str.clone(), ..Default::default() };
    let mut seen: HashSet<String> = HashSet::new();

    let walker = WalkBuilder::new(&root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .parents(true)
        .build();

    for dent in walker {
        let dent = match dent {
            Ok(d) => d,
            Err(_) => continue,
        };
        if !dent.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = dent.path();
        let rel = path.strip_prefix(&root).unwrap_or(path).to_string_lossy().to_string();
        let Some(lang_name) = lang::lang_for_path(&rel) else { continue };
        report.scanned += 1;
        seen.insert(rel.clone());

        let mtime = path
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        // Quick skip on unchanged mtime.
        let existing = db.with_conn(|c| {
            let row = c
                .query_row(
                    "SELECT id, mtime, hash FROM files WHERE path=?1",
                    [&rel],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)),
                )
                .ok();
            Ok(row)
        })?;

        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue, // binary / unreadable
        };
        let hash = hash_str(&src);
        if let Some((_, old_mtime, old_hash)) = &existing {
            if *old_mtime == mtime && *old_hash == hash {
                report.skipped += 1;
                continue;
            }
        }

        let parsed = match parse::parse(lang_name, &src) {
            Ok(p) => p,
            Err(e) => {
                report.errors.push(format!("{rel}: {e}"));
                continue;
            }
        };
        let loc = src.lines().count() as i64;

        db.with_conn_mut(|c| {
            let tx = c.transaction()?;
            // Replace the file row + cascade-delete its symbols/edges.
            if let Some((old_id, _, _)) = &existing {
                tx.execute("DELETE FROM symbols_fts WHERE symbol_id IN (SELECT id FROM symbols WHERE file_id=?1)", [old_id])?;
                tx.execute("DELETE FROM files WHERE id=?1", [old_id])?;
            }
            tx.execute(
                "INSERT INTO files(path,lang,hash,mtime,loc,indexed_at) VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![rel, lang_name, hash, mtime, loc, now()],
            )?;
            let file_id = tx.last_insert_rowid();

            for s in &parsed.symbols {
                tx.execute(
                    "INSERT INTO symbols(file_id,name,kind,parent,start_line,end_line,signature,doc) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                    rusqlite::params![file_id, s.name, s.kind, s.parent, s.start_line, s.end_line, s.signature, s.doc],
                )?;
                let sym_id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO symbols_fts(name,signature,doc,kind,symbol_id) VALUES(?1,?2,?3,?4,?5)",
                    rusqlite::params![s.name, s.signature, s.doc.clone().unwrap_or_default(), s.kind, sym_id],
                )?;
            }
            for e in &parsed.edges {
                tx.execute(
                    "INSERT INTO edges(src_file_id,src_symbol,kind,target,line) VALUES(?1,?2,?3,?4,?5)",
                    rusqlite::params![file_id, e.src_symbol, e.kind, e.target, e.line],
                )?;
            }
            tx.commit()?;
            Ok(())
        })?;

        report.indexed += 1;
        report.symbols += parsed.symbols.len();
        report.edges += parsed.edges.len();
    }

    // Prune files that disappeared.
    let known: Vec<(i64, String)> = db.with_conn(|c| {
        let mut stmt = c.prepare("SELECT id, path FROM files")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    for (id, path) in known {
        if !seen.contains(&path) {
            db.with_conn(|c| {
                c.execute("DELETE FROM symbols_fts WHERE symbol_id IN (SELECT id FROM symbols WHERE file_id=?1)", [id])?;
                c.execute("DELETE FROM files WHERE id=?1", [id])?;
                Ok(())
            })?;
            report.removed += 1;
        }
    }

    db.set_meta("last_indexed", &now().to_string())?;

    // Record this root in the indexed-history table (for quick re-selection).
    db.with_conn(|c| {
        let files: i64 = c.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
        let symbols: i64 = c.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))?;
        c.execute(
            "INSERT INTO indexed_roots(path,last_indexed,files,symbols) VALUES(?1,?2,?3,?4) \
             ON CONFLICT(path) DO UPDATE SET last_indexed=excluded.last_indexed, \
             files=excluded.files, symbols=excluded.symbols",
            rusqlite::params![root_str, now(), files, symbols],
        )?;
        Ok(())
    })?;

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_a_tiny_repo() {
        let dir = std::env::temp_dir().join(format!("codeindex_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "fn helper() {}\nfn main(){ helper(); }\n").unwrap();
        std::fs::write(dir.join("b.py"), "def util():\n    return 1\n").unwrap();

        let db = Db::open_memory().unwrap();
        let rep = index_repo(&db, &dir).unwrap();
        assert_eq!(rep.indexed, 2);
        assert!(rep.symbols >= 3);

        // Re-index is incremental: nothing changed.
        let rep2 = index_repo(&db, &dir).unwrap();
        assert_eq!(rep2.skipped, 2);
        assert_eq!(rep2.indexed, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
