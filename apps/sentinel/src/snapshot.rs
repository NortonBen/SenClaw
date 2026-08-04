//! Ảnh chụp cấu hình + so sánh.
//!
//! Đây là phần Sentinel **tạo ra dữ liệu daemon không hề có**. Trong SenClaw,
//! `tool_rules`, `groups.allowed_tools`, `hooks.json`, danh sách MCP server đều
//! ghi đè tại chỗ: không version, không `changed_at`, không actor. Nghĩa là câu
//! hỏi "hôm qua cấu hình khác hôm nay chỗ nào" hiện không trả lời được.
//!
//! Cách làm: chụp định kỳ, băm nội dung, chỉ lưu khi băm đổi, rồi sinh diff
//! `added / removed / changed` để luật đọc. Nhờ vậy mới phát hiện được kiểu tấn
//! công "mở cửa rồi đi qua" — thêm một luật auto-accept rộng, rồi dùng ngay.

use crate::db::Db;
use crate::source::{DaemonDb, DaemonRest};
use serde_json::{json, Map, Value};

/// Các nhóm được chụp. Tên nhóm cũng là khoá dùng trong `snapshots.kind`.
pub const KINDS: &[&str] = &[
    "mcp_servers",
    "mcp_tool_manifest",
    "tool_rules",
    "groups",
    "hooks",
    "admin_permissions",
    "skills",
    "plugins",
    "schedules",
];

/// Quy về `{khoá → giá trị}` để diff theo khoá thay vì so chuỗi thô. Chọn khoá
/// ổn định (id/tên) là điều kiện để diff có nghĩa: nếu khoá đổi mỗi lần chụp thì
/// mọi lần chụp đều thành "xoá hết + thêm hết".
fn keyed(items: &[Value], key_fields: &[&str]) -> Value {
    let mut m = Map::new();
    for (i, it) in items.iter().enumerate() {
        let k = key_fields
            .iter()
            .find_map(|f| it[*f].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("#{i}"));
        m.insert(k, it.clone());
    }
    Value::Object(m)
}

/// Diff hai map. `changed` giữ cả giá trị cũ và mới để giao diện hiện kiểu git.
pub fn diff_maps(from: &Value, to: &Value) -> (Value, Value, Value) {
    let empty = Map::new();
    let a = from.as_object().unwrap_or(&empty);
    let b = to.as_object().unwrap_or(&empty);

    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    for (k, v) in b {
        match a.get(k) {
            None => added.push(json!({ "key": k, "value": v })),
            Some(old) if old != v => changed.push(json!({ "key": k, "from": old, "to": v })),
            _ => {}
        }
    }
    for (k, v) in a {
        if !b.contains_key(k) {
            removed.push(json!({ "key": k, "value": v }));
        }
    }
    (json!(added), json!(removed), json!(changed))
}

/// Rút manifest tool của mọi MCP server: `{server → {tool → băm(description)}}`.
/// Băm chứ không giữ nguyên văn, vì mục đích là phát hiện **thay đổi** (rug pull)
/// chứ không phải lưu lại mô tả.
fn tool_manifest(servers: &Value) -> Value {
    let list = servers["servers"].as_array().cloned().unwrap_or_default();
    let mut m = Map::new();
    for s in list {
        let name = s["name"].as_str().unwrap_or("?").to_string();
        let mut tools = Map::new();
        if let Some(ts) = s["tools"].as_array() {
            for t in ts {
                let tn = t["name"].as_str().unwrap_or("?").to_string();
                let desc = t["description"].as_str().unwrap_or("");
                tools.insert(tn, json!(crate::db::sha256_hex(desc)));
            }
        }
        m.insert(name, Value::Object(tools));
    }
    Value::Object(m)
}

pub struct SnapshotReport {
    pub taken: Vec<String>,
    pub changed: Vec<String>,
    pub missing: Vec<String>,
}

impl SnapshotReport {
    pub fn to_value(&self) -> Value {
        json!({
            "taken": self.taken,
            "changed": self.changed,
            "missing": self.missing,
        })
    }
}

