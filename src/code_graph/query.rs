//! Graph query functions — callers, impact analysis, dependency tracing.

use anyhow::Result;
use rusqlite::params;
use std::collections::HashSet;
use std::sync::Arc;

use crate::db::Db;

use super::types::{CallerInfo, CodeNode, ImpactNode, NodeKind};

pub struct GraphQuery {
    db: Arc<Db>,
}

impl GraphQuery {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }

    // ─── Find symbol ─────────────────────────────────────────────────────────

    pub fn find_symbol(&self, project_id: &str, name: &str, file_hint: Option<&str>) -> Result<Vec<CodeNode>> {
        self.db.with_conn(|conn| {
            if let Some(file) = file_hint {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, file_path, name, kind, signature, start_line, end_line, language \
                     FROM cg_symbols WHERE project_id=?1 AND name=?2 AND file_path LIKE ?3"
                )?;
                let rows = stmt.query_map(params![project_id, name, format!("%{file}%")], map_symbol)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, project_id, file_path, name, kind, signature, start_line, end_line, language \
                     FROM cg_symbols WHERE project_id=?1 AND name=?2"
                )?;
                let rows = stmt.query_map(params![project_id, name], map_symbol)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            }
        })
    }

    // ─── Callers ─────────────────────────────────────────────────────────────

    /// Trả về tất cả nơi gọi đến `symbol_name` (CALLS relationship).
    pub fn find_callers(&self, project_id: &str, symbol_name: &str) -> Result<Vec<CallerInfo>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT e.from_name, e.from_file, COALESCE(s.kind, 'unknown'), e.at_line
                 FROM cg_edges e
                 LEFT JOIN cg_symbols s ON s.id = e.from_sym_id
                 WHERE e.project_id=?1 AND e.to_name=?2 AND e.kind='calls'
                 ORDER BY e.from_file, e.at_line"
            )?;
            let rows = stmt.query_map(params![project_id, symbol_name], |row| {
                Ok(CallerInfo {
                    caller_name: row.get(0)?,
                    caller_file: row.get(1)?,
                    caller_kind: row.get(2)?,
                    at_line: row.get::<_, i64>(3)? as u32,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ─── Callees ─────────────────────────────────────────────────────────────

    /// Hàm nào được `symbol_name` gọi?
    pub fn find_callees(&self, project_id: &str, symbol_name: &str) -> Result<Vec<CallerInfo>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT e.to_name, COALESCE(e.to_file, ''), COALESCE(s.kind, 'unknown'), e.at_line
                 FROM cg_edges e
                 LEFT JOIN cg_symbols s ON s.id = e.to_sym_id
                 WHERE e.project_id=?1 AND e.from_name=?2 AND e.kind='calls'
                 ORDER BY e.at_line"
            )?;
            let rows = stmt.query_map(params![project_id, symbol_name], |row| {
                Ok(CallerInfo {
                    caller_name: row.get(0)?,
                    caller_file: row.get(1)?,
                    caller_kind: row.get(2)?,
                    at_line: row.get::<_, i64>(3)? as u32,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ─── Impact analysis (BFS) ───────────────────────────────────────────────

    /// Blast radius: những symbol nào bị ảnh hưởng nếu `symbol_name` thay đổi interface?
    /// Traverses CALLS + IMPORTS edges ngược lại (ai phụ thuộc vào symbol này).
    pub fn impact_analysis(&self, project_id: &str, symbol_name: &str, max_depth: u32) -> Result<Vec<ImpactNode>> {
        let mut visited: HashSet<String> = HashSet::new();
        let mut result: Vec<ImpactNode> = Vec::new();
        visited.insert(symbol_name.to_string());

        self.bfs_impact(project_id, symbol_name, 1, max_depth, &mut visited, &mut result)?;
        Ok(result)
    }

    fn bfs_impact(
        &self,
        project_id: &str,
        target: &str,
        depth: u32,
        max_depth: u32,
        visited: &mut HashSet<String>,
        result: &mut Vec<ImpactNode>,
    ) -> Result<()> {
        if depth > max_depth { return Ok(()); }

        let callers = self.find_callers(project_id, target)?;
        for caller in callers {
            if visited.insert(caller.caller_name.clone()) {
                result.push(ImpactNode {
                    name: caller.caller_name.clone(),
                    file: caller.caller_file.clone(),
                    kind: caller.caller_kind.clone(),
                    depth,
                    via: format!("calls {target}"),
                });
                self.bfs_impact(project_id, &caller.caller_name, depth + 1, max_depth, visited, result)?;
            }
        }

        // Also traverse imports: who imports the file containing `target`?
        let importers = self.find_importers(project_id, target)?;
        for imp in importers {
            let key = format!("file:{}", imp.caller_file);
            if visited.insert(key) {
                result.push(ImpactNode {
                    name: imp.caller_name,
                    file: imp.caller_file.clone(),
                    kind: "file".to_string(),
                    depth,
                    via: format!("imports {target}"),
                });
            }
        }

        Ok(())
    }

    fn find_importers(&self, project_id: &str, target_name: &str) -> Result<Vec<CallerInfo>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT e.from_name, e.from_file, 'import', e.at_line
                 FROM cg_edges e
                 WHERE e.project_id=?1 AND e.to_name LIKE ?2 AND e.kind='imports'"
            )?;
            let rows = stmt.query_map(params![project_id, format!("%{target_name}%")], |row| {
                Ok(CallerInfo {
                    caller_name: row.get(0)?,
                    caller_file: row.get(1)?,
                    caller_kind: row.get(2)?,
                    at_line: row.get::<_, i64>(3)? as u32,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ─── Call tree trace ─────────────────────────────────────────────────────

    pub fn trace_call_tree(
        &self,
        project_id: &str,
        entry: &str,
        max_depth: u32,
    ) -> Result<Vec<ImpactNode>> {
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        visited.insert(entry.to_string());
        self.dfs_trace(project_id, entry, 0, max_depth, &mut visited, &mut result)?;
        Ok(result)
    }

    fn dfs_trace(
        &self,
        project_id: &str,
        from: &str,
        depth: u32,
        max_depth: u32,
        visited: &mut HashSet<String>,
        result: &mut Vec<ImpactNode>,
    ) -> Result<()> {
        if depth >= max_depth { return Ok(()); }

        let callees = self.find_callees(project_id, from)?;
        for callee in callees {
            if visited.insert(callee.caller_name.clone()) {
                result.push(ImpactNode {
                    name: callee.caller_name.clone(),
                    file: callee.caller_file.clone(),
                    kind: callee.caller_kind.clone(),
                    depth: depth + 1,
                    via: format!("called by {from}"),
                });
                self.dfs_trace(project_id, &callee.caller_name, depth + 1, max_depth, visited, result)?;
            }
        }
        Ok(())
    }

    // ─── File dependencies ────────────────────────────────────────────────────

    pub fn file_dependencies(&self, project_id: &str, file_path: &str) -> Result<Vec<String>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT to_name FROM cg_edges
                 WHERE project_id=?1 AND from_file=?2 AND kind='imports'"
            )?;
            let rows = stmt.query_map(params![project_id, file_path], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(rows)
        })
    }

    pub fn file_dependents(&self, project_id: &str, file_path: &str) -> Result<Vec<String>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT from_file FROM cg_edges
                 WHERE project_id=?1 AND to_file=?2 AND kind='imports'"
            )?;
            let rows = stmt.query_map(params![project_id, file_path], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(rows)
        })
    }

    // ─── Full-text search ─────────────────────────────────────────────────────

    pub fn search_symbols(&self, project_id: &str, query: &str, limit: u32) -> Result<Vec<CodeNode>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT s.id, s.project_id, s.file_path, s.name, s.kind, s.signature, s.start_line, s.end_line, s.language
                 FROM cg_symbols_fts fts
                 JOIN cg_symbols s ON s.id = fts.rowid
                 WHERE fts MATCH ?1 AND s.project_id=?2
                 LIMIT ?3"
            )?;
            let rows = stmt.query_map(params![query, project_id, limit], map_symbol)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ─── Skeleton (for context building) ─────────────────────────────────────

    /// Returns compact signature list for a file — used to give agent
    /// a skeleton without reading full file content.
    pub fn file_skeleton(&self, project_id: &str, file_path: &str) -> Result<Vec<String>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT kind, name, signature, start_line FROM cg_symbols
                 WHERE project_id=?1 AND file_path=?2
                 ORDER BY start_line"
            )?;
            let rows = stmt.query_map(params![project_id, file_path], |row| {
                let kind: String = row.get(0)?;
                let name: String = row.get(1)?;
                let sig: Option<String> = row.get(2)?;
                let line: i64 = row.get(3)?;
                Ok(format!("L{line} [{kind}] {}", sig.unwrap_or(name)))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Skeleton for entire project: file → symbol list map.
    pub fn project_skeleton(&self, project_id: &str) -> Result<Vec<(String, Vec<String>)>> {
        self.db.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT file_path, kind, name, signature, start_line FROM cg_symbols
                 WHERE project_id=?1
                 ORDER BY file_path, start_line"
            )?;
            let mut map: std::collections::BTreeMap<String, Vec<String>> = Default::default();
            stmt.query_map(params![project_id], |row| {
                let file: String = row.get(0)?;
                let kind: String = row.get(1)?;
                let name: String = row.get(2)?;
                let sig: Option<String> = row.get(3)?;
                let line: i64 = row.get(4)?;
                Ok((file, kind, sig.unwrap_or(name), line))
            })?
            .filter_map(|r| r.ok())
            .for_each(|(file, kind, sig, line)| {
                map.entry(file).or_default().push(format!("L{line} [{kind}] {sig}"));
            });
            Ok(map.into_iter().collect())
        })
    }

    // ─── Stats ───────────────────────────────────────────────────────────────

    pub fn stats(&self, project_id: &str) -> Result<(i64, i64, i64)> {
        self.db.with_conn(|conn| {
            let symbols: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cg_symbols WHERE project_id=?1",
                params![project_id], |r| r.get(0))?;
            let edges: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cg_edges WHERE project_id=?1",
                params![project_id], |r| r.get(0))?;
            let files: i64 = conn.query_row(
                "SELECT COUNT(*) FROM cg_index_state WHERE project_id=?1",
                params![project_id], |r| r.get(0))?;
            Ok((files, symbols, edges))
        })
    }
}

// ─── Row mapper ───────────────────────────────────────────────────────────────

fn map_symbol(row: &rusqlite::Row) -> rusqlite::Result<CodeNode> {
    Ok(CodeNode {
        id:         row.get(0)?,
        project_id: row.get(1)?,
        file_path:  row.get(2)?,
        name:       row.get(3)?,
        kind:       NodeKind::from_str(&row.get::<_, String>(4)?),
        signature:  row.get(5)?,
        doc_comment: None,
        start_line: row.get::<_, i64>(6)? as u32,
        end_line:   row.get::<_, i64>(7)? as u32,
        language:   {
            let s: String = row.get(8)?;
            match s.as_str() {
                "rust"       => super::types::Language::Rust,
                "typescript" => super::types::Language::TypeScript,
                "javascript" => super::types::Language::JavaScript,
                "python"     => super::types::Language::Python,
                _            => super::types::Language::Unknown,
            }
        },
    })
}
