//! Truy vết deterministic — parse ID từ markdown bằng regex (không AI),
//! suy coverage FR↔US↔AC↔UC↔TC, pipeline 8 chặng, staleness theo đồ thị
//! upstream của templates, và dashboard tổng hợp. Đây là bản Rust của
//! "engine deterministic" trong BA-Kit (docs/ba-app-design.md §5).

use crate::db::Db;
use crate::templates;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

pub struct IdEntry {
    pub kind: String,
    pub ident: String,
    /// 'def' — định nghĩa tại doc này; 'ref' — chỉ nhắc tới.
    pub role: String,
    /// ID của mục chứa nó (dòng bảng / heading gần nhất) nếu xác định được.
    pub from_ident: String,
    /// Chỉ có nghĩa với OQ: dòng chứa trạng thái resolved/đã chốt.
    pub resolved: bool,
}

fn id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // FR-<slug>-001 ... ; OQ-1 ; CR-20260802-001. Slug bắt buộc + số cuối
        // nên "E-mail" hay "US-East" không match.
        Regex::new(
            r"\b(?:(FR|NFR|BR|SC|US|AC|TC|UC|UR|PER|E)-([a-z0-9][a-z0-9-]*?)-(\d{2,3})|(OQ)-(\d{1,3})|(CR)-(\d{8})-(\d{3}))\b",
        )
        .unwrap()
    })
}

/// Loại ID mà mỗi doc_type ĐỊNH NGHĨA (mọi loại khác chỉ là ref).
fn def_kinds(doc_type: &str) -> &'static [&'static str] {
    match doc_type {
        "srs" | "reverse_doc" => &["FR", "NFR", "BR", "E", "SC"],
        "urd" => &["UR", "PER"],
        "prd" => &["PER"],
        "prd_epic" => &["SC"],
        "userstory" => &["US"],
        "ac" => &["AC"],
        "usecase" => &["UC"],
        "test_cases" => &["TC"],
        _ => &[],
    }
}

struct FoundId {
    kind: String,
    ident: String,
}

fn find_ids(line: &str) -> Vec<FoundId> {
    id_regex()
        .captures_iter(line)
        .map(|c| {
            if let Some(k) = c.get(1) {
                FoundId {
                    kind: k.as_str().to_string(),
                    ident: c.get(0).unwrap().as_str().to_string(),
                }
            } else if c.get(4).is_some() {
                FoundId {
                    kind: "OQ".into(),
                    ident: c.get(0).unwrap().as_str().to_string(),
                }
            } else {
                FoundId {
                    kind: "CR".into(),
                    ident: c.get(0).unwrap().as_str().to_string(),
                }
            }
        })
        .collect()
}

fn line_marks_resolved(line: &str) -> bool {
    let l = line.to_lowercase();
    l.contains("resolved") || l.contains("đã chốt") || l.contains("da chot")
}

/// Quét toàn bộ nội dung markdown của một doc, trả entries để ghi vào doc_ids.
pub fn parse_ids(doc_type: &str, content: &str) -> Vec<IdEntry> {
    let defs = def_kinds(doc_type);
    let mut out: Vec<IdEntry> = Vec::new();
    let mut anchor = String::new();
    // Dedup theo (ident, role, from) — một FR nhắc 20 lần không thành 20 ref.
    let mut seen: HashSet<(String, String, String)> = HashSet::new();
    let mut push = |out: &mut Vec<IdEntry>, kind: &str, ident: &str, role: &str, from: &str, resolved: bool| {
        let key = (ident.to_string(), role.to_string(), from.to_string());
        if seen.insert(key) {
            out.push(IdEntry {
                kind: kind.to_string(),
                ident: ident.to_string(),
                role: role.to_string(),
                from_ident: from.to_string(),
                resolved,
            });
        }
    };
    let mut in_code_fence = false;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            continue;
        }
        // Trong code fence (mermaid, script...) ID vẫn đáng ghi nhận là ref
        // (flow ghi 'FR-022' trong nhãn), nhưng không bao giờ là def.
        let ids = find_ids(line);
        if ids.is_empty() {
            continue;
        }
        let resolved = line_marks_resolved(line);
        let is_row = trimmed.starts_with('|');
        let is_heading = trimmed.starts_with('#');
        if is_row && !in_code_fence {
            // Dòng phân cách bảng |---| không chứa ID nên không tới đây.
            let mut def_ident: Option<String> = None;
            for (i, f) in ids.iter().enumerate() {
                let is_def = i == 0 && defs.contains(&f.kind.as_str());
                if is_def {
                    def_ident = Some(f.ident.clone());
                    push(&mut out, &f.kind, &f.ident, "def", &anchor, resolved);
                } else if f.kind == "OQ" {
                    push(&mut out, &f.kind, &f.ident, "def", "", resolved);
                } else {
                    let from = def_ident.clone().unwrap_or_else(|| anchor.clone());
                    push(&mut out, &f.kind, &f.ident, "ref", &from, resolved);
                }
            }
        } else if is_heading && !in_code_fence {
            let first = &ids[0];
            if defs.contains(&first.kind.as_str()) {
                push(&mut out, &first.kind, &first.ident, "def", &anchor, resolved);
            } else {
                push(&mut out, &first.kind, &first.ident, "ref", "", resolved);
            }
            // Heading là mỏ neo cho các dòng sau (vd `### US-x-001` trong doc AC,
            // `### UC-x-001` trong usecase, `### Flow` liệt kê FR).
            anchor = first.ident.clone();
            for f in &ids[1..] {
                push(&mut out, &f.kind, &f.ident, "ref", &first.ident, resolved);
            }
        } else {
            for f in &ids {
                if f.kind == "OQ" {
                    push(&mut out, &f.kind, &f.ident, "def", "", resolved);
                } else {
                    push(&mut out, &f.kind, &f.ident, "ref", &anchor, resolved);
                }
            }
        }
    }
    out
}