/// Chụp toàn bộ 9 nhóm. Nguồn nào không lấy được thì ghi vào `missing` và bỏ
/// qua — **không** lưu ảnh rỗng, vì một ảnh rỗng sẽ bị diff hiểu thành "toàn bộ
/// cấu hình vừa bị xoá" và tạo ra cảnh báo giả.
pub async fn take_all(db: &Db) -> SnapshotReport {
    let mut rep = SnapshotReport {
        taken: vec![],
        changed: vec![],
        missing: vec![],
    };
    let rest = DaemonRest::new();

    fn put(db: &Db, kind: &str, body: Value, rep: &mut SnapshotReport) {
        match db.put_snapshot(kind, &body) {
            Ok(Some((from_id, to_id))) => {
                rep.taken.push(kind.to_string());
                if from_id > 0 {
                    let old = db
                        .snapshot_body(from_id)
                        .unwrap_or(None)
                        .unwrap_or_else(|| json!({}));
                    let (a, r, c) = diff_maps(&old, &body);
                    let any = a.as_array().map(|x| !x.is_empty()).unwrap_or(false)
                        || r.as_array().map(|x| !x.is_empty()).unwrap_or(false)
                        || c.as_array().map(|x| !x.is_empty()).unwrap_or(false);
                    if any {
                        let _ = db.put_diff(kind, from_id, to_id, &a, &r, &c);
                        rep.changed.push(kind.to_string());
                    }
                }
            }
            Ok(None) => rep.taken.push(kind.to_string()),
            Err(e) => rep.missing.push(format!("{kind}: {e}")),
        }
    }

    // ---- nguồn REST ----
    if let Some(v) = rest.mcp_servers().await {
        let list = v["servers"].as_array().cloned().unwrap_or_default();
        // Chỉ giữ trường có ý nghĩa an ninh; `status` đổi liên tục nên loại ra,
        // nếu không mỗi lần chụp đều báo "đã thay đổi".
        let slim: Vec<Value> = list
            .iter()
            .map(|s| {
                json!({
                    "name": s["name"].clone(),
                    "transport": s["transport"].clone(),
                    "url": s["url"].clone(),
                    "enabled": s["enabled"].clone(),
                    "builtin": s["builtin"].clone(),
                    "tool_count": s["tools"].as_array().map(|t| t.len()).unwrap_or(0),
                })
            })
            .collect();
        put(db, "mcp_servers", keyed(&slim, &["name"]), &mut rep);
        put(db, "mcp_tool_manifest", tool_manifest(&v), &mut rep);
    } else {
        rep.missing.push("mcp_servers: daemon không trả JSON".into());
    }

    if let Some(v) = rest.admin_permissions().await {
        put(db, "admin_permissions", v, &mut rep);
    } else {
        rep.missing
            .push("admin_permissions: daemon không trả JSON".into());
    }

    if let Some(v) = rest.hooks().await {
        put(db, "hooks", v, &mut rep);
    } else {
        rep.missing.push("hooks: daemon không trả JSON".into());
    }

    if let Some(v) = rest.skills().await {
        let items = as_list(&v, &["skills", "items"]);
        put(db, "skills", keyed(&items, &["name", "id"]), &mut rep);
    } else {
        rep.missing.push("skills: daemon không trả JSON".into());
    }

    if let Some(v) = rest.plugins().await {
        let items = as_list(&v, &["plugins", "items"]);
        put(db, "plugins", keyed(&items, &["slug", "name", "id"]), &mut rep);
    } else {
        rep.missing.push("plugins: daemon không trả JSON".into());
    }

    // ---- nguồn SQLite ----
    match DaemonDb::open() {
        Ok(d) => {
            if let Ok(rules) = d.tool_rules() {
                put(db, "tool_rules", keyed(&rules, &["id"]), &mut rep);
            }
            if let Ok(groups) = d.groups() {
                let slim: Vec<Value> = groups
                    .iter()
                    .map(|g| {
                        json!({
                            "jid": g["jid"].clone(),
                            "folder": g["folder"].clone(),
                            "allowed_tools": g["allowed_tools"].clone(),
                            "approved_tools": g["approved_tools"].clone(),
                            "allowed_work_dirs": g["allowed_work_dirs"].clone(),
                        })
                    })
                    .collect();
                put(db, "groups", keyed(&slim, &["jid"]), &mut rep);
            }
            if let Ok(tasks) = d.scheduled_tasks() {
                let slim: Vec<Value> = tasks
                    .iter()
                    .map(|t| {
                        json!({
                            "id": t["id"].clone(),
                            "group_folder": t["group_folder"].clone(),
                            "context_mode": t["context_mode"].clone(),
                            "schedule_type": t["schedule_type"].clone(),
                            "schedule_value": t["schedule_value"].clone(),
                            "script_command": t["script_command"].clone(),
                            "status": t["status"].clone(),
                        })
                    })
                    .collect();
                put(db, "schedules", keyed(&slim, &["id"]), &mut rep);
            }
        }
        Err(e) => rep.missing.push(format!("sqlite: {e}")),
    }

    rep
}

