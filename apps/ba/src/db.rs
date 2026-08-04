//! SQLite layer — one serialized connection behind a `Mutex` with WAL,
//! matching the other Space Apps. Timestamps unix ms. Mọi hàm đọc trả
//! `serde_json::Value` để REST/MCP dùng chung không cần struct trung gian.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Fold riêng cho FTS: đ→d (unicode61 remove_diacritics KHÔNG fold đ vì nó là
/// chữ cái riêng, không phải dấu — bài học apps/news). Áp cho cả text đánh
/// chỉ mục lẫn query.
pub fn fold_d(s: &str) -> String {
    s.replace('đ', "d").replace('Đ', "D")
}

/// Slug kebab-case ascii từ tên tiếng Việt (bỏ dấu, đ→d).
pub fn slugify(s: &str) -> String {
    let s = s.to_lowercase();
    let mut out = String::new();
    for c in s.chars() {
        let mapped: Option<char> = match c {
            'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ'
            | 'ấ' | 'ẩ' | 'ẫ' | 'ậ' => Some('a'),
            'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => Some('e'),
            'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => Some('i'),
            'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ'
            | 'ớ' | 'ở' | 'ỡ' | 'ợ' => Some('o'),
            'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => Some('u'),
            'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => Some('y'),
            'đ' => Some('d'),
            c if c.is_ascii_alphanumeric() => Some(c.to_ascii_lowercase()),
            ' ' | '-' | '_' | '/' | '.' => Some('-'),
            _ => None,
        };
        let lower = match mapped {
            Some(m) => m,
            None => match c.to_lowercase().next() {
                Some(l) if l.is_ascii_alphanumeric() => l,
                _ => continue,
            },
        };
        out.push(lower);
    }
    let mut collapsed = String::new();
    for part in out.split('-').filter(|p| !p.is_empty()) {
        if !collapsed.is_empty() {
            collapsed.push('-');
        }
        collapsed.push_str(part);
    }
    if collapsed.is_empty() {
        "item".to_string()
    } else {
        collapsed.chars().take(48).collect()
    }
}