/// Đánh lại chỉ mục ID cho một doc (gọi sau mọi lần content đổi).
pub fn reindex_document(db: &Db, document_id: i64) {
    if let Some(doc) = db.get_document(document_id) {
        let doc_type = doc["doc_type"].as_str().unwrap_or("");
        // HTML (wireframe/prototype) vẫn quét được — ID nằm trong text/bảng mô tả.
        let content = doc["content"].as_str().unwrap_or("");
        let entries = parse_ids(doc_type, content);
        let _ = db.reindex_doc_ids(document_id, &entries);
    }
}

const TEST_DOC_TYPES: [&str; 3] = ["test_cases", "test_checklist", "playwright"];

/// Coverage cho một feature — thuần từ chỉ mục doc_ids.
pub fn coverage(db: &Db, project_id: i64, feature_id: i64) -> Value {
    let docs = db.docs_with_content(project_id, Some(feature_id));
    let ids: Vec<i64> = docs.iter().filter_map(|d| d["id"].as_i64()).collect();
    let doc_type_of: HashMap<i64, String> = docs
        .iter()
        .map(|d| {
            (
                d["id"].as_i64().unwrap_or(0),
                d["doc_type"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect();
    let entries = db.doc_ids_for_docs(&ids);

    let mut fr_defs: HashSet<String> = HashSet::new();
    let mut us_defs: HashSet<String> = HashSet::new();
    let mut uc_defs: HashSet<String> = HashSet::new();
    let mut ac_by_us: HashMap<String, i64> = HashMap::new();
    let mut us_to_frs: HashMap<String, HashSet<String>> = HashMap::new();
    let mut test_refs: HashSet<String> = HashSet::new();
    let mut oq_open = 0i64;
    let mut oq_total = 0i64;

    for (_doc_id, doc_type, kind, ident, role, from, resolved) in &entries {
        match (kind.as_str(), role.as_str()) {
            ("FR", "def") => {
                fr_defs.insert(ident.clone());
            }
            ("US", "def") => {
                us_defs.insert(ident.clone());
            }
            ("UC", "def") => {
                uc_defs.insert(ident.clone());
            }
            ("AC", "def") => {
                if from.starts_with("US-") {
                    *ac_by_us.entry(from.clone()).or_insert(0) += 1;
                }
            }
            ("OQ", "def") => {
                oq_total += 1;
                if !resolved {
                    oq_open += 1;
                }
            }
            ("FR", "ref") if doc_type == "userstory" && from.starts_with("US-") => {
                us_to_frs.entry(from.clone()).or_default().insert(ident.clone());
            }
            _ => {}
        }
        if role == "ref" && TEST_DOC_TYPES.contains(&doc_type.as_str()) {
            test_refs.insert(ident.clone());
        }
    }
    let _ = doc_type_of;

    let fr_covered: HashSet<String> = us_to_frs
        .values()
        .flat_map(|s| s.iter().cloned())
        .filter(|fr| fr_defs.contains(fr))
        .collect();
    let fr_uncovered: Vec<String> = {
        let mut v: Vec<String> = fr_defs.difference(&fr_covered).cloned().collect();
        v.sort();
        v
    };
    let us_orphans: Vec<String> = {
        let mut v: Vec<String> = us_defs
            .iter()
            .filter(|us| us_to_frs.get(*us).map(|s| s.is_empty()).unwrap_or(true))
            .cloned()
            .collect();
        v.sort();
        v
    };
    let us_without_ac: Vec<String> = {
        let mut v: Vec<String> = us_defs
            .iter()
            .filter(|us| !ac_by_us.contains_key(*us))
            .cloned()
            .collect();
        v.sort();
        v
    };
    // FR có test khi: được nhắc trực tiếp trong doc test, HOẶC một US phủ nó
    // được nhắc trong doc test.
    let fr_without_test: Vec<String> = {
        let mut v: Vec<String> = fr_defs
            .iter()
            .filter(|fr| {
                if test_refs.contains(*fr) {
                    return false;
                }
                let via_us = us_to_frs
                    .iter()
                    .any(|(us, frs)| frs.contains(*fr) && test_refs.contains(us));
                !via_us
            })
            .cloned()
            .collect();
        v.sort();
        v
    };
    let uc_without_test: Vec<String> = {
        let mut v: Vec<String> = uc_defs
            .iter()
            .filter(|uc| !test_refs.contains(*uc))
            .cloned()
            .collect();
        v.sort();
        v
    };
    let coverage_pct = if fr_defs.is_empty() {
        Value::Null
    } else {
        json!(((fr_covered.len() as f64 / fr_defs.len() as f64) * 100.0).round())
    };
    json!({
        "fr_total": fr_defs.len(),
        "fr_covered": fr_covered.len(),
        "coverage_pct": coverage_pct,
        "fr_uncovered": fr_uncovered,
        "fr_without_test": fr_without_test,
        "us_total": us_defs.len(),
        "us_orphans": us_orphans,
        "us_without_ac": us_without_ac,
        "uc_total": uc_defs.len(),
        "uc_without_test": uc_without_test,
        "oq_total": oq_total,
        "oq_open": oq_open,
    })
}

/// 8 chặng pipeline — chặng đạt khi feature có doc loại đó với nội dung thật.
pub fn pipeline(db: &Db, project_id: i64, feature_id: i64) -> Value {
    let docs = db.docs_with_content(project_id, Some(feature_id));
    let stages: Vec<Value> = templates::PIPELINE
        .iter()
        .map(|stage| {
            let done = docs.iter().any(|d| {
                d["doc_type"].as_str() == Some(stage)
                    && d["content"].as_str().map(|c| c.chars().count() > 50).unwrap_or(false)
            });
            json!({ "stage": stage, "done": done })
        })
        .collect();
    let done_n = stages.iter().filter(|s| s["done"] == json!(true)).count();
    json!({
        "stages": stages,
        "done": done_n,
        "total": templates::PIPELINE.len(),
        "pct": ((done_n as f64 / templates::PIPELINE.len() as f64) * 100.0).round(),
    })
}

const DAY_MS: i64 = 86_400_000;

/// Staleness: doc cũ hơn upstream cùng feature (hoặc upstream cấp project).
/// Trả (per-doc freshness + cạnh stale chain). Điểm: tươi = 100, vừa stale =
/// 60, -10 mỗi ngày stale, sàn 20 — khớp thang "doc 20đ" của dashboard mẫu.
pub fn staleness(db: &Db, project_id: i64, feature_id: Option<i64>) -> Value {
    let feature_docs = match feature_id {
        Some(fid) => db.docs_with_content(project_id, Some(fid)),
        None => vec![],
    };
    let project_docs = db.docs_with_content(project_id, None);
    let now = crate::db::now_ms();

    let updated_of = |doc_type: &str| -> Option<(i64, String)> {
        // upstream ưu tiên doc cùng feature; loại cấp project tra ở project_docs.
        feature_docs
            .iter()
            .chain(project_docs.iter())
            .filter(|d| d["doc_type"].as_str() == Some(doc_type))
            .map(|d| {
                (
                    d["updated_at"].as_i64().unwrap_or(0),
                    d["title"].as_str().unwrap_or("").to_string(),
                )
            })
            .max_by_key(|(t, _)| *t)
    };

    let mut items: Vec<Value> = Vec::new();
    let mut chain: Vec<Value> = Vec::new();
    for d in feature_docs.iter().chain(project_docs.iter()) {
        let doc_type = d["doc_type"].as_str().unwrap_or("");
        let subtype = d["subtype"].as_str().unwrap_or("");
        let Some(tpl) = templates::get(doc_type, subtype) else {
            continue;
        };
        let my_updated = d["updated_at"].as_i64().unwrap_or(0);
        let mut stale_since: Option<(i64, String)> = None;
        for up in tpl.upstream {
            if let Some((up_time, up_title)) = updated_of(up) {
                if up_time > my_updated {
                    let cand = (up_time, up_title);
                    if stale_since.as_ref().map(|(t, _)| cand.0 > *t).unwrap_or(true) {
                        stale_since = Some(cand);
                    }
                }
            }
        }
        let (score, stale, upstream_title) = match &stale_since {
            Some((since, title)) => {
                let days = ((now - since) / DAY_MS).max(0);
                ((60 - 10 * days).max(20), true, title.clone())
            }
            None => (100, false, String::new()),
        };
        if stale {
            chain.push(json!({
                "upstream": upstream_title,
                "doc_id": d["id"],
                "doc_title": d["title"],
                "doc_type": doc_type,
            }));
        }
        items.push(json!({
            "doc_id": d["id"],
            "title": d["title"],
            "doc_type": doc_type,
            "subtype": subtype,
            "status": d["status"],
            "score": score,
            "stale": stale,
        }));
    }
    let avg = if items.is_empty() {
        100
    } else {
        items.iter().map(|i| i["score"].as_i64().unwrap_or(100)).sum::<i64>() / items.len() as i64
    };
    json!({ "avg": avg, "docs": items, "chain": chain })
}

/// Dashboard project — 4 KPI + việc gấp + kanban + tiến độ từng feature,
/// đúng bố cục dashboard mẫu của BA-Kit.
pub fn dashboard(db: &Db, project_id: i64) -> Value {
    let features = db.list_features(project_id);
    let now = crate::db::now_ms();

    let mut per_feature: Vec<Value> = Vec::new();
    let mut cov_sum = 0f64;
    let mut cov_n = 0;
    let mut pipe_sum = 0f64;
    let mut oq_open_total = 0i64;
    for f in &features {
        let fid = f["id"].as_i64().unwrap_or(0);
        let cov = coverage(db, project_id, fid);
        let pipe = pipeline(db, project_id, fid);
        if let Some(p) = cov["coverage_pct"].as_f64() {
            cov_sum += p;
            cov_n += 1;
        }
        oq_open_total += cov["oq_open"].as_i64().unwrap_or(0);
        pipe_sum += pipe["pct"].as_f64().unwrap_or(0.0);
        per_feature.push(json!({
            "feature": f,
            "coverage": cov,
            "pipeline": pipe,
        }));
    }
    let stale_all = {
        // staleness từng feature + doc cấp project (gộp, khử trùng doc cấp
        // project bằng cách chỉ lấy từ lần gọi đầu).
        let mut docs: Vec<Value> = Vec::new();
        let mut chain: Vec<Value> = Vec::new();
        let mut seen_docs: HashSet<i64> = HashSet::new();
        let mut push_from = |v: Value, docs: &mut Vec<Value>, chain: &mut Vec<Value>| {
            for d in v["docs"].as_array().cloned().unwrap_or_default() {
                let id = d["doc_id"].as_i64().unwrap_or(0);
                if seen_docs.insert(id) {
                    docs.push(d);
                }
            }
            for c in v["chain"].as_array().cloned().unwrap_or_default() {
                chain.push(c);
            }
        };
        for f in &features {
            let fid = f["id"].as_i64().unwrap_or(0);
            push_from(staleness(db, project_id, Some(fid)), &mut docs, &mut chain);
        }
        if features.is_empty() {
            push_from(staleness(db, project_id, None), &mut docs, &mut chain);
        }
        let avg = if docs.is_empty() {
            100
        } else {
            docs.iter().map(|i| i["score"].as_i64().unwrap_or(100)).sum::<i64>() / docs.len() as i64
        };
        json!({ "avg": avg, "docs": docs, "chain": chain })
    };

    // Kanban theo lifecycle.
    let all_docs = db.list_documents(project_id, None, None);
    let mut kanban: HashMap<&str, Vec<Value>> = HashMap::new();
    for st in templates::DOC_STATUSES {
        kanban.insert(st, vec![]);
    }
    let mut review_overdue: Vec<Value> = Vec::new();
    for d in &all_docs {
        let st = d["status"].as_str().unwrap_or("draft");
        let entry = json!({
            "id": d["id"], "title": d["title"], "doc_type": d["doc_type"],
            "subtype": d["subtype"], "feature_id": d["feature_id"], "updated_at": d["updated_at"],
        });
        kanban.entry(st).or_default().push(entry.clone());
        if st == "in_review" {
            let days = (now - d["updated_at"].as_i64().unwrap_or(now)) / DAY_MS;
            if days > 7 {
                review_overdue.push(json!({ "doc": entry, "days": days }));
            }
        }
    }

    // Việc gấp: CR treo, doc stale nặng, OQ tồn, review quá hạn.
    let mut urgent: Vec<Value> = Vec::new();
    for (code, status, severity, created_at, pending) in db.open_crs(project_id) {
        if pending > 0 {
            let days = (now - created_at) / DAY_MS;
            urgent.push(json!({
                "level": if severity == "high" { "P0" } else { "P1" },
                "text": format!("CR {status} đã {days} ngày còn {pending} tài liệu chưa đồng bộ: {code}"),
                "kind": "cr", "ref": code,
            }));
        }
    }
    for d in stale_all["docs"].as_array().cloned().unwrap_or_default() {
        if d["stale"] == json!(true) && d["score"].as_i64().unwrap_or(100) < 40 {
            urgent.push(json!({
                "level": "P1",
                "text": format!("{} ({}đ) stale — upstream đã đổi, cần rà lại", d["title"].as_str().unwrap_or(""), d["score"]),
                "kind": "stale", "ref": d["doc_id"],
            }));
        }
    }
    for r in &review_overdue {
        urgent.push(json!({
            "level": "P1",
            "text": format!("Review quá hạn {} ngày: {}", r["days"], r["doc"]["title"].as_str().unwrap_or("")),
            "kind": "review", "ref": r["doc"]["id"],
        }));
    }
    if oq_open_total > 0 {
        urgent.push(json!({
            "level": "P2",
            "text": format!("{oq_open_total} Open Question chưa chốt trong các tài liệu"),
            "kind": "oq", "ref": Value::Null,
        }));
    }

    let coverage_avg: Value = if cov_n > 0 {
        json!((cov_sum / cov_n as f64).round())
    } else {
        Value::Null
    };
    let pipeline_avg = if features.is_empty() {
        0.0
    } else {
        pipe_sum / features.len() as f64
    };
    let crs = db.list_crs(project_id);
    json!({
        "kpi": {
            "coverage": coverage_avg,
            "pipeline": pipeline_avg.round(),
            "freshness": stale_all["avg"],
            "urgent": urgent.len(),
        },
        "urgent": urgent,
        "features": per_feature,
        "kanban": {
            "draft": kanban.get("draft").cloned().unwrap_or_default(),
            "in_review": kanban.get("in_review").cloned().unwrap_or_default(),
            "revisions": kanban.get("revisions").cloned().unwrap_or_default(),
            "approved": kanban.get("approved").cloned().unwrap_or_default(),
            "shipped": kanban.get("shipped").cloned().unwrap_or_default(),
        },
        "stale_chain": stale_all["chain"],
        "stale_docs": stale_all["docs"],
        "crs": crs,
        "oq_open": oq_open_total,
    })
}

fn mermaid_label(s: &str, max: usize) -> String {
    let cut: String = s.chars().take(max).collect();
    cut.replace('"', "'").replace('[', "(").replace(']', ")")
}

/// Knowledge Graph liên kết tài liệu (bản /kg của BA-Kit, deterministic):
/// node = tài liệu; cạnh `upstream` = quan hệ template (doc sau đọc doc trước
/// khi sinh); cạnh `ref` = doc này nhắc ID mà doc kia định nghĩa (đếm số ID).
/// Kèm chuỗi mermaid `graph LR` nhóm theo tính năng để UI render.
pub fn knowledge_graph(db: &Db, project_id: i64) -> Value {
    let features = db.list_features(project_id);
    // Gom node: doc cấp project + doc từng feature.
    let mut nodes: Vec<Value> = Vec::new();
    let mut feature_of_doc: HashMap<i64, i64> = HashMap::new(); // doc -> feature (0 = project)
    let mut collect = |docs: Vec<Value>, fid: i64, nodes: &mut Vec<Value>, map: &mut HashMap<i64, i64>| {
        for d in docs {
            let id = d["id"].as_i64().unwrap_or(0);
            map.insert(id, fid);
            nodes.push(json!({
                "id": id,
                "title": d["title"],
                "doc_type": d["doc_type"],
                "subtype": d["subtype"],
                "status": d["status"],
                "feature_id": if fid == 0 { Value::Null } else { json!(fid) },
                "updated_at": d["updated_at"],
            }));
        }
    };
    collect(db.list_documents(project_id, Some(None), None), 0, &mut nodes, &mut feature_of_doc);
    for f in &features {
        let fid = f["id"].as_i64().unwrap_or(0);
        collect(db.list_documents(project_id, Some(Some(fid)), None), fid, &mut nodes, &mut feature_of_doc);
    }

    let all_ids: Vec<i64> = nodes.iter().filter_map(|n| n["id"].as_i64()).collect();
    let entries = db.doc_ids_for_docs(&all_ids);

    // Cạnh ref: ident định nghĩa ở doc A, nhắc ở doc B (A≠B) → B -ref-> A.
    let mut def_of: HashMap<String, i64> = HashMap::new();
    for (doc_id, _dt, _kind, ident, role, _from, _res) in &entries {
        if role == "def" {
            def_of.entry(ident.clone()).or_insert(*doc_id);
        }
    }
    let mut ref_count: HashMap<(i64, i64), i64> = HashMap::new();
    for (doc_id, _dt, _kind, ident, role, _from, _res) in &entries {
        if role == "ref" {
            if let Some(&def_doc) = def_of.get(ident) {
                if def_doc != *doc_id {
                    *ref_count.entry((*doc_id, def_doc)).or_insert(0) += 1;
                }
            }
        }
    }

    // Cạnh upstream theo template (giữa doc thật, cùng feature — doc cấp
    // project là upstream chung).
    let type_of: HashMap<i64, (String, String)> = nodes
        .iter()
        .map(|n| {
            (
                n["id"].as_i64().unwrap_or(0),
                (
                    n["doc_type"].as_str().unwrap_or("").to_string(),
                    n["subtype"].as_str().unwrap_or("").to_string(),
                ),
            )
        })
        .collect();
    let mut upstream_edges: Vec<(i64, i64)> = Vec::new();
    for n in &nodes {
        let id = n["id"].as_i64().unwrap_or(0);
        let (dt, st) = type_of.get(&id).cloned().unwrap_or_default();
        let Some(tpl) = templates::get(&dt, &st) else { continue };
        let my_feature = feature_of_doc.get(&id).copied().unwrap_or(0);
        for up in tpl.upstream {
            // Ưu tiên doc cùng feature; không có thì doc cấp project.
            let cand = nodes
                .iter()
                .filter(|m| m["doc_type"].as_str() == Some(*up))
                .filter_map(|m| m["id"].as_i64())
                .find(|mid| feature_of_doc.get(mid).copied().unwrap_or(0) == my_feature)
                .or_else(|| {
                    nodes
                        .iter()
                        .filter(|m| m["doc_type"].as_str() == Some(*up))
                        .filter_map(|m| m["id"].as_i64())
                        .find(|mid| feature_of_doc.get(mid).copied().unwrap_or(0) == 0)
                });
            if let Some(up_id) = cand {
                if up_id != id {
                    upstream_edges.push((up_id, id));
                }
            }
        }
    }

    let mut edges: Vec<Value> = Vec::new();
    for (from, to) in &upstream_edges {
        edges.push(json!({ "from": from, "to": to, "kind": "upstream", "count": 1 }));
    }
    let mut ref_sorted: Vec<((i64, i64), i64)> = ref_count.into_iter().collect();
    ref_sorted.sort();
    for ((from, to), n) in &ref_sorted {
        edges.push(json!({ "from": from, "to": to, "kind": "ref", "count": n }));
    }

    // Mermaid graph — nhóm subgraph theo feature; quá to thì bỏ mermaid.
    let mermaid = if nodes.is_empty() {
        String::new()
    } else if nodes.len() > 80 {
        String::new()
    } else {
        let mut m = String::from("graph LR\n");
        let node_line = |n: &Value| -> String {
            format!(
                "    d{}[\"{}\"]\n",
                n["id"],
                mermaid_label(
                    &format!(
                        "{}{}",
                        n["doc_type"].as_str().unwrap_or(""),
                        n["subtype"].as_str().map(|s| if s.is_empty() { String::new() } else { format!("/{s}") }).unwrap_or_default()
                    ),
                    28
                )
            )
        };
        let proj_nodes: Vec<&Value> = nodes.iter().filter(|n| n["feature_id"].is_null()).collect();
        if !proj_nodes.is_empty() {
            m.push_str("  subgraph P[\"Cấp dự án\"]\n");
            for n in proj_nodes {
                m.push_str(&node_line(n));
            }
            m.push_str("  end\n");
        }
        for f in &features {
            let fid = f["id"].as_i64().unwrap_or(0);
            let f_nodes: Vec<&Value> = nodes
                .iter()
                .filter(|n| n["feature_id"].as_i64() == Some(fid))
                .collect();
            if f_nodes.is_empty() {
                continue;
            }
            m.push_str(&format!(
                "  subgraph F{}[\"{}\"]\n",
                fid,
                mermaid_label(f["name"].as_str().unwrap_or(""), 24)
            ));
            for n in f_nodes {
                m.push_str(&node_line(n));
            }
            m.push_str("  end\n");
        }
        for (from, to) in &upstream_edges {
            m.push_str(&format!("  d{from} -.-> d{to}\n"));
        }
        for ((from, to), n) in &ref_sorted {
            m.push_str(&format!("  d{from} -->|{n} id| d{to}\n"));
        }
        m
    };

    json!({
        "nodes": nodes,
        "edges": edges,
        "mermaid": mermaid,
        "note": if nodes.len() > 80 { "đồ thị > 80 tài liệu — bỏ bản vẽ mermaid, dùng bảng edges" } else { "" },
    })
}

/// Bóc bảng "Danh sách tính năng" từ PRD → (slug, name, description, priority).
/// Dòng bảng dạng `| slug | Tên | Mô tả | P0 |` — nhận cả khi cột slug là tên
/// (tự slugify).
pub fn parse_prd_features(content: &str) -> Vec<(String, String, String, String)> {
    let mut out = Vec::new();
    let mut in_features_section = false;
    let prio_re = Regex::new(r"\b(P0|P1|P2)\b").unwrap();
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("##") {
            let low = t.to_lowercase();
            in_features_section = low.contains("danh sách tính năng") || low.contains("tính năng");
            continue;
        }
        if !in_features_section || !t.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(|c| c.trim()).collect();
        if cells.len() < 3 {
            continue;
        }
        // Bỏ dòng header/tách cột.
        if cells[0].to_lowercase().contains("slug")
            || cells[0].to_lowercase().contains("tên")
            || cells[0].chars().all(|c| c == '-' || c == ':' || c.is_whitespace())
        {
            continue;
        }
        let slug = crate::db::slugify(cells[0]);
        let name = cells.get(1).unwrap_or(&"").to_string();
        let desc = cells.get(2).unwrap_or(&"").to_string();
        let prio = cells
            .iter()
            .find_map(|c| prio_re.find(c).map(|m| m.as_str().to_string()))
            .unwrap_or_else(|| "P1".to_string());
        if slug.is_empty() || name.is_empty() {
            continue;
        }
        out.push((slug, name, desc, prio));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_matches_conventions_not_prose() {
        let ids = find_ids("FR-authentication-001 và NFR-auth-002, E-auth-010, OQ-3, CR-20260802-001");
        let idents: Vec<&str> = ids.iter().map(|i| i.ident.as_str()).collect();
        assert_eq!(
            idents,
            vec!["FR-authentication-001", "NFR-auth-002", "E-auth-010", "OQ-3", "CR-20260802-001"]
        );
        assert!(find_ids("Gửi E-mail cho US-East region").is_empty());
    }

    #[test]
    fn srs_defines_fr_userstory_refs_it() {
        let srs = "## 3. FR\n| ID | Title |\n|---|---|\n| FR-auth-001 | Đăng ký |\n| FR-auth-002 | Đăng nhập |\n";
        let entries = parse_ids("srs", srs);
        assert!(entries.iter().any(|e| e.ident == "FR-auth-001" && e.role == "def"));

        let us = "## Backlog\n| ID | Story | FR phủ |\n|---|---|---|\n| US-auth-001 | Là learner... | FR-auth-001, FR-auth-002 |\n";
        let entries = parse_ids("userstory", us);
        assert!(entries.iter().any(|e| e.ident == "US-auth-001" && e.role == "def"));
        assert!(entries
            .iter()
            .any(|e| e.ident == "FR-auth-001" && e.role == "ref" && e.from_ident == "US-auth-001"));
    }

    #[test]
    fn ac_heading_anchors_to_story() {
        let ac = "### US-auth-001 — Đăng ký\n| ID | Given | When | Then |\n|---|---|---|---|\n| AC-auth-001 | ... | ... | ... |\n";
        let entries = parse_ids("ac", ac);
        let acdef = entries
            .iter()
            .find(|e| e.ident == "AC-auth-001" && e.role == "def")
            .expect("AC def");
        assert_eq!(acdef.from_ident, "US-auth-001");
    }

    #[test]
    fn oq_resolution_detected() {
        let doc = "## Open Questions\n| OQ | Câu hỏi | Trạng thái |\n|---|---|---|\n| OQ-1 | ngưỡng? | open |\n| OQ-2 | captcha? | resolved |\n";
        let entries = parse_ids("brainstorm", doc);
        let oq1 = entries.iter().find(|e| e.ident == "OQ-1").unwrap();
        let oq2 = entries.iter().find(|e| e.ident == "OQ-2").unwrap();
        assert!(!oq1.resolved);
        assert!(oq2.resolved);
    }

    #[test]
    fn code_fence_ids_never_define() {
        let doc = "```mermaid\nsequenceDiagram\nNote over A: FR-auth-009\n```\n";
        let entries = parse_ids("srs", doc);
        assert!(entries.iter().all(|e| e.role == "ref"));
    }

    fn seed(db: &Db) -> (i64, i64) {
        let p = db.create_project("P", "", "").unwrap();
        let f = db.add_feature(p, "auth", "", "P0").unwrap();
        (p, f)
    }

    fn put(db: &Db, p: i64, f: Option<i64>, dt: &str, content: &str) -> i64 {
        let (id, _) = db
            .upsert_document(p, f, dt, "", &format!("{dt} doc"), content, "markdown", "ai", "", "")
            .unwrap();
        reindex_document(db, id);
        id
    }

    #[test]
    fn coverage_end_to_end() {
        let db = Db::open_memory().unwrap();
        let (p, f) = seed(&db);
        put(&db, p, Some(f), "srs",
            "| FR-auth-001 | a |\n| FR-auth-002 | b |\n| FR-auth-003 | c |\n");
        put(&db, p, Some(f), "userstory",
            "| US-auth-001 | s1 | FR-auth-001, FR-auth-002 |\n| US-auth-002 | mồ côi | — |\n");
        put(&db, p, Some(f), "ac", "### US-auth-001\n| AC-auth-001 | g | w | t |\n");
        put(&db, p, Some(f), "usecase", "### UC-auth-001 — Đăng ký\nLiên quan: FR-auth-001\n");
        put(&db, p, Some(f), "test_cases",
            "| TC-auth-001 | test đăng ký | US-auth-001 |\n| TC-auth-002 | test uc | UC-auth-001 |\n");
        let cov = coverage(&db, p, f);
        assert_eq!(cov["fr_total"], 3);
        assert_eq!(cov["fr_covered"], 2);
        assert_eq!(cov["coverage_pct"], 67.0);
        assert_eq!(cov["fr_uncovered"], json!(["FR-auth-003"]));
        assert_eq!(cov["us_orphans"], json!(["US-auth-002"]));
        assert_eq!(cov["us_without_ac"], json!(["US-auth-002"]));
        // FR-001/002 có test qua US-auth-001; FR-003 không ai test.
        assert_eq!(cov["fr_without_test"], json!(["FR-auth-003"]));
        assert_eq!(cov["uc_without_test"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn pipeline_counts_real_docs_only() {
        let db = Db::open_memory().unwrap();
        let (p, f) = seed(&db);
        put(&db, p, Some(f), "srs", &"x".repeat(100));
        put(&db, p, Some(f), "urd", "ngắn"); // < 50 chars → chưa đạt
        let pipe = pipeline(&db, p, f);
        assert_eq!(pipe["done"], 1);
        assert_eq!(pipe["total"], 8);
    }

    #[test]
    fn staleness_flags_downstream_of_newer_upstream() {
        let db = Db::open_memory().unwrap();
        let (p, f) = seed(&db);
        let srs = put(&db, p, Some(f), "srs", &"s".repeat(100));
        std::thread::sleep(std::time::Duration::from_millis(5));
        // brainstorm là upstream của srs — cập nhật SAU srs ⇒ srs stale.
        put(&db, p, Some(f), "brainstorm", &"b".repeat(100));
        let st = staleness(&db, p, Some(f));
        let srs_item = st["docs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["doc_id"].as_i64() == Some(srs))
            .unwrap();
        assert_eq!(srs_item["stale"], json!(true));
        assert_eq!(srs_item["score"], 60); // vừa stale hôm nay
        assert!(st["chain"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn knowledge_graph_links_refs_and_upstream() {
        let db = Db::open_memory().unwrap();
        let (p, f) = seed(&db);
        let srs = put(&db, p, Some(f), "srs", "| FR-auth-001 | a |\n| FR-auth-002 | b |\n");
        let us = put(&db, p, Some(f), "userstory", "| US-auth-001 | s | FR-auth-001, FR-auth-002 |\n");
        let kg = knowledge_graph(&db, p);
        assert_eq!(kg["nodes"].as_array().unwrap().len(), 2);
        let edges = kg["edges"].as_array().unwrap();
        // upstream: srs → userstory (userstory đọc srs khi sinh)
        assert!(edges.iter().any(|e| e["kind"] == "upstream" && e["from"] == srs && e["to"] == us));
        // ref: userstory nhắc 2 FR định nghĩa ở srs
        assert!(edges.iter().any(|e| e["kind"] == "ref" && e["from"] == us && e["to"] == srs && e["count"] == 2));
        let m = kg["mermaid"].as_str().unwrap();
        assert!(m.starts_with("graph LR"));
        assert!(m.contains(&format!("d{srs}")) && m.contains("|2 id|"));
    }

    #[test]
    fn prd_feature_table_parses() {
        let prd = "## 5. Danh sách tính năng\n\
| Slug | Tên tính năng | Mô tả | Ưu tiên |\n\
|---|---|---|---|\n\
| authentication | Xác thực | Đăng ký đăng nhập | P0 |\n\
| payment | Thanh toán | Mua gói premium | P1 |\n\
\n## 6. Chỉ số\n| a | b |\n";
        let feats = parse_prd_features(prd);
        assert_eq!(feats.len(), 2);
        assert_eq!(feats[0].0, "authentication");
        assert_eq!(feats[0].3, "P0");
        assert_eq!(feats[1].3, "P1");
    }
}