/// Nhận cả `{"skills":[...]}`, `{"items":[...]}` lẫn mảng trần — các endpoint của
/// daemon không thống nhất một hình dạng.
fn as_list(v: &Value, keys: &[&str]) -> Vec<Value> {
    if let Some(a) = v.as_array() {
        return a.clone();
    }
    for k in keys {
        if let Some(a) = v[*k].as_array() {
            return a.clone();
        }
    }
    vec![]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_reports_added_removed_changed() {
        let a = json!({"x": {"v": 1}, "y": {"v": 2}});
        let b = json!({"y": {"v": 99}, "z": {"v": 3}});
        let (added, removed, changed) = diff_maps(&a, &b);
        assert_eq!(added.as_array().unwrap().len(), 1);
        assert_eq!(added[0]["key"], "z");
        assert_eq!(removed.as_array().unwrap().len(), 1);
        assert_eq!(removed[0]["key"], "x");
        assert_eq!(changed.as_array().unwrap().len(), 1);
        assert_eq!(changed[0]["key"], "y");
        assert_eq!(changed[0]["from"]["v"], 2);
        assert_eq!(changed[0]["to"]["v"], 99);
    }

    #[test]
    fn identical_bodies_produce_no_diff() {
        let a = json!({"x": 1});
        let (added, removed, changed) = diff_maps(&a, &a);
        assert!(added.as_array().unwrap().is_empty());
        assert!(removed.as_array().unwrap().is_empty());
        assert!(changed.as_array().unwrap().is_empty());
    }

    #[test]
    fn keyed_uses_first_available_key_field() {
        let items = vec![json!({"name": "a"}), json!({"id": "b"}), json!({"other": 1})];
        let m = keyed(&items, &["name", "id"]);
        assert!(m["a"].is_object());
        assert!(m["b"].is_object());
        assert!(m["#2"].is_object(), "không có khoá thì rơi về chỉ số");
    }

    #[test]
    fn tool_manifest_hashes_descriptions() {
        let servers = json!({"servers": [
            {"name": "s1", "tools": [{"name": "t1", "description": "mô tả gốc"}]}
        ]});
        let m1 = tool_manifest(&servers);
        let servers2 = json!({"servers": [
            {"name": "s1", "tools": [{"name": "t1", "description": "mô tả ĐÃ BỊ ĐỔI"}]}
        ]});
        let m2 = tool_manifest(&servers2);
        assert_ne!(m1["s1"]["t1"], m2["s1"]["t1"], "đổi mô tả phải đổi băm");
        assert!(
            !m1.to_string().contains("mô tả gốc"),
            "chỉ lưu băm, không lưu nguyên văn"
        );
    }

    #[test]
    fn as_list_accepts_three_shapes() {
        assert_eq!(as_list(&json!([1, 2]), &["skills"]).len(), 2);
        assert_eq!(as_list(&json!({"skills": [1]}), &["skills"]).len(), 1);
        assert_eq!(
            as_list(&json!({"items": [1, 2, 3]}), &["skills", "items"]).len(),
            3
        );
        assert_eq!(as_list(&json!({"gì đó": 1}), &["skills"]).len(), 0);
    }

    #[test]
    fn kinds_are_unique_and_complete() {
        let set: std::collections::HashSet<_> = KINDS.iter().collect();
        assert_eq!(set.len(), KINDS.len());
        assert_eq!(KINDS.len(), 9);
    }
}
