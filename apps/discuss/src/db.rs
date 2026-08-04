//! SQLite data layer — Arc<Mutex<Connection>>, schema idempotent, FTS5 tự fold.
//!
//! Quy ước khoá: mọi helper tự lock trong thân hàm và trả dữ liệu đã own —
//! KHÔNG giữ MutexGuard qua await ở tầng gọi.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

const SCHEMA: &str = include_str!("schema.sql");

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Fold tiếng Việt cho FTS: lowercase + bỏ dấu + đ→d. unicode61 của SQLite bỏ
/// được dấu nhưng KHÔNG fold đ (nó là chữ cái riêng) — phải fold cả lúc index
/// lẫn lúc query, nếu không "dẫn chứng" không khớp "dan chung".
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let lower: Vec<char> = c.to_lowercase().collect();
        for lc in lower {
            match lc {
                'à' | 'á' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ằ' | 'ắ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ầ'
                | 'ấ' | 'ẩ' | 'ẫ' | 'ậ' => out.push('a'),
                'è' | 'é' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ề' | 'ế' | 'ể' | 'ễ' | 'ệ' => out.push('e'),
                'ì' | 'í' | 'ỉ' | 'ĩ' | 'ị' => out.push('i'),
                'ò' | 'ó' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ồ' | 'ố' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ờ'
                | 'ớ' | 'ở' | 'ỡ' | 'ợ' => out.push('o'),
                'ù' | 'ú' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ừ' | 'ứ' | 'ử' | 'ữ' | 'ự' => out.push('u'),
                'ỳ' | 'ý' | 'ỷ' | 'ỹ' | 'ỵ' => out.push('y'),
                'đ' => out.push('d'),
                other => out.push(other),
            }
        }
    }
    out
}

