//! DSL khai báo flow (design §6.1): parse + validate + DAG + canonical JSON.
//!
//! **YAML (§6.1)**: `parse` sniff ký tự không-trắng đầu tiên — `{` → JSON object;
//! ngược lại → YAML (`serde_yaml_ng`, fork còn maintain). YAML deserialize thẳng về
//! `FlowDef` rồi chuẩn hoá về JSON canonical khi lưu (`to_canonical_json`). Chữ ký
//! `parse` giữ nguyên: caller không cần biết nguồn là JSON hay YAML.
//!
//! Quy tắc load-bearing:
//!   * validate trả DANH SÁCH `FieldError{step_id, field, message}` (§6.1) — không
//!     fail-fast, để UI/agent thấy hết lỗi một lần.
//!   * Bốn mode chạy được: `full_refresh` + `incremental_append` (Phase 2) và
//!     `incremental_merge` + `snapshot` SCD2 (Phase 3, ràng buộc partition ở validate).
//!   * DAG suy từ alias step trong FROM/JOIN của transform SQL + `exports[].input`;
//!     phát hiện chu trình (Kahn).

#![allow(dead_code)]

use std::collections::{HashMap, HashSet};

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Cấu trúc DSL
// ---------------------------------------------------------------------------

fn default_version() -> i64 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlowDef {
    #[serde(default = "default_version")]
    pub version: i64,
    pub flow: String,
    #[serde(default)]
    pub sources: Vec<SourceStep>,
    #[serde(default)]
    pub transforms: Vec<TransformStep>,
    #[serde(default)]
    pub exports: Vec<ExportStep>,
    /// Lịch tự chạy (§6.6). None = chỉ chạy thủ công.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
}

/// Lịch tự chạy của flow (§6.6). Untagged: `{"every_minutes":N}` hoặc
/// `{"daily_at":"HH:MM"}`. Scheduler tick 30s đọc field này (mirror sang cột
/// `flow.schedule` để đọc không cần parse def).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Schedule {
    Every { every_minutes: i64 },
    Daily { daily_at: String },
}

/// Parse "HH:MM" → (giờ, phút) nếu hợp lệ (0≤h≤23, 0≤m≤59). Dùng cho `daily_at`.
pub fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.split_once(':')?;
    let h: u32 = h.trim().parse().ok()?;
    let m: u32 = m.trim().parse().ok()?;
    (h < 24 && m < 60).then_some((h, m))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SourceStep {
    pub id: String,
    #[serde(default)]
    pub connection: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(default)]
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<Cursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_key: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_key: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    /// Projection cột tùy chọn (mở rộng ngoài §6.1 gốc — None = tất cả).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_policy: Option<Value>,
    /// merge: delete_insert|upsert|insert_only. snapshot: timestamp|check (§6.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    /// merge không partition_by → chấp nhận rewrite TOÀN BỘ dataset mỗi run (§6.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_full_rewrite: Option<bool>,
    /// snapshot: xử lý row nguồn biến mất — ignore|invalidate|new_record (§6.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_deletes: Option<String>,
    /// snapshot strategy=check: cột so đổi (rỗng/None = toàn bộ cột) (§6.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_columns: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Cursor {
    pub column: String,
    /// Giá trị khởi tạo khi chưa có watermark (dlt `initial_value`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial: Option<Value>,
    /// Cửa sổ đọc lùi late-data ("1h"/"2d") — Phase 3 dùng, Phase 2 chấp nhận & bỏ qua.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Target {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partition_by: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TransformStep {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_column: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lookback: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    #[serde(default)]
    pub sql: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExportStep {
    pub id: String,
    #[serde(default)]
    pub input: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default)]
    pub mode: String,
    /// Khoá upsert (DB-load mode=upsert) — bắt buộc khi mode=upsert (§5.1 LoadMode).
    #[serde(default)]
    pub keys: Vec<String>,
}

/// Lỗi validate một trường (§6.1). `step_id` rỗng = lỗi cấp flow.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FieldError {
    pub step_id: String,
    pub field: String,
    pub message: String,
}

impl FieldError {
    fn new(step_id: &str, field: &str, message: impl Into<String>) -> Self {
        Self {
            step_id: step_id.to_string(),
            field: field.to_string(),
            message: message.into(),
        }
    }
}

// ---------------------------------------------------------------------------
// enum mode hỗ trợ ở Phase 2
// ---------------------------------------------------------------------------

