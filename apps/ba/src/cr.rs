//! Change Request engine — một thay đổi cập nhật ĐỒNG BỘ các tài liệu liên
//! quan (tinh thần /cr của BA-Kit): AI phân tích tác động thành danh sách
//! impact trên tài liệu THẬT trong DB, rồi apply từng impact = viết lại tài
//! liệu draft-first (version mới, trạng thái quay về draft chờ review).

use crate::db::Db;
use crate::llm;
use crate::templates;
use serde_json::{json, Value};

fn today_yyyymmdd() -> String {
    chrono::Local::now().format("%Y%m%d").to_string()
}

/// Tạo CR + AI phân tích tác động. Trả CR đầy đủ kèm impacts.
pub async fn cr_create_value(
    db: &Db,
    project_id: i64,
    feature_id: Option<i64>,
    title: &str,
    description: &str,
    severity: &str,
) -> Value {
    let Some(project) = db.get_project(project_id) else {
        return json!({ "error": format!("dự án #{project_id} không tồn tại") });
    };
    if title.trim().is_empty() || description.trim().is_empty() {
        return json!({ "error": "CR cần title và description (mô tả thay đổi là gì, vì sao)" });
    }
    let severity = match severity {
        "" => "medium",
        s @ ("low" | "medium" | "high") => s,
        other => return json!({ "error": format!("severity '{other}' không hợp lệ — low | medium | high") }),
    };

    // Tập tài liệu trong tầm ảnh hưởng: doc của feature (nếu chỉ định) + doc
    // cấp project; không chỉ định feature thì toàn bộ doc của project.
    let mut docs: Vec<Value> = Vec::new();
    match feature_id {
        Some(fid) => {
            docs.extend(db.docs_with_content(project_id, Some(fid)));
            docs.extend(db.docs_with_content(project_id, None));
        }
        None => {
            for d in db.list_documents(project_id, None, None) {
                if let Some(full) = db.get_document(d["id"].as_i64().unwrap_or(0)) {
                    docs.push(full);
                }
            }
        }
    }
    if docs.is_empty() {
        return json!({ "error": "chưa có tài liệu nào để phân tích tác động — CR chỉ có nghĩa khi đã có tài liệu" });
    }

    let code = db.next_cr_code(&today_yyyymmdd());
    let cr_id = match db.create_cr(project_id, feature_id, &code, title, description, severity) {
        Ok(id) => id,
        Err(e) => return json!({ "error": e.to_string() }),
    };

    // Prompt phân tích: danh sách doc (id + loại + tiêu đề + đoạn đầu).
    let mut listing = String::new();
    let mut valid_ids: Vec<i64> = Vec::new();
    for d in &docs {
        let id = d["id"].as_i64().unwrap_or(0);
        valid_ids.push(id);
        let (clean, _) = llm::sanitize_retrieved(d["content"].as_str().unwrap_or(""));
        let head: String = clean.chars().take(1200).collect();
        listing.push_str(&format!(
            "--- doc_id={} | loại={}{} | tiêu đề={} | trạng thái={} ---\n{}\n\n",
            id,
            d["doc_type"].as_str().unwrap_or(""),
            d["subtype"].as_str().map(|s| if s.is_empty() { String::new() } else { format!("/{s}") }).unwrap_or_default(),
            d["title"].as_str().unwrap_or(""),
            d["status"].as_str().unwrap_or(""),
            head
        ));
    }
    let (desc_clean, _) = llm::sanitize_retrieved(description);
    let prompt = format!(
        "Dự án: {}\nCHANGE REQUEST {code}: {title}\nMô tả thay đổi:\n{}\n\nDanh sách tài liệu hiện có (mỗi cái kèm đoạn đầu):\n{}\nPhân tích tác động của thay đổi này. Trả về JSON DUY NHẤT dạng:\n{{\n  \"analysis\": \"markdown phân tích: thay đổi chạm những đâu, mức độ, rủi ro, thứ tự nên cập nhật\",\n  \"impacts\": [ {{ \"doc_id\": <số trong danh sách trên>, \"summary\": \"mục nào của tài liệu phải sửa, sửa thành gì (1-3 câu)\" }} ]\n}}\nQuy tắc: CHỈ liệt kê tài liệu THẬT SỰ phải sửa (đừng rải thảm); doc_id phải nằm trong danh sách; tài liệu không bị ảnh hưởng thì không đưa vào.",
        project["name"].as_str().unwrap_or(""),
        desc_clean,
        listing
    );
    let text = match llm::bridge_llm(
        "Bạn là Business Analyst phân tích tác động thay đổi (impact analysis), tiếng Việt, trả về đúng JSON được yêu cầu, không thêm chữ nào ngoài JSON.",
        &prompt,
        8000,
    )
    .await
    {
        Ok(t) => t,
        Err(e) => {
            let _ = db.set_cr_analysis(cr_id, &format!("(phân tích thất bại: {e})"), "open");
            return json!({ "error": format!("tạo CR {code} xong nhưng phân tích tác động thất bại: {e} — gọi ba_cr_get rồi thử lại") });
        }
    };
    let Some(parsed) = llm::extract_json(&text) else {
        let _ = db.set_cr_analysis(cr_id, "(AI không trả JSON hợp lệ)", "open");
        return json!({ "error": "AI không trả JSON hợp lệ khi phân tích tác động — thử lại" });
    };
    let analysis = parsed["analysis"].as_str().unwrap_or("").to_string();
    let mut n_impacts = 0;
    for imp in parsed["impacts"].as_array().cloned().unwrap_or_default() {
        let doc_id = imp["doc_id"].as_i64().unwrap_or(0);
        let summary = imp["summary"].as_str().unwrap_or("").trim().to_string();
        if valid_ids.contains(&doc_id) && !summary.is_empty() {
            if db.add_cr_impact(cr_id, doc_id, &summary).is_ok() {
                n_impacts += 1;
            }
        }
    }
    let _ = db.set_cr_analysis(cr_id, &analysis, "analyzed");
    db.log("ai", "cr_create", &format!("{code}: {n_impacts} tài liệu bị ảnh hưởng"));
    match db.get_cr(cr_id) {
        Some(cr) => json!({ "ok": true, "cr": cr }),
        None => json!({ "error": "CR biến mất sau khi tạo?" }),
    }
}