/// Chuỗi FTS MATCH an toàn: mỗi từ thành `"từ"*` (prefix, AND ngầm).
pub fn fts_query(q: &str) -> String {
    fold(q)
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn slugify(s: &str) -> String {
    let folded = fold(s);
    let mut out = String::new();
    for c in folded.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_matches('-').chars().take(40).collect()
}

// ---------------- Row structs ----------------

#[derive(Debug, Clone, Serialize)]
pub struct Discussion {
    pub id: i64,
    pub title: String,
    pub requirement: String,
    pub status: String,
    pub mode: String,
    pub pace_secs: i64,
    pub max_rounds: i64,
    pub round: i64,
    pub manager_score: i64,
    pub manager_missing: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
    pub concluded_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Member {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub role: String,
    pub expertise: String,
    pub style: String,
    pub hat: String,
    pub use_tools: bool,
    pub tools: Option<serde_json::Value>,
    pub model: Option<String>,
    pub enabled: bool,
    pub sort: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Message {
    pub id: i64,
    pub discussion_id: i64,
    pub round: i64,
    pub author_kind: String,
    pub member_id: Option<i64>,
    pub kind: String,
    pub content: String,
    pub claim_type: Option<String>,
    pub provability: Option<String>,
    pub hat: Option<String>,
    pub stance: Option<String>,
    pub reply_to: Option<i64>,
    pub citations: serde_json::Value,
    pub flags: serde_json::Value,
    pub created_at: i64,
}

/// Tin nhắn mới cần chèn (mọi field nghiệp vụ; id/created_at do DB cấp).
#[derive(Debug, Clone, Default)]
pub struct NewMessage {
    pub discussion_id: i64,
    pub round: i64,
    pub author_kind: String,
    pub member_id: Option<i64>,
    pub kind: String,
    pub content: String,
    pub claim_type: Option<String>,
    pub provability: Option<String>,
    pub hat: Option<String>,
    pub stance: Option<String>,
    pub reply_to: Option<i64>,
    pub citations: serde_json::Value,
    pub flags: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocRow {
    pub id: i64,
    pub discussion_id: Option<i64>,
    pub title: String,
    pub filename: String,
    pub content: String,
    pub source: String,
    pub created_by: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MinutesRow {
    pub id: i64,
    pub discussion_id: i64,
    pub round: i64,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResultRow {
    pub id: i64,
    pub discussion_id: i64,
    pub content: String,
    pub status: String,
    pub feedback: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryRow {
    pub id: i64,
    pub member_id: i64,
    pub discussion_id: Option<i64>,
    pub kind: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Participation {
    pub member_id: i64,
    pub key: String,
    pub name: String,
    pub message_count: i64,
    pub last_round: i64,
    pub silent_rounds: i64,
}

fn parse_json(s: String, fallback: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(&s).unwrap_or(fallback)
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        conn.execute_batch(SCHEMA)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.seed_defaults()?;
        Ok(db)
    }

    pub fn open_default() -> Result<Self> {
        Self::open(&crate::config::db_path())
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.seed_defaults()?;
        Ok(db)
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("db mutex poisoned")
    }

    // ---------------- Seed ----------------

    /// Roster mặc định: 1 Manager + 1 Thư ký + 5 member (Én tắt sẵn). INSERT OR
    /// IGNORE theo key nên chạy lại vô hại, user sửa/xoá tuỳ ý.
    pub fn seed_defaults(&self) -> Result<()> {
        let t = now();
        let seed: Vec<(&str, &str, &str, &str, &str, &str, bool, i64, bool)> = vec![
            // (key, name, role, expertise, style, hat, use_tools, sort, enabled)
            ("quan-ly", "Quản Lý", "manager", "Điều phối cuộc họp, theo dõi tiến độ so với yêu cầu của BOSS",
             "Nghiêm túc, công tâm, KHÔNG bàn nội dung — chỉ điều phối", "blue", false, 0, true),
            ("thu-ky", "Thư Ký", "secretary", "Ghi biên bản, chắt lọc ý chính, tổng hợp kết quả",
             "Chính xác, trung lập, gọn gàng", "white", false, 1, true),
            ("an-dan-chung", "An • Dẫn chứng", "member", "Tìm kiếm và kiểm chứng thông tin đa nguồn",
             "Chỉ tin điều kiểm chứng được; luôn kèm nguồn; ưu tiên mũ trắng", "white", true, 2, true),
            ("binh-phan-bien", "Bình • Phản biện", "member", "Soi rủi ro, lỗ hổng logic, phản ví dụ",
             "Hoài nghi có phương pháp; phản đối phải kèm dẫn chứng; ưu tiên mũ đen", "black,red", true, 3, true),
            ("chi-suy-luan", "Chi • Suy luận", "member", "Suy diễn hệ quả từ thông tin đã có, tìm lợi ích khả thi",
             "Lập luận từng bước từ dữ kiện trong phòng; ưu tiên mũ vàng", "yellow", false, 4, true),
            ("dung-sang-tao", "Dũng • Sáng tạo", "member", "Đề xuất hướng mới, giả thuyết táo bạo",
             "Nghĩ ngoài khung, chấp nhận ý tưởng chưa chứng minh nhưng dán nhãn rõ; ưu tiên mũ xanh lá", "green,yellow", false, 5, true),
            ("en-thoi-su", "Én • Thời sự", "member", "Tin tức, xu hướng, bối cảnh thời điểm",
             "Bám dòng sự kiện; dùng News trước tiên; ưu tiên mũ đỏ (trực giác nêu ngắn)", "red", true, 6, false),
        ];
        let conn = self.lock();
        for (key, name, role, expertise, style, hat, use_tools, sort, enabled) in seed {
            conn.execute(
                "INSERT OR IGNORE INTO members (key, name, role, expertise, style, hat, use_tools, tools, model, enabled, sort, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8, ?9, ?10)",
                params![key, name, role, expertise, style, hat, use_tools as i64, enabled as i64, sort, t],
            )?;
        }
        Ok(())
    }

    // ---------------- Discussions ----------------

    pub fn discussion_create(
        &self,
        title: &str,
        requirement: &str,
        mode: &str,
        pace_secs: i64,
        max_rounds: i64,
        member_ids: &[i64],
    ) -> Result<i64> {
        let t = now();
        let conn = self.lock();
        conn.execute(
            "INSERT INTO discussions (title, requirement, status, mode, pace_secs, max_rounds, round, created_at, updated_at)
             VALUES (?1, ?2, 'draft', ?3, ?4, ?5, 0, ?6, ?6)",
            params![title, requirement, mode, pace_secs, max_rounds, t],
        )?;
        let id = conn.last_insert_rowid();
        for m in member_ids {
            conn.execute(
                "INSERT OR IGNORE INTO discussion_members (discussion_id, member_id) VALUES (?1, ?2)",
                params![id, m],
            )?;
        }
        Ok(id)
    }

    fn row_to_discussion(row: &rusqlite::Row<'_>) -> rusqlite::Result<Discussion> {
        Ok(Discussion {
            id: row.get(0)?,
            title: row.get(1)?,
            requirement: row.get(2)?,
            status: row.get(3)?,
            mode: row.get(4)?,
            pace_secs: row.get(5)?,
            max_rounds: row.get(6)?,
            round: row.get(7)?,
            manager_score: row.get(8)?,
            manager_missing: parse_json(row.get(9)?, serde_json::json!([])),
            created_at: row.get(10)?,
            updated_at: row.get(11)?,
            concluded_at: row.get(12)?,
        })
    }

    const DISC_COLS: &'static str = "id, title, requirement, status, mode, pace_secs, max_rounds, round, manager_score, manager_missing, created_at, updated_at, concluded_at";

    pub fn discussion_get(&self, id: i64) -> Result<Option<Discussion>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM discussions WHERE id = ?1",
            Self::DISC_COLS
        ))?;
        let mut rows = stmt.query_map(params![id], Self::row_to_discussion)?;
        Ok(rows.next().transpose()?)
    }

    pub fn discussion_list(&self, limit: i64) -> Result<Vec<Discussion>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM discussions ORDER BY id DESC LIMIT ?1",
            Self::DISC_COLS
        ))?;
        let rows = stmt.query_map(params![limit], Self::row_to_discussion)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn discussions_with_status(&self, status: &str) -> Result<Vec<Discussion>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM discussions WHERE status = ?1 ORDER BY id",
            Self::DISC_COLS
        ))?;
        let rows = stmt.query_map(params![status], Self::row_to_discussion)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn discussion_set_status(&self, id: i64, status: &str) -> Result<()> {
        let concluded = if status == "done" { Some(now()) } else { None };
        self.lock().execute(
            "UPDATE discussions SET status = ?2, updated_at = ?3, concluded_at = COALESCE(?4, concluded_at) WHERE id = ?1",
            params![id, status, now(), concluded],
        )?;
        Ok(())
    }

    pub fn discussion_set_round(&self, id: i64, round: i64) -> Result<()> {
        self.lock().execute(
            "UPDATE discussions SET round = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, round, now()],
        )?;
        Ok(())
    }

    pub fn discussion_set_pace(&self, id: i64, pace_secs: Option<i64>, mode: Option<&str>, max_rounds: Option<i64>) -> Result<()> {
        let conn = self.lock();
        if let Some(p) = pace_secs {
            conn.execute("UPDATE discussions SET pace_secs = ?2, updated_at = ?3 WHERE id = ?1", params![id, p.clamp(0, 600), now()])?;
        }
        if let Some(m) = mode {
            conn.execute("UPDATE discussions SET mode = ?2, updated_at = ?3 WHERE id = ?1", params![id, m, now()])?;
        }
        if let Some(r) = max_rounds {
            conn.execute("UPDATE discussions SET max_rounds = ?2, updated_at = ?3 WHERE id = ?1", params![id, r.clamp(1, 100), now()])?;
        }
        Ok(())
    }

    pub fn discussion_set_manager_eval(&self, id: i64, score: i64, missing: &serde_json::Value) -> Result<()> {
        self.lock().execute(
            "UPDATE discussions SET manager_score = ?2, manager_missing = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, score, missing.to_string(), now()],
        )?;
        Ok(())
    }

    // ---------------- Members ----------------

    fn row_to_member(row: &rusqlite::Row<'_>) -> rusqlite::Result<Member> {
        let tools_s: Option<String> = row.get(8)?;
        Ok(Member {
            id: row.get(0)?,
            key: row.get(1)?,
            name: row.get(2)?,
            role: row.get(3)?,
            expertise: row.get(4)?,
            style: row.get(5)?,
            hat: row.get(6)?,
            use_tools: row.get::<_, i64>(7)? != 0,
            tools: tools_s.and_then(|s| serde_json::from_str(&s).ok()),
            model: row.get(9)?,
            enabled: row.get::<_, i64>(10)? != 0,
            sort: row.get(11)?,
            created_at: row.get(12)?,
        })
    }

    const MEMBER_COLS: &'static str =
        "id, key, name, role, expertise, style, hat, use_tools, tools, model, enabled, sort, created_at";

    pub fn member_list(&self) -> Result<Vec<Member>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM members ORDER BY sort, id",
            Self::MEMBER_COLS
        ))?;
        let rows = stmt.query_map([], Self::row_to_member)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn member_get(&self, id: i64) -> Result<Option<Member>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM members WHERE id = ?1",
            Self::MEMBER_COLS
        ))?;
        let mut rows = stmt.query_map(params![id], Self::row_to_member)?;
        Ok(rows.next().transpose()?)
    }

    pub fn member_get_by_key(&self, key: &str) -> Result<Option<Member>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM members WHERE key = ?1",
            Self::MEMBER_COLS
        ))?;
        let mut rows = stmt.query_map(params![key], Self::row_to_member)?;
        Ok(rows.next().transpose()?)
    }

    /// Member có vai trò đặc biệt (manager / secretary) — lấy người enabled đầu tiên.
    pub fn member_with_role(&self, role: &str) -> Result<Option<Member>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM members WHERE role = ?1 AND enabled = 1 ORDER BY sort, id LIMIT 1",
            Self::MEMBER_COLS
        ))?;
        let mut rows = stmt.query_map(params![role], Self::row_to_member)?;
        Ok(rows.next().transpose()?)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn member_add(
        &self,
        name: &str,
        role: &str,
        expertise: &str,
        style: &str,
        hat: &str,
        use_tools: bool,
        tools: Option<&serde_json::Value>,
        model: Option<&str>,
    ) -> Result<Member> {
        let base = slugify(name);
        let base = if base.is_empty() { "member".to_string() } else { base };
        let conn = self.lock();
        // key duy nhất: slug, slug-2, slug-3...
        let mut key = base.clone();
        let mut i = 1;
        loop {
            let exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM members WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )?;
            if exists == 0 {
                break;
            }
            i += 1;
            key = format!("{base}-{i}");
        }
        let sort: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort), 0) + 1 FROM members", [], |r| r.get(0))
            .unwrap_or(99);
        conn.execute(
            "INSERT INTO members (key, name, role, expertise, style, hat, use_tools, tools, model, enabled, sort, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11)",
            params![
                key,
                name,
                role,
                expertise,
                style,
                hat,
                use_tools as i64,
                tools.map(|t| t.to_string()),
                model,
                sort,
                now()
            ],
        )?;
        let id = conn.last_insert_rowid();
        drop(conn);
        Ok(self.member_get(id)?.expect("member vừa tạo phải tồn tại"))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn member_update(
        &self,
        id: i64,
        name: Option<&str>,
        expertise: Option<&str>,
        style: Option<&str>,
        hat: Option<&str>,
        use_tools: Option<bool>,
        tools: Option<Option<&serde_json::Value>>,
        model: Option<Option<&str>>,
        enabled: Option<bool>,
    ) -> Result<()> {
        let conn = self.lock();
        if let Some(v) = name {
            conn.execute("UPDATE members SET name = ?2 WHERE id = ?1", params![id, v])?;
        }
        if let Some(v) = expertise {
            conn.execute("UPDATE members SET expertise = ?2 WHERE id = ?1", params![id, v])?;
        }
        if let Some(v) = style {
            conn.execute("UPDATE members SET style = ?2 WHERE id = ?1", params![id, v])?;
        }
        if let Some(v) = hat {
            conn.execute("UPDATE members SET hat = ?2 WHERE id = ?1", params![id, v])?;
        }
        if let Some(v) = use_tools {
            conn.execute("UPDATE members SET use_tools = ?2 WHERE id = ?1", params![id, v as i64])?;
        }
        if let Some(v) = tools {
            conn.execute(
                "UPDATE members SET tools = ?2 WHERE id = ?1",
                params![id, v.map(|t| t.to_string())],
            )?;
        }
        if let Some(v) = model {
            conn.execute("UPDATE members SET model = ?2 WHERE id = ?1", params![id, v])?;
        }
        if let Some(v) = enabled {
            conn.execute("UPDATE members SET enabled = ?2 WHERE id = ?1", params![id, v as i64])?;
        }
        Ok(())
    }

    pub fn member_delete(&self, id: i64) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM members WHERE id = ?1", params![id])?;
        conn.execute("DELETE FROM discussion_members WHERE member_id = ?1", params![id])?;
        Ok(())
    }

    pub fn discussion_members(&self, discussion_id: i64) -> Result<Vec<Member>> {
        let conn = self.lock();
        // Cột trần an toàn: discussion_members không có cột trùng tên với members.
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM members WHERE id IN
               (SELECT member_id FROM discussion_members WHERE discussion_id = ?1)
             AND enabled = 1 AND role = 'member' ORDER BY sort, id",
            Self::MEMBER_COLS
        ))?;
        let rows = stmt.query_map(params![discussion_id], Self::row_to_member)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---------------- Messages ----------------

    pub fn message_insert(&self, m: &NewMessage) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO messages (discussion_id, round, author_kind, member_id, kind, content, claim_type, provability, hat, stance, reply_to, citations, flags, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                m.discussion_id,
                m.round,
                m.author_kind,
                m.member_id,
                m.kind,
                m.content,
                m.claim_type,
                m.provability,
                m.hat,
                m.stance,
                m.reply_to,
                m.citations.to_string(),
                m.flags.to_string(),
                now()
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    fn row_to_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
        Ok(Message {
            id: row.get(0)?,
            discussion_id: row.get(1)?,
            round: row.get(2)?,
            author_kind: row.get(3)?,
            member_id: row.get(4)?,
            kind: row.get(5)?,
            content: row.get(6)?,
            claim_type: row.get(7)?,
            provability: row.get(8)?,
            hat: row.get(9)?,
            stance: row.get(10)?,
            reply_to: row.get(11)?,
            citations: parse_json(row.get(12)?, serde_json::json!([])),
            flags: parse_json(row.get(13)?, serde_json::json!({})),
            created_at: row.get(14)?,
        })
    }

    const MSG_COLS: &'static str = "id, discussion_id, round, author_kind, member_id, kind, content, claim_type, provability, hat, stance, reply_to, citations, flags, created_at";

    /// Feed tăng dần cho UI/MCP: mọi tin có id > after.
    pub fn messages_after(&self, discussion_id: i64, after: i64, limit: i64) -> Result<Vec<Message>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM messages WHERE discussion_id = ?1 AND id > ?2 ORDER BY id LIMIT ?3",
            Self::MSG_COLS
        ))?;
        let rows = stmt.query_map(params![discussion_id, after, limit], Self::row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Cửa sổ ngữ cảnh cho lượt member: N tin gần nhất, trả theo thứ tự thời gian.
    pub fn messages_recent(&self, discussion_id: i64, limit: i64) -> Result<Vec<Message>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM (SELECT {} FROM messages WHERE discussion_id = ?1 ORDER BY id DESC LIMIT ?2) ORDER BY id",
            Self::MSG_COLS
        ))?;
        let rows = stmt.query_map(params![discussion_id, limit], Self::row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn message_get(&self, id: i64) -> Result<Option<Message>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM messages WHERE id = ?1",
            Self::MSG_COLS
        ))?;
        let mut rows = stmt.query_map(params![id], Self::row_to_message)?;
        Ok(rows.next().transpose()?)
    }

    /// Luận điểm "mở": opinion của member chưa được BẤT KỲ member khác nào phản hồi.
    pub fn open_opinions(&self, discussion_id: i64, limit: i64) -> Result<Vec<Message>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM messages AS o
             WHERE o.discussion_id = ?1 AND o.kind = 'opinion'
               AND NOT EXISTS (
                 SELECT 1 FROM messages r
                 WHERE r.discussion_id = o.discussion_id AND r.kind = 'reaction'
                   AND r.reply_to = o.id AND COALESCE(r.member_id, -1) != COALESCE(o.member_id, -2)
               )
             ORDER BY o.id DESC LIMIT ?2",
            Self::MSG_COLS
        ))?;
        let rows = stmt.query_map(params![discussion_id, limit], Self::row_to_message)?;
        let mut v = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        v.reverse();
        Ok(v)
    }

    /// Tin BOSS đến sau tin gần nhất của member này (member phải xử lý trước tiên).
    pub fn boss_messages_since_member_last(&self, discussion_id: i64, member_id: i64) -> Result<Vec<Message>> {
        let conn = self.lock();
        let last: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(id), 0) FROM messages WHERE discussion_id = ?1 AND member_id = ?2",
                params![discussion_id, member_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM messages WHERE discussion_id = ?1 AND kind = 'boss' AND id > ?2 ORDER BY id",
            Self::MSG_COLS
        ))?;
        let rows = stmt.query_map(params![discussion_id, last], Self::row_to_message)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Thống kê tham gia cho Manager: số tin, vòng cuối có phát biểu, số vòng im lặng.
    pub fn participation(&self, discussion_id: i64, current_round: i64) -> Result<Vec<Participation>> {
        let members = self.discussion_members(discussion_id)?;
        let conn = self.lock();
        let mut out = Vec::new();
        for m in members {
            let (count, last_round): (i64, i64) = conn
                .query_row(
                    "SELECT COUNT(*), COALESCE(MAX(round), 0) FROM messages
                     WHERE discussion_id = ?1 AND member_id = ?2 AND kind IN ('opinion','reaction')",
                    params![discussion_id, m.id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap_or((0, 0));
            out.push(Participation {
                member_id: m.id,
                key: m.key,
                name: m.name,
                message_count: count,
                last_round,
                silent_rounds: (current_round - last_round).max(0),
            });
        }
        Ok(out)
    }

    // ---------------- Documents ----------------

    pub fn doc_add(
        &self,
        discussion_id: Option<i64>,
        title: &str,
        filename: &str,
        content: &str,
        source: &str,
        created_by: &str,
    ) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO documents (discussion_id, title, filename, content, source, created_by, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![discussion_id, title, filename, content, source, created_by, now()],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO documents_fts (rowid, title, content) VALUES (?1, ?2, ?3)",
            params![id, fold(title), fold(content)],
        )?;
        Ok(id)
    }

    fn row_to_doc(row: &rusqlite::Row<'_>) -> rusqlite::Result<DocRow> {
        Ok(DocRow {
            id: row.get(0)?,
            discussion_id: row.get(1)?,
            title: row.get(2)?,
            filename: row.get(3)?,
            content: row.get(4)?,
            source: row.get(5)?,
            created_by: row.get(6)?,
            created_at: row.get(7)?,
        })
    }

    const DOC_COLS: &'static str = "id, discussion_id, title, filename, content, source, created_by, created_at";

    pub fn doc_get(&self, id: i64) -> Result<Option<DocRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!("SELECT {} FROM documents WHERE id = ?1", Self::DOC_COLS))?;
        let mut rows = stmt.query_map(params![id], Self::row_to_doc)?;
        Ok(rows.next().transpose()?)
    }

    /// Danh sách tài liệu một phiên NHÌN THẤY: tài liệu của phiên + kho chung (NULL).
    pub fn doc_list(&self, discussion_id: Option<i64>, limit: i64) -> Result<Vec<DocRow>> {
        let conn = self.lock();
        let mut stmt = match discussion_id {
            Some(_) => conn.prepare(&format!(
                "SELECT {} FROM documents WHERE discussion_id = ?1 OR discussion_id IS NULL ORDER BY id DESC LIMIT ?2",
                Self::DOC_COLS
            ))?,
            None => conn.prepare(&format!(
                "SELECT {} FROM documents WHERE 1 = COALESCE(?1, 1) ORDER BY id DESC LIMIT ?2",
                Self::DOC_COLS
            ))?,
        };
        let rows = match discussion_id {
            Some(d) => stmt.query_map(params![d, limit], Self::row_to_doc)?,
            None => stmt.query_map(params![Option::<i64>::None, limit], Self::row_to_doc)?,
        };
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn doc_search(&self, q: &str, discussion_id: Option<i64>, limit: i64) -> Result<Vec<DocRow>> {
        let ftsq = fts_query(q);
        if ftsq.is_empty() {
            return self.doc_list(discussion_id, limit);
        }
        let conn = self.lock();
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM documents
             WHERE id IN (SELECT rowid FROM documents_fts WHERE documents_fts MATCH ?1)
               AND (?2 IS NULL OR discussion_id = ?2 OR discussion_id IS NULL)
             ORDER BY id DESC LIMIT ?3",
            Self::DOC_COLS
        ))?;
        let rows = stmt.query_map(params![ftsq, discussion_id, limit], Self::row_to_doc)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn doc_set_filename(&self, id: i64, filename: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE documents SET filename = ?2 WHERE id = ?1",
            params![id, filename],
        )?;
        Ok(())
    }

    pub fn doc_delete(&self, id: i64) -> Result<()> {
        let conn = self.lock();
        conn.execute("DELETE FROM documents WHERE id = ?1", params![id])?;
        conn.execute("DELETE FROM documents_fts WHERE rowid = ?1", params![id])?;
        Ok(())
    }

    pub fn doc_exists(&self, id: i64) -> bool {
        let conn = self.lock();
        conn.query_row("SELECT 1 FROM documents WHERE id = ?1", params![id], |_| Ok(()))
            .is_ok()
    }

    // ---------------- Member memory & thinking ----------------

    pub fn memory_add(&self, member_id: i64, discussion_id: Option<i64>, kind: &str, content: &str) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO member_memory (member_id, discussion_id, kind, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![member_id, discussion_id, kind, content, now()],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO member_memory_fts (rowid, content) VALUES (?1, ?2)",
            params![id, fold(content)],
        )?;
        Ok(id)
    }

    fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRow> {
        Ok(MemoryRow {
            id: row.get(0)?,
            member_id: row.get(1)?,
            discussion_id: row.get(2)?,
            kind: row.get(3)?,
            content: row.get(4)?,
            created_at: row.get(5)?,
        })
    }

    /// Recall bộ nhớ riêng: FTS theo chủ đề + fallback gần đây. Riêng tư per-member.
    pub fn memory_recall(&self, member_id: i64, query: &str, limit: i64) -> Result<Vec<MemoryRow>> {
        let ftsq = fts_query(query);
        let conn = self.lock();
        let mut out: Vec<MemoryRow> = Vec::new();
        if !ftsq.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT m.id, m.member_id, m.discussion_id, m.kind, m.content, m.created_at
                 FROM member_memory m
                 WHERE m.member_id = ?1 AND m.id IN (SELECT rowid FROM member_memory_fts WHERE member_memory_fts MATCH ?2)
                 ORDER BY m.id DESC LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![member_id, ftsq, limit], Self::row_to_memory)?;
            out = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        }
        if out.len() < limit as usize {
            let need = limit - out.len() as i64;
            let mut stmt = conn.prepare(
                "SELECT id, member_id, discussion_id, kind, content, created_at FROM member_memory
                 WHERE member_id = ?1 ORDER BY id DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![member_id, need + out.len() as i64], Self::row_to_memory)?;
            for r in rows {
                let r = r?;
                if !out.iter().any(|x| x.id == r.id) {
                    out.push(r);
                    if out.len() >= limit as usize {
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn memory_list(&self, member_id: i64, limit: i64) -> Result<Vec<MemoryRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, member_id, discussion_id, kind, content, created_at FROM member_memory
             WHERE member_id = ?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![member_id, limit], Self::row_to_memory)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn thinking_add(&self, member_id: i64, discussion_id: i64, round: i64, content: &str) -> Result<()> {
        self.lock().execute(
            "INSERT INTO member_thinking (member_id, discussion_id, round, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![member_id, discussion_id, round, content, now()],
        )?;
        Ok(())
    }

    /// Mạch suy nghĩ gần nhất của CHÍNH member đó (giữ nhất quán lập trường giữa các lượt).
    pub fn thinking_recent(&self, member_id: i64, discussion_id: i64, limit: i64) -> Result<Vec<(i64, String)>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT round, content FROM member_thinking
             WHERE member_id = ?1 AND discussion_id = ?2 ORDER BY id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![member_id, discussion_id, limit], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut v = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        v.reverse();
        Ok(v)
    }

    // ---------------- Minutes & results ----------------

    pub fn minutes_add(&self, discussion_id: i64, round: i64, content: &str) -> Result<()> {
        self.lock().execute(
            "INSERT INTO minutes (discussion_id, round, content, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![discussion_id, round, content, now()],
        )?;
        Ok(())
    }

    pub fn minutes_latest(&self, discussion_id: i64) -> Result<Option<MinutesRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, discussion_id, round, content, created_at FROM minutes
             WHERE discussion_id = ?1 ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![discussion_id], |r| {
            Ok(MinutesRow {
                id: r.get(0)?,
                discussion_id: r.get(1)?,
                round: r.get(2)?,
                content: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn result_add(&self, discussion_id: i64, content: &str) -> Result<i64> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO results (discussion_id, content, status, created_at) VALUES (?1, ?2, 'draft', ?3)",
            params![discussion_id, content, now()],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn result_latest(&self, discussion_id: i64) -> Result<Option<ResultRow>> {
        let conn = self.lock();
        let mut stmt = conn.prepare(
            "SELECT id, discussion_id, content, status, feedback, created_at FROM results
             WHERE discussion_id = ?1 ORDER BY id DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![discussion_id], |r| {
            Ok(ResultRow {
                id: r.get(0)?,
                discussion_id: r.get(1)?,
                content: r.get(2)?,
                status: r.get(3)?,
                feedback: r.get(4)?,
                created_at: r.get(5)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn result_set_status(&self, id: i64, status: &str, feedback: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE results SET status = ?2, feedback = ?3 WHERE id = ?1",
            params![id, status, feedback],
        )?;
        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_vietnamese() {
        assert_eq!(fold("Dẫn chứng ĐẦY đủ"), "dan chung day du");
        assert_eq!(fold("sáng tạo"), "sang tao");
    }

    #[test]
    fn seed_roster_has_roles() {
        let db = Db::open_memory().unwrap();
        let members = db.member_list().unwrap();
        assert!(members.iter().any(|m| m.role == "manager"));
        assert!(members.iter().any(|m| m.role == "secretary"));
        assert!(members.iter().filter(|m| m.role == "member" && m.enabled).count() >= 4);
        // seed idempotent
        db.seed_defaults().unwrap();
        assert_eq!(db.member_list().unwrap().len(), members.len());
    }

    #[test]
    fn discussion_lifecycle() {
        let db = Db::open_memory().unwrap();
        let members: Vec<i64> = db
            .member_list()
            .unwrap()
            .into_iter()
            .filter(|m| m.role == "member" && m.enabled)
            .map(|m| m.id)
            .collect();
        let id = db
            .discussion_create("Chủ đề thử", "Cần 3 kết luận", "sequential", 10, 8, &members)
            .unwrap();
        let d = db.discussion_get(id).unwrap().unwrap();
        assert_eq!(d.status, "draft");
        assert_eq!(db.discussion_members(id).unwrap().len(), members.len());
        db.discussion_set_status(id, "running").unwrap();
        assert_eq!(db.discussions_with_status("running").unwrap().len(), 1);
    }

    #[test]
    fn open_opinions_and_reactions() {
        let db = Db::open_memory().unwrap();
        let ms = db.member_list().unwrap();
        let m1 = ms.iter().find(|m| m.key == "an-dan-chung").unwrap().id;
        let m2 = ms.iter().find(|m| m.key == "binh-phan-bien").unwrap().id;
        let d = db.discussion_create("t", "r", "sequential", 0, 8, &[m1, m2]).unwrap();
        let op = db
            .message_insert(&NewMessage {
                discussion_id: d,
                round: 1,
                author_kind: "member".into(),
                member_id: Some(m1),
                kind: "opinion".into(),
                content: "Luận điểm A".into(),
                claim_type: Some("evidence".into()),
                citations: serde_json::json!([{"kind":"url","ref":"https://x"}]),
                flags: serde_json::json!({}),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(db.open_opinions(d, 10).unwrap().len(), 1);
        // Tự phản hồi mình KHÔNG đóng luận điểm
        db.message_insert(&NewMessage {
            discussion_id: d,
            round: 1,
            author_kind: "member".into(),
            member_id: Some(m1),
            kind: "reaction".into(),
            content: "tự bổ sung".into(),
            stance: Some("agree".into()),
            reply_to: Some(op),
            citations: serde_json::json!([]),
            flags: serde_json::json!({}),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(db.open_opinions(d, 10).unwrap().len(), 1);
        // Member khác phản hồi → đóng
        db.message_insert(&NewMessage {
            discussion_id: d,
            round: 1,
            author_kind: "member".into(),
            member_id: Some(m2),
            kind: "reaction".into(),
            content: "đồng tình".into(),
            stance: Some("agree".into()),
            reply_to: Some(op),
            citations: serde_json::json!([]),
            flags: serde_json::json!({}),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(db.open_opinions(d, 10).unwrap().len(), 0);
    }

    #[test]
    fn docs_fts_fold() {
        let db = Db::open_memory().unwrap();
        let id = db
            .doc_add(None, "Định hướng sản phẩm", "doc.md", "Chiến lược dẫn chứng đầy đủ", "paste", "boss")
            .unwrap();
        let hits = db.doc_search("dan chung", None, 10).unwrap();
        assert!(hits.iter().any(|d| d.id == id));
        let hits2 = db.doc_search("dẫn chứng", None, 10).unwrap();
        assert!(hits2.iter().any(|d| d.id == id));
        db.doc_delete(id).unwrap();
        assert!(db.doc_search("dan chung", None, 10).unwrap().is_empty());
    }

    #[test]
    fn member_memory_recall() {
        let db = Db::open_memory().unwrap();
        let m = db.member_list().unwrap()[2].id;
        db.memory_add(m, None, "fact", "Giá điện tăng 4.8% từ tháng 5").unwrap();
        db.memory_add(m, None, "stance", "Tôi ủng hộ phương án điện mặt trời áp mái").unwrap();
        let hits = db.memory_recall(m, "điện mặt trời", 5).unwrap();
        assert!(!hits.is_empty());
        // member khác không thấy
        let other = db.member_list().unwrap()[3].id;
        let none = db.memory_recall(other, "điện mặt trời", 5).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn participation_counts() {
        let db = Db::open_memory().unwrap();
        let ms = db.member_list().unwrap();
        let m1 = ms.iter().find(|m| m.key == "an-dan-chung").unwrap().id;
        let m2 = ms.iter().find(|m| m.key == "chi-suy-luan").unwrap().id;
        let d = db.discussion_create("t", "r", "sequential", 0, 8, &[m1, m2]).unwrap();
        db.message_insert(&NewMessage {
            discussion_id: d,
            round: 2,
            author_kind: "member".into(),
            member_id: Some(m1),
            kind: "opinion".into(),
            content: "x".into(),
            citations: serde_json::json!([]),
            flags: serde_json::json!({}),
            ..Default::default()
        })
        .unwrap();
        let p = db.participation(d, 3).unwrap();
        let p1 = p.iter().find(|x| x.member_id == m1).unwrap();
        let p2 = p.iter().find(|x| x.member_id == m2).unwrap();
        assert_eq!(p1.silent_rounds, 1);
        assert_eq!(p2.silent_rounds, 3);
    }
}