impl Db {
    pub fn open(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn open_default() -> Result<Self> {
        Self::open(crate::config::db_path())
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("db mutex poisoned")
    }

    pub fn log(&self, actor: &str, action: &str, detail: &str) {
        let _ = self.lock().execute(
            "INSERT INTO activity(actor, action, detail, created_at) VALUES (?1,?2,?3,?4)",
            params![actor, action, detail, now_ms()],
        );
    }

    pub fn list_activity(&self, limit: i64) -> Vec<Value> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT actor, action, detail, created_at FROM activity ORDER BY id DESC LIMIT ?1")
            .unwrap();
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok(json!({
                    "actor": r.get::<_, String>(0)?,
                    "action": r.get::<_, String>(1)?,
                    "detail": r.get::<_, String>(2)?,
                    "created_at": r.get::<_, i64>(3)?,
                }))
            })
            .unwrap()
            .filter_map(|x| x.ok())
            .collect();
        rows
    }

    // ---------- projects ----------

    pub fn create_project(&self, name: &str, description: &str, context: &str) -> Result<i64> {
        let slug = slugify(name);
        let t = now_ms();
        let conn = self.lock();
        // slug trùng thì thêm hậu tố số — tên dự án được phép trùng.
        let mut final_slug = slug.clone();
        let mut n = 1;
        loop {
            let exists: Option<i64> = conn
                .query_row(
                    "SELECT id FROM projects WHERE slug=?1",
                    params![final_slug],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_none() {
                break;
            }
            n += 1;
            final_slug = format!("{slug}-{n}");
        }
        conn.execute(
            "INSERT INTO projects(slug, name, description, context, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?5)",
            params![final_slug, name, description, context, t],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn resolve_project(&self, key: &str) -> Option<i64> {
        let conn = self.lock();
        if let Ok(id) = key.trim().parse::<i64>() {
            let found: Option<i64> = conn
                .query_row("SELECT id FROM projects WHERE id=?1", params![id], |r| r.get(0))
                .optional()
                .ok()
                .flatten();
            if found.is_some() {
                return found;
            }
        }
        conn.query_row(
            "SELECT id FROM projects WHERE slug=?1",
            params![key.trim()],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn list_projects(&self) -> Vec<Value> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT p.id, p.slug, p.name, p.description, p.updated_at,
                        (SELECT COUNT(*) FROM features f WHERE f.project_id=p.id),
                        (SELECT COUNT(*) FROM documents d WHERE d.project_id=p.id)
                 FROM projects p ORDER BY p.updated_at DESC",
            )
            .unwrap();
        stmt.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "slug": r.get::<_, String>(1)?,
                "name": r.get::<_, String>(2)?,
                "description": r.get::<_, String>(3)?,
                "updated_at": r.get::<_, i64>(4)?,
                "features": r.get::<_, i64>(5)?,
                "documents": r.get::<_, i64>(6)?,
            }))
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    }

    pub fn get_project(&self, id: i64) -> Option<Value> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, slug, name, description, context, created_at, updated_at FROM projects WHERE id=?1",
            params![id],
            |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "slug": r.get::<_, String>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "description": r.get::<_, String>(3)?,
                    "context": r.get::<_, String>(4)?,
                    "created_at": r.get::<_, i64>(5)?,
                    "updated_at": r.get::<_, i64>(6)?,
                }))
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn update_project(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        context: Option<&str>,
    ) -> Result<()> {
        let conn = self.lock();
        if let Some(v) = name {
            conn.execute("UPDATE projects SET name=?1, updated_at=?2 WHERE id=?3", params![v, now_ms(), id])?;
        }
        if let Some(v) = description {
            conn.execute("UPDATE projects SET description=?1, updated_at=?2 WHERE id=?3", params![v, now_ms(), id])?;
        }
        if let Some(v) = context {
            conn.execute("UPDATE projects SET context=?1, updated_at=?2 WHERE id=?3", params![v, now_ms(), id])?;
        }
        Ok(())
    }

    // ---------- features ----------

    pub fn add_feature(
        &self,
        project_id: i64,
        name: &str,
        description: &str,
        priority: &str,
    ) -> Result<i64> {
        let slug = slugify(name);
        let t = now_ms();
        let conn = self.lock();
        let exists: Option<i64> = conn
            .query_row(
                "SELECT id FROM features WHERE project_id=?1 AND slug=?2",
                params![project_id, slug],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = exists {
            return Err(anyhow!(
                "tính năng slug '{slug}' đã tồn tại trong dự án (id {id}) — dùng tên khác hoặc cập nhật cái cũ"
            ));
        }
        let sort: i64 = conn.query_row(
            "SELECT COALESCE(MAX(sort),0)+1 FROM features WHERE project_id=?1",
            params![project_id],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO features(project_id, slug, name, description, priority, sort, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
            params![project_id, slug, name, description, priority, sort, t],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn resolve_feature(&self, project_id: i64, key: &str) -> Option<i64> {
        let conn = self.lock();
        if let Ok(id) = key.trim().parse::<i64>() {
            let found: Option<i64> = conn
                .query_row(
                    "SELECT id FROM features WHERE id=?1 AND project_id=?2",
                    params![id, project_id],
                    |r| r.get(0),
                )
                .optional()
                .ok()
                .flatten();
            if found.is_some() {
                return found;
            }
        }
        conn.query_row(
            "SELECT id FROM features WHERE project_id=?1 AND slug=?2",
            params![project_id, key.trim()],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn list_features(&self, project_id: i64) -> Vec<Value> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT f.id, f.slug, f.name, f.description, f.priority, f.status, f.updated_at,
                        (SELECT COUNT(*) FROM documents d WHERE d.feature_id=f.id)
                 FROM features f WHERE f.project_id=?1 ORDER BY f.sort, f.id",
            )
            .unwrap();
        stmt.query_map(params![project_id], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "slug": r.get::<_, String>(1)?,
                "name": r.get::<_, String>(2)?,
                "description": r.get::<_, String>(3)?,
                "priority": r.get::<_, String>(4)?,
                "status": r.get::<_, String>(5)?,
                "updated_at": r.get::<_, i64>(6)?,
                "documents": r.get::<_, i64>(7)?,
            }))
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    }

    pub fn get_feature(&self, id: i64) -> Option<Value> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, project_id, slug, name, description, priority, status FROM features WHERE id=?1",
            params![id],
            |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "project_id": r.get::<_, i64>(1)?,
                    "slug": r.get::<_, String>(2)?,
                    "name": r.get::<_, String>(3)?,
                    "description": r.get::<_, String>(4)?,
                    "priority": r.get::<_, String>(5)?,
                    "status": r.get::<_, String>(6)?,
                }))
            },
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn update_feature(
        &self,
        id: i64,
        name: Option<&str>,
        description: Option<&str>,
        priority: Option<&str>,
        status: Option<&str>,
    ) -> Result<()> {
        let conn = self.lock();
        if let Some(v) = name {
            conn.execute("UPDATE features SET name=?1, updated_at=?2 WHERE id=?3", params![v, now_ms(), id])?;
        }
        if let Some(v) = description {
            conn.execute("UPDATE features SET description=?1, updated_at=?2 WHERE id=?3", params![v, now_ms(), id])?;
        }
        if let Some(v) = priority {
            conn.execute("UPDATE features SET priority=?1, updated_at=?2 WHERE id=?3", params![v, now_ms(), id])?;
        }
        if let Some(v) = status {
            conn.execute("UPDATE features SET status=?1, updated_at=?2 WHERE id=?3", params![v, now_ms(), id])?;
        }
        Ok(())
    }

    // ---------- documents ----------

    /// Đồng bộ hàng FTS cho một doc (text đã fold đ→d). Gọi sau mọi lần
    /// title/content đổi.
    fn fts_sync(conn: &Connection, id: i64, title: &str, content: &str) {
        let _ = conn.execute("DELETE FROM documents_fts WHERE rowid=?1", params![id]);
        let _ = conn.execute(
            "INSERT INTO documents_fts(rowid, title, content) VALUES (?1,?2,?3)",
            params![id, fold_d(title), fold_d(content)],
        );
    }

    /// Một tài liệu "sống" mỗi (project, feature, doc_type, subtype). Đã có →
    /// đẩy bản cũ vào doc_versions, bump version, trạng thái quay về draft.
    #[allow(clippy::too_many_arguments)]
    pub fn upsert_document(
        &self,
        project_id: i64,
        feature_id: Option<i64>,
        doc_type: &str,
        subtype: &str,
        title: &str,
        content: &str,
        format: &str,
        source: &str,
        confidence: &str,
        note: &str,
    ) -> Result<(i64, i64)> {
        let t = now_ms();
        let conn = self.lock();
        let existing: Option<(i64, i64, String)> = conn
            .query_row(
                "SELECT id, version, content FROM documents
                 WHERE project_id=?1 AND feature_id IS ?2 AND doc_type=?3 AND subtype=?4",
                params![project_id, feature_id, doc_type, subtype],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        match existing {
            Some((id, ver, old_content)) => {
                conn.execute(
                    "INSERT INTO doc_versions(document_id, version, content, note, created_at) VALUES (?1,?2,?3,?4,?5)",
                    params![id, ver, old_content, note, t],
                )?;
                let new_ver = ver + 1;
                conn.execute(
                    "UPDATE documents SET title=?1, content=?2, format=?3, status='draft', version=?4, source=?5, confidence=?6, updated_at=?7 WHERE id=?8",
                    params![title, content, format, new_ver, source, confidence, t, id],
                )?;
                Self::fts_sync(&conn, id, title, content);
                Ok((id, new_ver))
            }
            None => {
                conn.execute(
                    "INSERT INTO documents(project_id, feature_id, doc_type, subtype, title, content, format, status, version, source, confidence, created_at, updated_at)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,'draft',1,?8,?9,?10,?10)",
                    params![project_id, feature_id, doc_type, subtype, title, content, format, source, confidence, t],
                )?;
                let id = conn.last_insert_rowid();
                Self::fts_sync(&conn, id, title, content);
                Ok((id, 1))
            }
        }
    }

    pub fn find_document(
        &self,
        project_id: i64,
        feature_id: Option<i64>,
        doc_type: &str,
        subtype: &str,
    ) -> Option<i64> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id FROM documents WHERE project_id=?1 AND feature_id IS ?2 AND doc_type=?3 AND subtype=?4",
            params![project_id, feature_id, doc_type, subtype],
            |r| r.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    fn doc_row(r: &rusqlite::Row<'_>, with_content: bool) -> rusqlite::Result<Value> {
        let content: String = r.get(6)?;
        let mut v = json!({
            "id": r.get::<_, i64>(0)?,
            "project_id": r.get::<_, i64>(1)?,
            "feature_id": r.get::<_, Option<i64>>(2)?,
            "doc_type": r.get::<_, String>(3)?,
            "subtype": r.get::<_, String>(4)?,
            "title": r.get::<_, String>(5)?,
            "format": r.get::<_, String>(7)?,
            "status": r.get::<_, String>(8)?,
            "version": r.get::<_, i64>(9)?,
            "source": r.get::<_, String>(10)?,
            "confidence": r.get::<_, String>(11)?,
            "created_at": r.get::<_, i64>(12)?,
            "updated_at": r.get::<_, i64>(13)?,
            "chars": content.chars().count(),
        });
        if with_content {
            v["content"] = json!(content);
        }
        Ok(v)
    }

    const DOC_COLS: &'static str = "id, project_id, feature_id, doc_type, subtype, title, content, format, status, version, source, confidence, created_at, updated_at";

    pub fn get_document(&self, id: i64) -> Option<Value> {
        let conn = self.lock();
        conn.query_row(
            &format!("SELECT {} FROM documents WHERE id=?1", Self::DOC_COLS),
            params![id],
            |r| Self::doc_row(r, true),
        )
        .optional()
        .ok()
        .flatten()
    }

    /// feature: None = mọi doc của project; Some(None) = chỉ doc cấp project;
    /// Some(Some(fid)) = doc của feature đó.
    pub fn list_documents(
        &self,
        project_id: i64,
        feature: Option<Option<i64>>,
        doc_type: Option<&str>,
    ) -> Vec<Value> {
        let conn = self.lock();
        let mut sql = format!(
            "SELECT {} FROM documents WHERE project_id=?1",
            Self::DOC_COLS
        );
        match &feature {
            None => {}
            Some(None) => sql.push_str(" AND feature_id IS NULL"),
            Some(Some(fid)) => sql.push_str(&format!(" AND feature_id={fid}")),
        }
        if let Some(dt) = doc_type {
            sql.push_str(&format!(" AND doc_type='{}'", dt.replace('\'', "")));
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let mut stmt = conn.prepare(&sql).unwrap();
        stmt.query_map(params![project_id], |r| Self::doc_row(r, false))
            .unwrap()
            .filter_map(|x| x.ok())
            .collect()
    }

    /// Toàn bộ doc của một feature KÈM nội dung (context assembly + trace).
    pub fn docs_with_content(&self, project_id: i64, feature_id: Option<i64>) -> Vec<Value> {
        let conn = self.lock();
        let sql = match feature_id {
            Some(fid) => format!(
                "SELECT {} FROM documents WHERE project_id=?1 AND feature_id={fid} ORDER BY updated_at DESC",
                Self::DOC_COLS
            ),
            None => format!(
                "SELECT {} FROM documents WHERE project_id=?1 AND feature_id IS NULL ORDER BY updated_at DESC",
                Self::DOC_COLS
            ),
        };
        let mut stmt = conn.prepare(&sql).unwrap();
        stmt.query_map(params![project_id], |r| Self::doc_row(r, true))
            .unwrap()
            .filter_map(|x| x.ok())
            .collect()
    }

    pub fn update_document(
        &self,
        id: i64,
        title: Option<&str>,
        content: Option<&str>,
        status: Option<&str>,
    ) -> Result<()> {
        let t = now_ms();
        let conn = self.lock();
        if let Some(c) = content {
            // Sửa tay = version mới, source về 'user'.
            let (ver, old): (i64, String) = conn
                .query_row(
                    "SELECT version, content FROM documents WHERE id=?1",
                    params![id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|_| anyhow!("tài liệu #{id} không tồn tại"))?;
            if old != c {
                conn.execute(
                    "INSERT INTO doc_versions(document_id, version, content, note, created_at) VALUES (?1,?2,?3,'trước khi sửa tay',?4)",
                    params![id, ver, old, t],
                )?;
                conn.execute(
                    "UPDATE documents SET content=?1, version=?2, source='user', updated_at=?3 WHERE id=?4",
                    params![c, ver + 1, t, id],
                )?;
            }
        }
        if let Some(v) = title {
            conn.execute("UPDATE documents SET title=?1, updated_at=?2 WHERE id=?3", params![v, t, id])?;
        }
        if content.is_some() || title.is_some() {
            if let Ok((cur_title, cur_content)) = conn.query_row(
                "SELECT title, content FROM documents WHERE id=?1",
                params![id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            ) {
                Self::fts_sync(&conn, id, &cur_title, &cur_content);
            }
        }
        if let Some(v) = status {
            if !crate::templates::DOC_STATUSES.contains(&v) {
                return Err(anyhow!(
                    "trạng thái '{v}' không hợp lệ — dùng một trong: {}",
                    crate::templates::DOC_STATUSES.join(", ")
                ));
            }
            conn.execute("UPDATE documents SET status=?1, updated_at=?2 WHERE id=?3", params![v, t, id])?;
        }
        Ok(())
    }

    pub fn delete_document(&self, id: i64) -> Result<()> {
        let conn = self.lock();
        let n = conn.execute("DELETE FROM documents WHERE id=?1", params![id])?;
        if n == 0 {
            return Err(anyhow!("tài liệu #{id} không tồn tại"));
        }
        let _ = conn.execute("DELETE FROM documents_fts WHERE rowid=?1", params![id]);
        Ok(())
    }

    pub fn doc_versions(&self, document_id: i64) -> Vec<Value> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT version, note, created_at, LENGTH(content) FROM doc_versions WHERE document_id=?1 ORDER BY version DESC")
            .unwrap();
        stmt.query_map(params![document_id], |r| {
            Ok(json!({
                "version": r.get::<_, i64>(0)?,
                "note": r.get::<_, String>(1)?,
                "created_at": r.get::<_, i64>(2)?,
                "bytes": r.get::<_, i64>(3)?,
            }))
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    }

    pub fn version_content(&self, document_id: i64, version: i64) -> Option<String> {
        self.lock()
            .query_row(
                "SELECT content FROM doc_versions WHERE document_id=?1 AND version=?2",
                params![document_id, version],
                |r| r.get(0),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn search_docs(&self, project_id: Option<i64>, query: &str, limit: i64) -> Vec<Value> {
        // FTS5: mỗi từ thành "từ"* (prefix, AND ngầm định) — tránh cú pháp lạ nổ query.
        let folded = fold_d(query);
        let terms: Vec<String> = folded
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .map(|w| format!("\"{}\"*", w.replace('"', "")))
            .collect();
        if terms.is_empty() {
            return vec![];
        }
        let match_expr = terms.join(" ");
        let conn = self.lock();
        let sql = format!(
            "SELECT d.id, d.project_id, d.feature_id, d.doc_type, d.subtype, d.title, d.status,
                    snippet(documents_fts, 1, '«', '»', '…', 18)
             FROM documents_fts JOIN documents d ON d.id = documents_fts.rowid
             WHERE documents_fts MATCH ?1 {} ORDER BY rank LIMIT ?2",
            match project_id {
                Some(pid) => format!("AND d.project_id={pid}"),
                None => String::new(),
            }
        );
        let mut stmt = match conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(params![match_expr, limit], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "project_id": r.get::<_, i64>(1)?,
                "feature_id": r.get::<_, Option<i64>>(2)?,
                "doc_type": r.get::<_, String>(3)?,
                "subtype": r.get::<_, String>(4)?,
                "title": r.get::<_, String>(5)?,
                "status": r.get::<_, String>(6)?,
                "snippet": r.get::<_, String>(7)?,
            }))
        })
        .map(|rows| rows.filter_map(|x| x.ok()).collect())
        .unwrap_or_default()
    }

    // ---------- doc_ids (trace index) ----------

    pub fn reindex_doc_ids(&self, document_id: i64, entries: &[crate::trace::IdEntry]) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM doc_ids WHERE document_id=?1", params![document_id])?;
        for e in entries {
            tx.execute(
                "INSERT INTO doc_ids(document_id, kind, ident, role, from_ident, resolved) VALUES (?1,?2,?3,?4,?5,?6)",
                params![document_id, e.kind, e.ident, e.role, e.from_ident, e.resolved as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Mọi entry ID của một tập documents, kèm doc_type nguồn.
    pub fn doc_ids_for_docs(&self, doc_ids: &[i64]) -> Vec<(i64, String, String, String, String, String, bool)> {
        if doc_ids.is_empty() {
            return vec![];
        }
        let list = doc_ids
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let conn = self.lock();
        let sql = format!(
            "SELECT di.document_id, d.doc_type, di.kind, di.ident, di.role, di.from_ident, di.resolved
             FROM doc_ids di JOIN documents d ON d.id=di.document_id
             WHERE di.document_id IN ({list})"
        );
        let mut stmt = conn.prepare(&sql).unwrap();
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)? != 0,
            ))
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    }

    // ---------- workflows ----------

    pub fn create_workflow(
        &self,
        project_id: i64,
        feature_id: i64,
        name: &str,
        template: &str,
        steps_json: &str,
    ) -> Result<i64> {
        let t = now_ms();
        let conn = self.lock();
        conn.execute(
            "UPDATE workflows SET status='abandoned', updated_at=?1 WHERE feature_id=?2 AND status='active'",
            params![t, feature_id],
        )?;
        conn.execute(
            "INSERT INTO workflows(project_id, feature_id, name, template, steps, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?6)",
            params![project_id, feature_id, name, template, steps_json, t],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn workflow_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
        let steps: String = r.get(5)?;
        Ok(json!({
            "id": r.get::<_, i64>(0)?,
            "project_id": r.get::<_, i64>(1)?,
            "feature_id": r.get::<_, i64>(2)?,
            "name": r.get::<_, String>(3)?,
            "template": r.get::<_, String>(4)?,
            "steps": serde_json::from_str::<Value>(&steps).unwrap_or_else(|_| json!([])),
            "status": r.get::<_, String>(6)?,
            "updated_at": r.get::<_, i64>(7)?,
        }))
    }

    pub fn active_workflow(&self, feature_id: i64) -> Option<Value> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, project_id, feature_id, name, template, steps, status, updated_at FROM workflows WHERE feature_id=?1 AND status='active' ORDER BY id DESC LIMIT 1",
            params![feature_id],
            |r| Self::workflow_row(r),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn get_workflow(&self, id: i64) -> Option<Value> {
        let conn = self.lock();
        conn.query_row(
            "SELECT id, project_id, feature_id, name, template, steps, status, updated_at FROM workflows WHERE id=?1",
            params![id],
            |r| Self::workflow_row(r),
        )
        .optional()
        .ok()
        .flatten()
    }

    pub fn update_workflow(&self, id: i64, steps_json: &str, status: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE workflows SET steps=?1, status=?2, updated_at=?3 WHERE id=?4",
            params![steps_json, status, now_ms(), id],
        )?;
        Ok(())
    }

    // ---------- change requests ----------

    pub fn next_cr_code(&self, date_yyyymmdd: &str) -> String {
        let prefix = format!("CR-{date_yyyymmdd}-");
        let conn = self.lock();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM change_requests WHERE code LIKE ?1 || '%'",
                params![prefix],
                |r| r.get(0),
            )
            .unwrap_or(0);
        format!("{prefix}{:03}", n + 1)
    }

    pub fn create_cr(
        &self,
        project_id: i64,
        feature_id: Option<i64>,
        code: &str,
        title: &str,
        description: &str,
        severity: &str,
    ) -> Result<i64> {
        let t = now_ms();
        let conn = self.lock();
        conn.execute(
            "INSERT INTO change_requests(project_id, feature_id, code, title, description, severity, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
            params![project_id, feature_id, code, title, description, severity, t],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn set_cr_analysis(&self, id: i64, analysis: &str, status: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE change_requests SET analysis=?1, status=?2, updated_at=?3 WHERE id=?4",
            params![analysis, status, now_ms(), id],
        )?;
        Ok(())
    }

    pub fn set_cr_status(&self, id: i64, status: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE change_requests SET status=?1, updated_at=?2 WHERE id=?3",
            params![status, now_ms(), id],
        )?;
        Ok(())
    }

    pub fn add_cr_impact(&self, cr_id: i64, document_id: i64, summary: &str) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO cr_impacts(cr_id, document_id, summary, created_at) VALUES (?1,?2,?3,?4)",
            params![cr_id, document_id, summary, now_ms()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn list_crs(&self, project_id: i64) -> Vec<Value> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT c.id, c.code, c.title, c.severity, c.status, c.feature_id, c.created_at, c.updated_at,
                        (SELECT COUNT(*) FROM cr_impacts i WHERE i.cr_id=c.id),
                        (SELECT COUNT(*) FROM cr_impacts i WHERE i.cr_id=c.id AND i.status='pending')
                 FROM change_requests c WHERE c.project_id=?1 ORDER BY c.id DESC",
            )
            .unwrap();
        stmt.query_map(params![project_id], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "code": r.get::<_, String>(1)?,
                "title": r.get::<_, String>(2)?,
                "severity": r.get::<_, String>(3)?,
                "status": r.get::<_, String>(4)?,
                "feature_id": r.get::<_, Option<i64>>(5)?,
                "created_at": r.get::<_, i64>(6)?,
                "updated_at": r.get::<_, i64>(7)?,
                "impacts": r.get::<_, i64>(8)?,
                "impacts_pending": r.get::<_, i64>(9)?,
            }))
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    }

    pub fn get_cr(&self, id: i64) -> Option<Value> {
        let conn = self.lock();
        let mut cr = conn
            .query_row(
                "SELECT id, project_id, feature_id, code, title, description, severity, status, analysis, created_at, updated_at FROM change_requests WHERE id=?1",
                params![id],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "project_id": r.get::<_, i64>(1)?,
                        "feature_id": r.get::<_, Option<i64>>(2)?,
                        "code": r.get::<_, String>(3)?,
                        "title": r.get::<_, String>(4)?,
                        "description": r.get::<_, String>(5)?,
                        "severity": r.get::<_, String>(6)?,
                        "status": r.get::<_, String>(7)?,
                        "analysis": r.get::<_, String>(8)?,
                        "created_at": r.get::<_, i64>(9)?,
                        "updated_at": r.get::<_, i64>(10)?,
                    }))
                },
            )
            .optional()
            .ok()
            .flatten()?;
        let mut stmt = conn
            .prepare(
                "SELECT i.id, i.document_id, i.summary, i.status, i.applied_at, d.title, d.doc_type, d.subtype
                 FROM cr_impacts i JOIN documents d ON d.id=i.document_id WHERE i.cr_id=?1 ORDER BY i.id",
            )
            .unwrap();
        let impacts: Vec<Value> = stmt
            .query_map(params![id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "document_id": r.get::<_, i64>(1)?,
                    "summary": r.get::<_, String>(2)?,
                    "status": r.get::<_, String>(3)?,
                    "applied_at": r.get::<_, Option<i64>>(4)?,
                    "doc_title": r.get::<_, String>(5)?,
                    "doc_type": r.get::<_, String>(6)?,
                    "subtype": r.get::<_, String>(7)?,
                }))
            })
            .unwrap()
            .filter_map(|x| x.ok())
            .collect();
        cr["impacts"] = json!(impacts);
        Some(cr)
    }

    pub fn get_impact(&self, impact_id: i64) -> Option<(i64, i64, String, String)> {
        self.lock()
            .query_row(
                "SELECT cr_id, document_id, summary, status FROM cr_impacts WHERE id=?1",
                params![impact_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .ok()
            .flatten()
    }

    pub fn set_impact_status(&self, impact_id: i64, status: &str) -> Result<()> {
        let applied_at = if status == "applied" { Some(now_ms()) } else { None };
        self.lock().execute(
            "UPDATE cr_impacts SET status=?1, applied_at=?2 WHERE id=?3",
            params![status, applied_at, impact_id],
        )?;
        Ok(())
    }

    pub fn cr_pending_impacts(&self, cr_id: i64) -> i64 {
        self.lock()
            .query_row(
                "SELECT COUNT(*) FROM cr_impacts WHERE cr_id=?1 AND status='pending'",
                params![cr_id],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }

    /// CR đang treo toàn project (cho dashboard): (code, status, severity, ngày tạo, pending impacts).
    pub fn open_crs(&self, project_id: i64) -> Vec<(String, String, String, i64, i64)> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT c.code, c.status, c.severity, c.created_at,
                        (SELECT COUNT(*) FROM cr_impacts i WHERE i.cr_id=c.id AND i.status='pending')
                 FROM change_requests c WHERE c.project_id=?1 AND c.status != 'closed' ORDER BY c.id DESC",
            )
            .unwrap();
        stmt.query_map(params![project_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    }

    // ---------- qa ----------

    pub fn add_qa(
        &self,
        project_id: i64,
        feature_id: Option<i64>,
        question: &str,
        answer: &str,
        citations: &str,
    ) -> Result<()> {
        self.lock().execute(
            "INSERT INTO qa_log(project_id, feature_id, question, answer, citations, created_at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![project_id, feature_id, question, answer, citations, now_ms()],
        )?;
        Ok(())
    }

    pub fn list_qa(&self, project_id: i64, limit: i64) -> Vec<Value> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare("SELECT question, answer, citations, created_at FROM qa_log WHERE project_id=?1 ORDER BY id DESC LIMIT ?2")
            .unwrap();
        stmt.query_map(params![project_id, limit], |r| {
            let cit: String = r.get(2)?;
            Ok(json!({
                "question": r.get::<_, String>(0)?,
                "answer": r.get::<_, String>(1)?,
                "citations": serde_json::from_str::<Value>(&cit).unwrap_or_else(|_| json!([])),
                "created_at": r.get::<_, i64>(3)?,
            }))
        })
        .unwrap()
        .filter_map(|x| x.ok())
        .collect()
    }

    pub fn counts(&self) -> Value {
        let conn = self.lock();
        let q = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0) };
        json!({
            "projects": q("SELECT COUNT(*) FROM projects"),
            "features": q("SELECT COUNT(*) FROM features"),
            "documents": q("SELECT COUNT(*) FROM documents"),
            "change_requests": q("SELECT COUNT(*) FROM change_requests"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_vietnamese() {
        assert_eq!(slugify("Xác thực người dùng"), "xac-thuc-nguoi-dung");
        assert_eq!(slugify("Thanh toán / Đơn hàng"), "thanh-toan-don-hang");
        assert_eq!(slugify("  --Weird__name!!  "), "weird-name");
        assert_eq!(slugify("!!!"), "item");
    }

    #[test]
    fn project_feature_crud_and_slug_dedup() {
        let db = Db::open_memory().unwrap();
        let p1 = db.create_project("Demo App", "mô tả", "bối cảnh").unwrap();
        let p2 = db.create_project("Demo App", "", "").unwrap();
        let s1 = db.get_project(p1).unwrap()["slug"].as_str().unwrap().to_string();
        let s2 = db.get_project(p2).unwrap()["slug"].as_str().unwrap().to_string();
        assert_eq!(s1, "demo-app");
        assert_eq!(s2, "demo-app-2");
        assert_eq!(db.resolve_project("demo-app"), Some(p1));
        assert_eq!(db.resolve_project(&p2.to_string()), Some(p2));

        let f = db.add_feature(p1, "Xác thực", "đăng nhập đăng ký", "P0").unwrap();
        assert!(db.add_feature(p1, "Xác thực", "trùng", "P1").is_err());
        assert_eq!(db.resolve_feature(p1, "xac-thuc"), Some(f));
        assert_eq!(db.list_features(p1).len(), 1);
    }

    #[test]
    fn document_upsert_versions_and_manual_edit() {
        let db = Db::open_memory().unwrap();
        let p = db.create_project("P", "", "").unwrap();
        let f = db.add_feature(p, "auth", "", "P0").unwrap();
        let (id, v1) = db
            .upsert_document(p, Some(f), "srs", "", "SRS auth", "# SRS v1", "markdown", "ai", "", "")
            .unwrap();
        assert_eq!(v1, 1);
        let (id2, v2) = db
            .upsert_document(p, Some(f), "srs", "", "SRS auth", "# SRS v2", "markdown", "ai", "", "regen")
            .unwrap();
        assert_eq!(id, id2);
        assert_eq!(v2, 2);
        assert_eq!(db.doc_versions(id).len(), 1);
        // sửa tay → version 3, source user
        db.update_document(id, None, Some("# SRS v3 (tay)"), None).unwrap();
        let d = db.get_document(id).unwrap();
        assert_eq!(d["version"], 3);
        assert_eq!(d["source"], "user");
        // status validate
        assert!(db.update_document(id, None, None, Some("bogus")).is_err());
        db.update_document(id, None, None, Some("approved")).unwrap();
        // doc cấp project tách namespace với doc feature
        let (pid_doc, _) = db
            .upsert_document(p, None, "prd", "", "PRD", "# PRD", "markdown", "ai", "", "")
            .unwrap();
        assert_ne!(pid_doc, id);
        assert_eq!(db.list_documents(p, Some(None), None).len(), 1);
        assert_eq!(db.list_documents(p, Some(Some(f)), None).len(), 1);
        assert_eq!(db.list_documents(p, None, None).len(), 2);
    }

    #[test]
    fn fts_search_finds_vietnamese() {
        let db = Db::open_memory().unwrap();
        let p = db.create_project("P", "", "").unwrap();
        db.upsert_document(p, None, "prd", "", "PRD sản phẩm", "Tính năng đăng nhập bằng Google", "markdown", "ai", "", "")
            .unwrap();
        let hits = db.search_docs(Some(p), "đăng nhập", 10);
        assert_eq!(hits.len(), 1);
        let hits2 = db.search_docs(Some(p), "dang nhap", 10);
        assert_eq!(hits2.len(), 1, "remove_diacritics phải cho tìm không dấu");
    }

    #[test]
    fn cr_flow() {
        let db = Db::open_memory().unwrap();
        let p = db.create_project("P", "", "").unwrap();
        let f = db.add_feature(p, "auth", "", "P0").unwrap();
        let (doc, _) = db
            .upsert_document(p, Some(f), "srs", "", "SRS", "# SRS", "markdown", "ai", "", "")
            .unwrap();
        let code = db.next_cr_code("20260802");
        assert_eq!(code, "CR-20260802-001");
        let cr = db.create_cr(p, Some(f), &code, "Đổi chính sách", "mô tả", "high").unwrap();
        assert_eq!(db.next_cr_code("20260802"), "CR-20260802-002");
        let imp = db.add_cr_impact(cr, doc, "sửa FR").unwrap();
        assert_eq!(db.cr_pending_impacts(cr), 1);
        db.set_impact_status(imp, "applied").unwrap();
        assert_eq!(db.cr_pending_impacts(cr), 0);
        let detail = db.get_cr(cr).unwrap();
        assert_eq!(detail["impacts"].as_array().unwrap().len(), 1);
    }
}