/// Apply MỘT impact: AI viết lại tài liệu theo CR (draft-first). impact_id=None
/// → apply impact pending đầu tiên.
pub async fn cr_apply_value(db: &Db, cr_id: i64, impact_id: Option<i64>) -> Value {
    let Some(cr) = db.get_cr(cr_id) else {
        return json!({ "error": format!("CR #{cr_id} không tồn tại") });
    };
    let code = cr["code"].as_str().unwrap_or("").to_string();
    let impact = match impact_id {
        Some(iid) => db.get_impact(iid).map(|x| (iid, x)),
        None => cr["impacts"]
            .as_array()
            .and_then(|arr| {
                arr.iter()
                    .find(|i| i["status"] == json!("pending"))
                    .and_then(|i| i["id"].as_i64())
            })
            .and_then(|iid| db.get_impact(iid).map(|x| (iid, x))),
    };
    let Some((iid, (imp_cr_id, document_id, summary, status))) = impact else {
        return json!({ "error": "không còn impact pending nào — ba_cr_get để xem trạng thái, đóng CR bằng status closed" });
    };
    if imp_cr_id != cr_id {
        return json!({ "error": format!("impact #{iid} không thuộc CR #{cr_id}") });
    }
    if status != "pending" {
        return json!({ "error": format!("impact #{iid} đã ở trạng thái '{status}'") });
    }
    let Some(doc) = db.get_document(document_id) else {
        return json!({ "error": format!("tài liệu #{document_id} của impact không còn tồn tại — đánh dấu skip") });
    };
    let doc_type = doc["doc_type"].as_str().unwrap_or("").to_string();
    let subtype = doc["subtype"].as_str().unwrap_or("").to_string();
    let tpl = templates::get(&doc_type, &subtype);
    let (content_clean, _) = llm::sanitize_retrieved(doc["content"].as_str().unwrap_or(""));
    let (cr_desc_clean, _) = llm::sanitize_retrieved(cr["description"].as_str().unwrap_or(""));

    let mut prompt = format!(
        "CHANGE REQUEST {code}: {}\nMô tả thay đổi:\n{}\n\nViệc cần làm trên tài liệu này (từ phân tích tác động): {}\n\n===== TÀI LIỆU HIỆN TẠI ({}) =====\n{}\n===== HẾT =====\n\nViết lại TOÀN BỘ tài liệu đã cập nhật theo CR. Quy tắc:\n- Giữ nguyên khung section và mọi phần KHÔNG liên quan tới thay đổi (kể cả ID cũ — không đánh lại số ID đang có).\n- Mục bị sửa: cập nhật nội dung; mục/dòng mới thêm ID nối tiếp số lớn nhất hiện có.\n- Ngay dưới tiêu đề thêm dòng ghi chú: `> {code}: <tóm tắt 1 câu thay đổi đã áp>`. Nếu đã có ghi chú CR cũ thì giữ và thêm dòng mới.\n- Trả về đúng định dạng gốc ({}).",
        cr["title"].as_str().unwrap_or(""),
        cr_desc_clean,
        summary,
        doc["title"].as_str().unwrap_or(""),
        content_clean,
        if doc["format"] == json!("html") { "HTML thuần, bắt đầu <!DOCTYPE html>" } else { "markdown, bắt đầu bằng `# `" }
    );
    if let Some(t) = tpl {
        if !t.sections.is_empty() {
            prompt.push_str(&format!("\nKhung section chuẩn của loại tài liệu này:\n{}\n", t.sections.join("\n")));
        }
    }
    let max_tokens = tpl.map(|t| t.max_tokens).unwrap_or(16000);
    let text = match llm::bridge_llm(templates::SYSTEM_BA, &prompt, max_tokens).await {
        Ok(t) => t,
        Err(e) => return json!({ "error": format!("apply impact #{iid} thất bại: {e}") }),
    };
    let new_content = llm::strip_outer_fence(&text);
    if let Err(e) = db.update_document(document_id, None, Some(&new_content), None) {
        return json!({ "error": e.to_string() });
    }
    // update_document đặt source='user' cho sửa tay — đây là AI sửa theo CR,
    // ghi đè nhãn lại cho đúng nguồn gốc + trạng thái quay về draft chờ review.
    let _ = db.update_document(document_id, None, None, Some("draft"));
    crate::trace::reindex_document(db, document_id);
    let _ = db.set_impact_status(iid, "applied");
    let pending = db.cr_pending_impacts(cr_id);
    if pending == 0 {
        let _ = db.set_cr_status(cr_id, "applied");
    }
    db.log("ai", "cr_apply", &format!("{code} → tài liệu #{document_id} ({})", doc["title"].as_str().unwrap_or("")));
    json!({
        "ok": true,
        "applied_impact": iid,
        "document": db.get_document(document_id),
        "impacts_pending": pending,
        "cr": db.get_cr(cr_id),
    })
}

/// skip một impact / đóng CR.
pub fn cr_update_value(db: &Db, cr_id: i64, impact_skip: Option<i64>, close: bool) -> Value {
    if let Some(iid) = impact_skip {
        match db.get_impact(iid) {
            Some((imp_cr, _, _, st)) if imp_cr == cr_id => {
                if st != "pending" {
                    return json!({ "error": format!("impact #{iid} đã '{st}'") });
                }
                let _ = db.set_impact_status(iid, "skipped");
                if db.cr_pending_impacts(cr_id) == 0 {
                    let _ = db.set_cr_status(cr_id, "applied");
                }
            }
            Some(_) => return json!({ "error": format!("impact #{iid} không thuộc CR #{cr_id}") }),
            None => return json!({ "error": format!("impact #{iid} không tồn tại") }),
        }
    }
    if close {
        if let Err(e) = db.set_cr_status(cr_id, "closed") {
            return json!({ "error": e.to_string() });
        }
    }
    match db.get_cr(cr_id) {
        Some(cr) => json!({ "ok": true, "cr": cr }),
        None => json!({ "error": format!("CR #{cr_id} không tồn tại") }),
    }
}