const MODES_ALL: &[&str] = &[
    "full_refresh",
    "incremental_append",
    "incremental_merge",
    "snapshot",
];
const TRANSFORM_KINDS: &[&str] = &["full", "incremental_by_time"];
const INTERVALS: &[&str] = &["hour", "day", "week", "month"];
const EXPORT_MODES: &[&str] = &["full_refresh", "append", "upsert"];
const MERGE_STRATEGIES: &[&str] = &["delete_insert", "upsert", "insert_only"];
const SNAPSHOT_STRATEGIES: &[&str] = &["timestamp", "check"];
const HARD_DELETES: &[&str] = &["ignore", "invalidate", "new_record"];

// ---------------------------------------------------------------------------
// parse
// ---------------------------------------------------------------------------

/// Parse định nghĩa flow. Sniff: ký tự không-trắng đầu tiên là `{` → JSON object;
/// ngược lại → YAML. Cả hai deserialize về cùng `FlowDef`.
pub fn parse(def: &str) -> Result<FlowDef> {
    let trimmed = def.trim_start();
    match trimmed.chars().next() {
        Some('{') => serde_json::from_str::<FlowDef>(def)
            .map_err(|e| anyhow!("flow def JSON không hợp lệ: {e}")),
        Some(_) => serde_yaml_ng::from_str::<FlowDef>(def)
            .map_err(|e| anyhow!("flow def YAML không hợp lệ: {e}")),
        None => Err(anyhow!("flow def rỗng")),
    }
}

// ---------------------------------------------------------------------------
// target mặc định
// ---------------------------------------------------------------------------

/// (namespace, dataset) của một source step — default `raw/<id>` (§6.1).
pub fn source_target(step: &SourceStep) -> (String, String) {
    resolve_target(step.target.as_ref(), "raw", &step.id)
}

/// (namespace, dataset) của một transform step — default `marts/<id>` (§6.1).
pub fn transform_target(step: &TransformStep) -> (String, String) {
    resolve_target(step.target.as_ref(), "marts", &step.id)
}

fn resolve_target(t: Option<&Target>, default_ns: &str, id: &str) -> (String, String) {
    let ns = t
        .and_then(|t| t.namespace.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| default_ns.to_string());
    let ds = t
        .and_then(|t| t.dataset.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| id.to_string());
    (ns, ds)
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

/// Validate theo bảng §6.1. Trả `Err(Vec<FieldError>)` với TẤT CẢ lỗi tìm được.
pub fn validate(def: &FlowDef) -> std::result::Result<(), Vec<FieldError>> {
    let mut errs = Vec::new();

    // flow id: [a-z0-9_-]{1,64}
    if !is_valid_flow_id(&def.flow) {
        errs.push(FieldError::new(
            "",
            "flow",
            "id flow phải khớp [a-z0-9_-]{1,64}",
        ));
    }

    // id unique toàn cục (DAG cần).
    let mut seen: HashSet<&str> = HashSet::new();
    let all_ids: Vec<&str> = def
        .sources
        .iter()
        .map(|s| s.id.as_str())
        .chain(def.transforms.iter().map(|t| t.id.as_str()))
        .chain(def.exports.iter().map(|e| e.id.as_str()))
        .collect();
    for id in &all_ids {
        if id.trim().is_empty() {
            errs.push(FieldError::new("", "id", "step thiếu id"));
        } else if !seen.insert(id) {
            errs.push(FieldError::new(id, "id", "id step trùng trong flow"));
        }
    }

    for s in &def.sources {
        validate_source(s, &mut errs);
    }
    let known: HashSet<&str> = all_ids.iter().copied().collect();
    for t in &def.transforms {
        validate_transform(t, &mut errs);
    }
    for e in &def.exports {
        validate_export(e, &known, &mut errs);
    }

    // schedule (§6.6): every_minutes ≥ 1; daily_at dạng HH:MM.
    if let Some(sch) = &def.schedule {
        match sch {
            Schedule::Every { every_minutes } if *every_minutes < 1 => {
                errs.push(FieldError::new("", "schedule.every_minutes", "phải ≥ 1"));
            }
            Schedule::Daily { daily_at } if parse_hhmm(daily_at).is_none() => {
                errs.push(FieldError::new(
                    "",
                    "schedule.daily_at",
                    "phải dạng HH:MM (00:00–23:59)",
                ));
            }
            _ => {}
        }
    }

    // Chu trình → lỗi cấp flow.
    if errs.is_empty() {
        if let Err(cyc) = derive_dag(def) {
            errs.extend(cyc);
        }
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

fn validate_source(s: &SourceStep, errs: &mut Vec<FieldError>) {
    let id = &s.id;
    if s.connection.trim().is_empty() {
        errs.push(FieldError::new(id, "connection", "bắt buộc"));
    }
    // Đúng một trong table/query.
    match (s.table.as_deref(), s.query.as_deref()) {
        (Some(t), None) if !t.trim().is_empty() => {}
        (None, Some(q)) if !q.trim().is_empty() => {}
        (Some(_), Some(_)) => errs.push(FieldError::new(
            id,
            "table/query",
            "chỉ được khai MỘT trong table hoặc query",
        )),
        _ => errs.push(FieldError::new(
            id,
            "table/query",
            "bắt buộc MỘT trong table hoặc query",
        )),
    }

    // mode.
    if !MODES_ALL.contains(&s.mode.as_str()) {
        errs.push(FieldError::new(
            id,
            "mode",
            format!("mode không hợp lệ '{}'; hợp lệ: {}", s.mode, MODES_ALL.join(", ")),
        ));
    }

    let is_incremental = s.mode == "incremental_append" || s.mode == "incremental_merge";
    if is_incremental {
        match &s.cursor {
            None => errs.push(FieldError::new(id, "cursor", "bắt buộc khi mode incremental_*")),
            Some(c) => {
                if c.column.trim().is_empty() {
                    errs.push(FieldError::new(id, "cursor.column", "bắt buộc"));
                }
                if c.initial.is_none() {
                    errs.push(FieldError::new(id, "cursor.initial", "bắt buộc"));
                }
            }
        }
    }

    // merge/snapshot: primary_key bắt buộc.
    if s.mode == "incremental_merge" || s.mode == "snapshot" {
        if s.primary_key.as_ref().is_none_or(|k| k.is_empty()) {
            errs.push(FieldError::new(id, "primary_key", "bắt buộc khi merge/snapshot"));
        }
    }

    if s.mode == "incremental_merge" {
        validate_merge(s, errs);
    }
    if s.mode == "snapshot" {
        validate_snapshot(s, errs);
    }
}

/// Ràng buộc vật lý merge (§6.2): partition_by bắt buộc (trừ allow_full_rewrite),
/// merge_key ⊆ partition_by, strategy hợp lệ.
fn validate_merge(s: &SourceStep, errs: &mut Vec<FieldError>) {
    let id = &s.id;
    if let Some(st) = s.strategy.as_deref() {
        if !MERGE_STRATEGIES.contains(&st) {
            errs.push(FieldError::new(
                id,
                "strategy",
                format!("strategy merge không hợp lệ '{st}'; hợp lệ: {}", MERGE_STRATEGIES.join(", ")),
            ));
        }
    }
    let allow_full = s.allow_full_rewrite.unwrap_or(false);
    let partition: HashSet<&str> = s
        .target
        .as_ref()
        .and_then(|t| t.partition_by.as_ref())
        .map(|p| p.iter().map(|x| x.as_str()).collect())
        .unwrap_or_default();

    if partition.is_empty() && !allow_full {
        errs.push(FieldError::new(
            id,
            "target.partition_by",
            "merge bắt buộc target có partition_by (merge_key ⊆ partition_by); \
             hoặc allow_full_rewrite:true để rewrite TOÀN BỘ dataset mỗi run",
        ));
    }
    // insert_only không cần merge_key (identity theo primary_key).
    let strategy = s.strategy.as_deref().unwrap_or("delete_insert");
    if strategy != "insert_only" {
        match &s.merge_key {
            Some(keys) if !keys.is_empty() => {
                if !partition.is_empty() {
                    for k in keys {
                        if !partition.contains(k.as_str()) {
                            errs.push(FieldError::new(
                                id,
                                "merge_key",
                                format!("merge_key '{k}' phải nằm trong target.partition_by"),
                            ));
                        }
                    }
                }
            }
            _ if !allow_full => {
                errs.push(FieldError::new(id, "merge_key", "bắt buộc khi merge (trừ allow_full_rewrite)"));
            }
            _ => {}
        }
    }
}

/// Ràng buộc snapshot SCD2 (§6.2): strategy hợp lệ; timestamp cần cột updated_at
/// (cursor.column); hard_deletes hợp lệ. Partition SCD2 = `_is_current` (engine tự lo).
fn validate_snapshot(s: &SourceStep, errs: &mut Vec<FieldError>) {
    let id = &s.id;
    let strategy = s.strategy.as_deref().unwrap_or("check");
    if !SNAPSHOT_STRATEGIES.contains(&strategy) {
        errs.push(FieldError::new(
            id,
            "strategy",
            format!("strategy snapshot không hợp lệ '{strategy}'; hợp lệ: {}", SNAPSHOT_STRATEGIES.join(", ")),
        ));
    }
    if strategy == "timestamp"
        && s.cursor.as_ref().map(|c| c.column.trim().is_empty()).unwrap_or(true)
    {
        errs.push(FieldError::new(
            id,
            "cursor.column",
            "snapshot strategy=timestamp cần cursor.column (cột updated_at)",
        ));
    }
    if let Some(hd) = s.hard_deletes.as_deref() {
        if !HARD_DELETES.contains(&hd) {
            errs.push(FieldError::new(
                id,
                "hard_deletes",
                format!("hard_deletes không hợp lệ '{hd}'; hợp lệ: {}", HARD_DELETES.join(", ")),
            ));
        }
    }
}

fn validate_transform(t: &TransformStep, errs: &mut Vec<FieldError>) {
    let id = &t.id;
    if !TRANSFORM_KINDS.contains(&t.kind.as_str()) {
        errs.push(FieldError::new(
            id,
            "kind",
            format!("kind không hợp lệ '{}'; hợp lệ: {}", t.kind, TRANSFORM_KINDS.join(", ")),
        ));
    }
    if t.sql.trim().is_empty() {
        errs.push(FieldError::new(id, "sql", "bắt buộc (SELECT-only)"));
    }
    if t.kind == "incremental_by_time" {
        if t.time_column.as_deref().unwrap_or("").trim().is_empty() {
            errs.push(FieldError::new(id, "time_column", "bắt buộc khi incremental_by_time"));
        }
        match &t.interval {
            Some(i) if INTERVALS.contains(&i.as_str()) => {}
            _ => errs.push(FieldError::new(
                id,
                "interval",
                format!("bắt buộc, một trong: {}", INTERVALS.join(", ")),
            )),
        }
        match t.lookback {
            Some(n) if n >= 0 => {}
            _ => errs.push(FieldError::new(id, "lookback", "bắt buộc, số nguyên ≥ 0")),
        }
    }
}

fn validate_export(e: &ExportStep, known: &HashSet<&str>, errs: &mut Vec<FieldError>) {
    let id = &e.id;
    if e.input.trim().is_empty() {
        errs.push(FieldError::new(id, "input", "bắt buộc"));
    } else if !known.contains(e.input.as_str()) {
        errs.push(FieldError::new(id, "input", format!("input '{}' không phải step trong flow", e.input)));
    }
    // Đúng một trong (connection+table) / format.
    let has_conn = e.connection.as_deref().is_some_and(|c| !c.trim().is_empty());
    let has_fmt = e.format.as_deref().is_some_and(|c| !c.trim().is_empty());
    if has_conn == has_fmt {
        errs.push(FieldError::new(
            id,
            "connection/format",
            "khai MỘT trong (connection+table) hoặc format",
        ));
    }
    if has_conn && e.table.as_deref().unwrap_or("").trim().is_empty() {
        errs.push(FieldError::new(id, "table", "bắt buộc khi export qua connection"));
    }
    if !EXPORT_MODES.contains(&e.mode.as_str()) {
        errs.push(FieldError::new(
            id,
            "mode",
            format!("mode export không hợp lệ '{}'; hợp lệ: {}", e.mode, EXPORT_MODES.join(", ")),
        ));
    }
    // Upsert (DB-load) cần khoá; và keys chỉ có nghĩa cho DB-load qua connection.
    if e.mode == "upsert" && e.keys.iter().all(|k| k.trim().is_empty()) {
        errs.push(FieldError::new(id, "keys", "mode=upsert cần 'keys' không rỗng"));
    }
}

fn is_valid_flow_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

// ---------------------------------------------------------------------------
// DAG — topo order + phát hiện chu trình
// ---------------------------------------------------------------------------

/// Thứ tự topo của mọi step (source→transform→export theo phụ thuộc). Cạnh:
/// transform phụ thuộc alias step trong FROM/JOIN của `sql`; export phụ thuộc
/// `input`. Chu trình → `Err(vec![FieldError])`.
pub fn derive_dag(def: &FlowDef) -> std::result::Result<Vec<String>, Vec<FieldError>> {
    let ids: Vec<String> = def
        .sources
        .iter()
        .map(|s| s.id.clone())
        .chain(def.transforms.iter().map(|t| t.id.clone()))
        .chain(def.exports.iter().map(|e| e.id.clone()))
        .collect();
    let known: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();

    // deps[x] = tập step x phụ thuộc.
    let mut deps: HashMap<String, HashSet<String>> = HashMap::new();
    for id in &ids {
        deps.entry(id.clone()).or_default();
    }
    for t in &def.transforms {
        let refs = referenced_ids(&t.sql, &known);
        let entry = deps.entry(t.id.clone()).or_default();
        for r in refs {
            if r != t.id {
                entry.insert(r);
            }
        }
    }
    for e in &def.exports {
        if known.contains(e.input.as_str()) && e.input != e.id {
            deps.entry(e.id.clone()).or_default().insert(e.input.clone());
        }
    }

    // Kahn: indegree = số phụ thuộc chưa giải quyết.
    let mut indeg: HashMap<String, usize> =
        ids.iter().map(|id| (id.clone(), deps[id].len())).collect();
    // dependents[d] = các step phụ thuộc d.
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    for (x, ds) in &deps {
        for d in ds {
            dependents.entry(d.clone()).or_default().push(x.clone());
        }
    }

    let mut queue: Vec<String> = ids.iter().filter(|id| indeg[*id] == 0).cloned().collect();
    queue.sort(); // ổn định
    let mut order = Vec::with_capacity(ids.len());
    while let Some(n) = queue.pop() {
        order.push(n.clone());
        if let Some(deps_of) = dependents.get(&n) {
            let mut newly = Vec::new();
            for d in deps_of {
                let e = indeg.get_mut(d).unwrap();
                *e -= 1;
                if *e == 0 {
                    newly.push(d.clone());
                }
            }
            newly.sort();
            queue.extend(newly);
        }
    }

    if order.len() == ids.len() {
        Ok(order)
    } else {
        let stuck: Vec<String> = ids.into_iter().filter(|id| !order.contains(id)).collect();
        Err(vec![FieldError::new(
            "",
            "dag",
            format!("phát hiện chu trình phụ thuộc giữa các step: {}", stuck.join(", ")),
        )])
    }
}

/// Rút các identifier xuất hiện ngay sau `FROM`/`JOIN` mà nằm trong `known` (alias
/// step). Ref namespaced (`ns.dataset`, chứa '.') bị bỏ — chỉ alias trần mới tính.
fn referenced_ids(sql: &str, known: &HashSet<&str>) -> HashSet<String> {
    let lower = sql.to_lowercase();
    let toks = tokenize(&lower);
    let mut out = HashSet::new();
    for i in 0..toks.len() {
        if (toks[i] == "from" || toks[i] == "join") && i + 1 < toks.len() {
            let cand = &toks[i + 1];
            if !cand.contains('.') && known.contains(cand.as_str()) {
                out.insert(cand.clone());
            }
        }
    }
    out
}

/// Token = maximal run của [a-z0-9_.]; ký tự khác là dấu tách.
fn tokenize(s: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
            cur.push(c);
        } else if !cur.is_empty() {
            toks.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    toks
}

// ---------------------------------------------------------------------------
// flow edit impact (§6.3)
// ---------------------------------------------------------------------------

/// Impact của một lần sửa flow (§6.3). `steps_reset` cần `confirm_reset`.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct FlowImpact {
    /// Step có thay đổi state-resetting (reset stream_state + step_interval).
    pub steps_reset: Vec<String>,
    /// Step giữ nguyên state (thay đổi state-compatible hoặc không đổi).
    pub steps_kept: Vec<String>,
    /// Dataset target của step cũ không còn ở def mới (owner ghi nhận, dữ liệu giữ).
    pub datasets_orphaned: Vec<String>,
}

/// Field source state-resetting (§6.3): đổi một trong các field này → reset state.
fn source_reset_fields(a: &SourceStep, b: &SourceStep) -> bool {
    a.connection != b.connection
        || a.table != b.table
        || a.query != b.query
        || a.mode != b.mode
        || a.cursor.as_ref().map(|c| &c.column) != b.cursor.as_ref().map(|c| &c.column)
        || a.primary_key != b.primary_key
        || a.merge_key != b.merge_key
        // partition_by + strategy quyết định layout merge (apply_source) → đổi phải reset
        // state, nếu không layout lệch âm thầm (BUG diff_impact bỏ sót partition_by).
        || a.target.as_ref().and_then(|t| t.partition_by.as_ref())
            != b.target.as_ref().and_then(|t| t.partition_by.as_ref())
        || a.strategy != b.strategy
}

/// Field transform state-resetting (§6.3): đổi `time_column`/`interval`/`kind`.
fn transform_reset_fields(a: &TransformStep, b: &TransformStep) -> bool {
    a.kind != b.kind || a.time_column != b.time_column || a.interval != b.interval
}

/// Phân loại tác động khi đổi từ `old` sang `new` (§6.3): step nào reset state, step nào
/// giữ, dataset nào mồ côi. Step mới (không có ở old) không tính reset (chưa có state).
pub fn diff_impact(old: &FlowDef, new: &FlowDef) -> FlowImpact {
    let mut out = FlowImpact::default();

    for ns in &new.sources {
        match old.sources.iter().find(|os| os.id == ns.id) {
            Some(os) if source_reset_fields(os, ns) => out.steps_reset.push(ns.id.clone()),
            Some(_) => out.steps_kept.push(ns.id.clone()),
            None => {}
        }
    }
    for nt in &new.transforms {
        match old.transforms.iter().find(|ot| ot.id == nt.id) {
            Some(ot) if transform_reset_fields(ot, nt) => out.steps_reset.push(nt.id.clone()),
            Some(_) => out.steps_kept.push(nt.id.clone()),
            None => {}
        }
    }

    // Dataset mồ côi: target của source/transform cũ không còn ở def mới.
    let new_src_ids: HashSet<&str> = new.sources.iter().map(|s| s.id.as_str()).collect();
    let new_tf_ids: HashSet<&str> = new.transforms.iter().map(|t| t.id.as_str()).collect();
    for os in &old.sources {
        if !new_src_ids.contains(os.id.as_str()) {
            let (ns, name) = source_target(os);
            out.datasets_orphaned.push(format!("{ns}.{name}"));
        }
    }
    for ot in &old.transforms {
        if !new_tf_ids.contains(ot.id.as_str()) {
            let (ns, name) = transform_target(ot);
            out.datasets_orphaned.push(format!("{ns}.{name}"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// lineage — cạnh dataset (§4)
// ---------------------------------------------------------------------------

/// Cạnh dataset `(parent, child)` suy từ một flow: target của transform phụ thuộc
/// dataset của các step tham chiếu trong FROM/JOIN của SQL (khớp `derive_dag`). Source
/// KHÔNG có dataset cha (đọc thẳng từ connection). Mỗi phần tử là `((ns,name),(ns,name))`.
pub fn dataset_edges(def: &FlowDef) -> Vec<((String, String), (String, String))> {
    let mut step_ds: HashMap<&str, (String, String)> = HashMap::new();
    for s in &def.sources {
        step_ds.insert(s.id.as_str(), source_target(s));
    }
    for t in &def.transforms {
        step_ds.insert(t.id.as_str(), transform_target(t));
    }
    let known: HashSet<&str> = step_ds.keys().copied().collect();
    let mut edges = Vec::new();
    for t in &def.transforms {
        let child = transform_target(t);
        for r in referenced_ids(&t.sql, &known) {
            if r == t.id {
                continue;
            }
            if let Some(parent) = step_ds.get(r.as_str()) {
                if *parent != child {
                    edges.push((parent.clone(), child.clone()));
                }
            }
        }
    }
    edges
}

// ---------------------------------------------------------------------------
// canonical JSON
// ---------------------------------------------------------------------------

/// Serialize về JSON canonical để lưu `flow.def`. Field theo thứ tự khai báo struct;
/// map con (schema_policy) key sắp xếp (serde_json BTreeMap, không preserve_order).
pub fn to_canonical_json(def: &FlowDef) -> Result<String> {
    serde_json::to_string(def).map_err(|e| anyhow!("serialize flow def thất bại: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn full_flow_json() -> String {
        json!({
            "version": 1,
            "flow": "shop",
            "sources": [{
                "id": "orders_raw",
                "connection": "pg_main",
                "table": "public.orders",
                "mode": "full_refresh",
                "target": {"namespace": "raw", "dataset": "orders_raw"}
            }]
        })
        .to_string()
    }

    #[test]
    fn parse_json_and_yaml_both_ok() {
        let f = parse(&full_flow_json()).unwrap();
        assert_eq!(f.flow, "shop");
        assert_eq!(f.sources.len(), 1);

        // YAML (không bắt đầu bằng '{') được chấp nhận, deserialize về cùng FlowDef.
        let yaml = "\
flow: shop
sources:
  - id: orders_raw
    connection: pg_main
    table: public.orders
    mode: full_refresh
transforms:
  - id: rev
    kind: incremental_by_time
    time_column: day
    interval: day
    lookback: 2
    sql: SELECT day FROM orders_raw WHERE day >= @start AND day < @end
";
        let fy = parse(yaml).unwrap();
        assert_eq!(fy.flow, "shop");
        assert_eq!(fy.sources.len(), 1);
        assert_eq!(fy.sources[0].mode, "full_refresh");
        assert_eq!(fy.transforms.len(), 1);
        assert_eq!(fy.transforms[0].kind, "incremental_by_time");
        assert_eq!(fy.transforms[0].interval.as_deref(), Some("day"));
        assert_eq!(fy.transforms[0].lookback, Some(2));
        assert!(validate(&fy).is_ok());

        // Rỗng vẫn lỗi.
        assert!(parse("   ").is_err());
    }

    #[test]
    fn validate_full_refresh_ok() {
        let f = parse(&full_flow_json()).unwrap();
        assert!(validate(&f).is_ok());
    }

    #[test]
    fn validate_incremental_requires_cursor() {
        let f: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [{
                "id": "s1", "connection": "c", "table": "t",
                "mode": "incremental_append"
            }]
        }))
        .unwrap();
        let errs = validate(&f).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "cursor" && e.step_id == "s1"));

        // Có cursor đủ column+initial → ok.
        let f2: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [{
                "id": "s1", "connection": "c", "table": "t",
                "mode": "incremental_append",
                "cursor": {"column": "updated_at", "initial": "2024-01-01"}
            }]
        }))
        .unwrap();
        assert!(validate(&f2).is_ok());
    }

    #[test]
    fn validate_reports_multiple_errors() {
        // flow id sai, source thiếu connection + cả table lẫn query, mode lạ.
        let f: FlowDef = serde_json::from_value(json!({
            "flow": "SHOP!",
            "sources": [{ "id": "s1", "mode": "weird" }]
        }))
        .unwrap();
        let errs = validate(&f).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "flow"));
        assert!(errs.iter().any(|e| e.field == "connection"));
        assert!(errs.iter().any(|e| e.field == "table/query"));
        assert!(errs.iter().any(|e| e.field == "mode"));
    }

    #[test]
    fn validate_merge_ok_and_constraints() {
        // merge hợp lệ: merge_key ⊆ partition_by.
        let f: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [{
                "id": "s1", "connection": "c", "table": "t",
                "mode": "incremental_merge",
                "cursor": {"column": "u", "initial": "x"},
                "primary_key": ["id"],
                "merge_key": ["d"],
                "target": {"namespace": "raw", "dataset": "s1", "partition_by": ["d"]}
            }]
        }))
        .unwrap();
        assert!(validate(&f).is_ok(), "merge hợp lệ phải qua ở Phase 3");

        // merge thiếu partition_by (và không allow_full_rewrite) → lỗi.
        let bad: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [{
                "id": "s1", "connection": "c", "table": "t",
                "mode": "incremental_merge",
                "cursor": {"column": "u", "initial": "x"},
                "primary_key": ["id"], "merge_key": ["d"]
            }]
        }))
        .unwrap();
        let errs = validate(&bad).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "target.partition_by"));

        // merge_key ∉ partition_by → lỗi.
        let bad2: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [{
                "id": "s1", "connection": "c", "table": "t",
                "mode": "incremental_merge",
                "cursor": {"column": "u", "initial": "x"},
                "primary_key": ["id"], "merge_key": ["x"],
                "target": {"namespace": "raw", "dataset": "s1", "partition_by": ["d"]}
            }]
        }))
        .unwrap();
        assert!(validate(&bad2).unwrap_err().iter().any(|e| e.field == "merge_key"));

        // insert_only không cần merge_key nhưng vẫn cần partition_by.
        let io: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [{
                "id": "s1", "connection": "c", "table": "t",
                "mode": "incremental_merge", "strategy": "insert_only",
                "cursor": {"column": "u", "initial": "x"},
                "primary_key": ["id"],
                "target": {"namespace": "raw", "dataset": "s1", "partition_by": ["d"]}
            }]
        }))
        .unwrap();
        assert!(validate(&io).is_ok());
    }

    #[test]
    fn validate_snapshot_timestamp_needs_cursor() {
        // timestamp thiếu cursor.column → lỗi.
        let bad: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [{
                "id": "s1", "connection": "c", "table": "t",
                "mode": "snapshot", "strategy": "timestamp",
                "primary_key": ["id"]
            }]
        }))
        .unwrap();
        assert!(validate(&bad).unwrap_err().iter().any(|e| e.field == "cursor.column"));

        // check strategy không cần cursor → ok.
        let ok: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [{
                "id": "s1", "connection": "c", "table": "t",
                "mode": "snapshot", "strategy": "check",
                "primary_key": ["id"]
            }]
        }))
        .unwrap();
        assert!(validate(&ok).is_ok());
    }

    #[test]
    fn diff_impact_classifies_reset_kept_orphan() {
        let old: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [
                {"id": "a", "connection": "c", "table": "t1", "mode": "full_refresh"},
                {"id": "b", "connection": "c", "table": "t2", "mode": "incremental_append",
                 "cursor": {"column": "u", "initial": 0}},
                {"id": "gone", "connection": "c", "table": "t3", "mode": "full_refresh"}
            ]
        }))
        .unwrap();
        let new: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [
                {"id": "a", "connection": "c", "table": "t1", "mode": "full_refresh"},
                {"id": "b", "connection": "c", "table": "t2", "mode": "incremental_append",
                 "cursor": {"column": "changed", "initial": 0}}
            ]
        }))
        .unwrap();
        let imp = diff_impact(&old, &new);
        assert_eq!(imp.steps_reset, vec!["b".to_string()], "đổi cursor.column → reset");
        assert_eq!(imp.steps_kept, vec!["a".to_string()], "a không đổi → kept");
        assert_eq!(imp.datasets_orphaned, vec!["raw.gone".to_string()]);
    }

    #[test]
    fn diff_impact_partition_by_change_resets() {
        // BUG: đổi target.partition_by KHÔNG bị coi state-resetting → layout merge lệch.
        // CHỈ đổi partition_by — mọi field khác (kể cả merge_key) giữ nguyên, để cô lập
        // đúng nhánh partition_by (nếu đổi merge_key thì reset đã kích hoạt sẵn).
        let mk = |parts: Value| -> FlowDef {
            serde_json::from_value(json!({
                "flow": "shop",
                "sources": [{
                    "id": "m", "connection": "c", "table": "t", "mode": "incremental_merge",
                    "strategy": "delete_insert", "merge_key": ["k"],
                    "target": {"namespace": "raw", "dataset": "m", "partition_by": parts}
                }]
            }))
            .unwrap()
        };
        let old = mk(json!(["d"]));
        let new = mk(json!(["d", "region"]));
        let imp = diff_impact(&old, &new);
        assert_eq!(imp.steps_reset, vec!["m".to_string()], "đổi partition_by → reset");
        assert!(imp.steps_kept.is_empty(), "không được coi là kept");
    }

    #[test]
    fn source_target_defaults_to_raw_id() {
        let s: SourceStep = serde_json::from_value(json!({
            "id": "orders", "connection": "c", "table": "t", "mode": "full_refresh"
        }))
        .unwrap();
        assert_eq!(source_target(&s), ("raw".to_string(), "orders".to_string()));
    }

    #[test]
    fn dag_topo_order_sources_before_transform_before_export() {
        let f: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "sources": [{
                "id": "orders", "connection": "c", "table": "t", "mode": "full_refresh"
            }],
            "transforms": [{
                "id": "rev", "kind": "full",
                "sql": "SELECT * FROM orders"
            }],
            "exports": [{
                "id": "out", "input": "rev", "format": "csv", "mode": "full_refresh"
            }]
        }))
        .unwrap();
        let order = derive_dag(&f).unwrap();
        let pos = |id: &str| order.iter().position(|x| x == id).unwrap();
        assert!(pos("orders") < pos("rev"));
        assert!(pos("rev") < pos("out"));
    }

    #[test]
    fn dag_detects_cycle() {
        // t1 SELECT FROM t2, t2 SELECT FROM t1 → chu trình.
        let f: FlowDef = serde_json::from_value(json!({
            "flow": "shop",
            "transforms": [
                {"id": "t1", "kind": "full", "sql": "SELECT * FROM t2"},
                {"id": "t2", "kind": "full", "sql": "SELECT * FROM t1"}
            ]
        }))
        .unwrap();
        let errs = derive_dag(&f).unwrap_err();
        assert!(errs.iter().any(|e| e.field == "dag" && e.message.contains("chu trình")));
    }

    #[test]
    fn canonical_json_roundtrips() {
        let f = parse(&full_flow_json()).unwrap();
        let canon = to_canonical_json(&f).unwrap();
        let back = parse(&canon).unwrap();
        assert_eq!(f, back);
    }
}
