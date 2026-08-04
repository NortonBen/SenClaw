//! Quizzes built from the learner's own material.
//!
//! The whole module is organised around one rule:
//!
//! > **A question that cannot be traced back to a real sentence in the
//! > document does not get saved.**
//!
//! Every generated question must name a `chunkId` that exists in this document
//! and a `quote` that really occurs inside that chunk. Both are checked in
//! code, and a question failing either is dropped with a reason. This is the
//! same guard `apps/zeach` applies to research claims: a model that invents an
//! evidence id produces a citation nobody can check, attached to something that
//! *looks* verified. In a quiz that failure is worse than useless — it marks
//! the learner wrong for not knowing something the document never said.
//!
//! Grading is arithmetic, never a model call. The model's only role after
//! generation is writing an explanation, and that explanation is built on the
//! already-verified quote.

use serde_json::{json, Value};

use crate::corpus;
use crate::db::Db;
use crate::llm;

/// Question kinds the grader understands.
pub const KINDS: &[&str] = &["single", "multi", "truefalse", "cloze", "order", "match"];

/// Characters of source text shown to the model per generation call.
const GEN_CHARS: usize = 3_200;

const SYSTEM: &str = "Bạn là người ra đề kiểm tra từ tài liệu học. \
Bạn CHỈ được ra câu hỏi về nội dung có trong các ĐOẠN được cung cấp. \
Mỗi câu hỏi BẮT BUỘC kèm `chunkId` của đoạn chứa căn cứ và `quote` là một câu \
TRÍCH NGUYÊN VĂN từ chính đoạn đó. Câu nào không trích nguyên văn được thì \
đừng ra câu đó. Trả về JSON THUẦN, không lời dẫn, không bọc khối mã.";

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuizGenReport {
    pub created: usize,
    /// Questions dropped by the evidence guard, with the reason for each.
    pub rejected: Vec<String>,
}

/// Generate questions for one section.
pub async fn generate_for_section(
    db: &Db,
    section_id: &str,
    count: usize,
    kinds: &[String],
) -> Result<QuizGenReport, String> {
    let sec = db
        .section_get(section_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "không tìm thấy mục".to_string())?;
    let chunks = db
        .chunks_of_section(section_id)
        .map_err(|e| e.to_string())?;
    if chunks.is_empty() {
        return Err("mục này chưa có đoạn nào được chỉ mục — chạy lại chỉ mục trước".into());
    }

    let allowed: Vec<&str> = if kinds.is_empty() {
        KINDS.to_vec()
    } else {
        kinds
            .iter()
            .map(String::as_str)
            .filter(|k| KINDS.contains(k))
            .collect()
    };
    if allowed.is_empty() {
        return Err(format!("dạng câu hỏi không hợp lệ — chọn trong: {}", KINDS.join(", ")));
    }

    // Feed whole chunks with their real ids so the model has something true to
    // point at. Truncating a chunk here would make verbatim quotes from its
    // tail unverifiable, so chunks are included whole until the budget runs out.
    let mut budget = GEN_CHARS;
    let mut shown = Vec::new();
    for c in &chunks {
        let len = c.text.chars().count();
        if len > budget && !shown.is_empty() {
            break;
        }
        shown.push(json!({ "chunkId": c.id, "text": c.text }));
        budget = budget.saturating_sub(len);
        if budget == 0 {
            break;
        }
    }

    let count = count.clamp(1, 20);
    let prompt = format!(
        "Ra {count} câu hỏi kiểm tra từ các đoạn dưới đây của mục \"{}\".\n\
         Dạng được phép: {}.\n\
         Mỗi câu là một object:\n\
         {{ \"kind\": \"...\", \"stem\": \"đề bài\", \"options\": ..., \"answer\": ..., \
         \"explain\": \"giải thích ngắn\", \"chunkId\": <id đoạn>, \"quote\": \"câu trích NGUYÊN VĂN\", \
         \"difficulty\": 1-5 }}\n\
         Quy ước theo dạng:\n\
         - single: options = [\"A\",\"B\",\"C\",\"D\"], answer = chỉ số đáp án đúng (0-based).\n\
         - multi: options = [...], answer = [các chỉ số đúng].\n\
         - truefalse: options = null, answer = true/false.\n\
         - cloze: stem chứa `___`, options = null, answer = \"cụm từ bị che\".\n\
         - order: options = [các bước ĐÃ ĐẢO], answer = [chỉ số theo đúng thứ tự].\n\
         - match: options = {{\"left\": [...], \"right\": [...]}}, answer = [chỉ số bên phải ứng với mỗi mục bên trái].\n\
         `quote` phải xuất hiện Y NGUYÊN trong đoạn có `chunkId` tương ứng.\n\
         Trả về MẢNG JSON.\n\nCÁC ĐOẠN:\n{}",
        sec.title,
        allowed.join(", "),
        serde_json::to_string(&shown).unwrap_or_default()
    );

    let items = llm::ask_json_array(SYSTEM, &prompt, 10_000, "ra đề trắc nghiệm").await?;

    let mut rep = QuizGenReport::default();
    for it in items {
        match validate(&it, &chunks, &allowed) {
            Ok(v) => {
                match db.question_insert(
                    &sec.doc_id,
                    Some(section_id),
                    None,
                    &v.kind,
                    &v.stem,
                    &v.options,
                    &v.answer,
                    &v.explain,
                    v.chunk_id,
                    &v.quote,
                    v.difficulty,
                ) {
                    Ok(_) => rep.created += 1,
                    Err(e) => rep.rejected.push(format!("lưu lỗi: {e}")),
                }
            }
            Err(e) => rep.rejected.push(e),
        }
    }
    Ok(rep)
}

#[derive(Debug)]
struct Valid {
    kind: String,
    stem: String,
    options: Value,
    answer: Value,
    explain: String,
    chunk_id: i64,
    quote: String,
    difficulty: i64,
}

/// The evidence + shape guard. Returns the reason on rejection so the caller
/// can show the learner what was thrown away and why.
fn validate(
    item: &Value,
    chunks: &[crate::db::ChunkRow],
    allowed: &[&str],
) -> Result<Valid, String> {
    let stem = item["stem"].as_str().unwrap_or("").trim().to_string();
    if stem.is_empty() {
        return Err("câu hỏi không có đề bài".into());
    }
    let kind = item["kind"].as_str().unwrap_or("").trim().to_string();
    if !allowed.contains(&kind.as_str()) {
        return Err(format!("`{stem}`: dạng `{kind}` không được phép"));
    }

    // ── Evidence: the id must exist in THIS document's chunks ───────────────
    let chunk_id = item["chunkId"]
        .as_i64()
        .ok_or_else(|| format!("`{stem}`: thiếu chunkId"))?;
    let chunk = chunks
        .iter()
        .find(|c| c.id == chunk_id)
        .ok_or_else(|| format!("`{stem}`: chunkId {chunk_id} không có thật — loại"))?;

    let quote = item["quote"].as_str().unwrap_or("").trim().to_string();
    if quote.chars().count() < 12 {
        return Err(format!("`{stem}`: trích dẫn quá ngắn để kiểm chứng"));
    }
    let hay = corpus::squash_ws(&corpus::fold(&chunk.text));
    let needle = corpus::squash_ws(&corpus::fold(&quote));
    if !hay.contains(&needle) {
        return Err(format!(
            "`{stem}`: trích dẫn không có trong đoạn {chunk_id} — loại"
        ));
    }

    // ── Shape per kind ──────────────────────────────────────────────────────
    let opts = &item["options"];
    let ans = &item["answer"];
    let (options, answer) = match kind.as_str() {
        "single" => {
            let list = str_options(opts).ok_or_else(|| format!("`{stem}`: thiếu lựa chọn"))?;
            let i = ans
                .as_i64()
                .ok_or_else(|| format!("`{stem}`: đáp án phải là chỉ số"))?;
            if i < 0 || i as usize >= list.len() {
                return Err(format!("`{stem}`: chỉ số đáp án ngoài phạm vi"));
            }
            (json!(list), json!(i))
        }
        "multi" => {
            let list = str_options(opts).ok_or_else(|| format!("`{stem}`: thiếu lựa chọn"))?;
            let idx = idx_list(ans).ok_or_else(|| format!("`{stem}`: đáp án phải là mảng chỉ số"))?;
            if idx.is_empty() || idx.iter().any(|i| *i as usize >= list.len()) {
                return Err(format!("`{stem}`: chỉ số đáp án ngoài phạm vi"));
            }
            (json!(list), json!(idx))
        }
        "truefalse" => {
            let b = ans
                .as_bool()
                .ok_or_else(|| format!("`{stem}`: đáp án phải là true/false"))?;
            (Value::Null, json!(b))
        }
        "cloze" => {
            if !stem.contains("___") {
                return Err(format!("`{stem}`: câu điền khuyết phải có chỗ trống `___`"));
            }
            let a = ans.as_str().unwrap_or("").trim().to_string();
            if a.is_empty() {
                return Err(format!("`{stem}`: thiếu đáp án"));
            }
            (Value::Null, json!(a))
        }
        "order" => {
            let list = str_options(opts).ok_or_else(|| format!("`{stem}`: thiếu các bước"))?;
            let idx = idx_list(ans).ok_or_else(|| format!("`{stem}`: thứ tự đúng phải là mảng chỉ số"))?;
            if !is_permutation(&idx, list.len()) {
                return Err(format!("`{stem}`: thứ tự đúng không phải hoán vị của các bước"));
            }
            (json!(list), json!(idx))
        }
        "match" => {
            let left = str_options(&opts["left"])
                .ok_or_else(|| format!("`{stem}`: thiếu cột trái"))?;
            let right = str_options(&opts["right"])
                .ok_or_else(|| format!("`{stem}`: thiếu cột phải"))?;
            let idx = idx_list(ans).ok_or_else(|| format!("`{stem}`: đáp án phải là mảng chỉ số"))?;
            if idx.len() != left.len() || idx.iter().any(|i| *i as usize >= right.len()) {
                return Err(format!("`{stem}`: ghép cặp không khớp số lượng"));
            }
            (json!({ "left": left, "right": right }), json!(idx))
        }
        other => return Err(format!("`{stem}`: dạng `{other}` chưa hỗ trợ")),
    };

    Ok(Valid {
        kind,
        stem,
        options,
        answer,
        explain: item["explain"].as_str().unwrap_or("").trim().to_string(),
        chunk_id,
        quote,
        difficulty: item["difficulty"].as_i64().unwrap_or(3).clamp(1, 5),
    })
}

fn str_options(v: &Value) -> Option<Vec<String>> {
    let a = v.as_array()?;
    let list: Vec<String> = a
        .iter()
        .filter_map(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    (list.len() == a.len() && list.len() >= 2).then_some(list)
}

fn idx_list(v: &Value) -> Option<Vec<i64>> {
    let a = v.as_array()?;
    let list: Vec<i64> = a.iter().filter_map(Value::as_i64).filter(|i| *i >= 0).collect();
    (list.len() == a.len()).then_some(list)
}

fn is_permutation(idx: &[i64], n: usize) -> bool {
    if idx.len() != n || n < 2 {
        return false;
    }
    let mut seen = vec![false; n];
    for i in idx {
        let i = *i as usize;
        if i >= n || seen[i] {
            return false;
        }
        seen[i] = true;
    }
    true
}

// ── Grading ─────────────────────────────────────────────────────────────────

/// Compare a learner's answer with the stored one. Pure and deterministic —
/// no model is asked whether the learner was right.
pub fn is_correct(kind: &str, expected: &Value, given: &Value) -> bool {
    match kind {
        "single" => given.as_i64() == expected.as_i64(),
        "multi" => {
            let (mut a, mut b) = match (idx_list(expected), idx_list(given)) {
                (Some(a), Some(b)) => (a, b),
                _ => return false,
            };
            a.sort_unstable();
            a.dedup();
            b.sort_unstable();
            b.dedup();
            a == b
        }
        "truefalse" => given.as_bool() == expected.as_bool(),
        "cloze" => {
            let e = corpus::squash_ws(&corpus::fold(expected.as_str().unwrap_or("")));
            let g = corpus::squash_ws(&corpus::fold(given.as_str().unwrap_or("")));
            !g.is_empty() && e == g
        }
        "order" | "match" => match (idx_list(expected), idx_list(given)) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        },
        _ => false,
    }
}

/// Grade a whole submission.
///
/// Each wrong answer spawns an SRS card for what was missed, which is what
/// turns a test into part of the study loop instead of a verdict.
pub fn grade(db: &Db, quiz_id: &str, answers: &[(String, Value)]) -> Result<Value, String> {
    let mut results = Vec::new();
    let mut correct_n = 0usize;
    let mut new_cards = 0usize;

    for (qid, given) in answers {
        let Some(q) = db.question_get(qid).map_err(|e| e.to_string())? else {
            results.push(json!({ "questionId": qid, "error": "câu hỏi không tồn tại" }));
            continue;
        };
        let kind = q["kind"].as_str().unwrap_or("");
        let ok = is_correct(kind, &q["answer"], given);
        if ok {
            correct_n += 1;
        } else if let Ok(Some(_)) = crate::cards::card_from_missed_question(db, &q) {
            new_cards += 1;
        }
        db.attempt_insert(qid, quiz_id, given, ok)
            .map_err(|e| e.to_string())?;
        results.push(json!({
            "questionId": qid,
            "correct": ok,
            "expected": q["answer"],
            "given": given,
            "explain": q["explain"],
            // The quote is the point: right or wrong, the learner sees the
            // sentence the question came from.
            "quote": q["quote"],
            "chunkId": q["chunkId"],
            "sectionId": q["sectionId"],
        }));
    }

    let total = results.len();
    Ok(json!({
        "quizId": quiz_id,
        "total": total,
        "correct": correct_n,
        "score": if total > 0 { (correct_n as f64 / total as f64 * 100.0).round() } else { 0.0 },
        "newCards": new_cards,
        "results": results,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ChunkRow;

    fn chunk(id: i64, text: &str) -> ChunkRow {
        ChunkRow {
            id,
            doc_id: "d".into(),
            section_id: Some("s".into()),
            ord: 0,
            char_start: 0,
            char_end: text.chars().count() as i64,
            text: text.into(),
        }
    }

    fn chunks() -> Vec<ChunkRow> {
        vec![chunk(
            7,
            "Lãi suất điều hành do Ngân hàng Nhà nước công bố và tác động tới lãi suất huy động.",
        )]
    }

    fn base(kind: &str) -> Value {
        json!({
            "kind": kind,
            "stem": "Ai công bố lãi suất điều hành?",
            "explain": "Theo tài liệu.",
            "chunkId": 7,
            "quote": "Lãi suất điều hành do Ngân hàng Nhà nước công bố",
            "difficulty": 2,
        })
    }

    #[test]
    fn a_question_citing_a_nonexistent_chunk_is_dropped() {
        let mut q = base("truefalse");
        q["chunkId"] = json!(999);
        q["answer"] = json!(true);
        let err = validate(&q, &chunks(), KINDS).unwrap_err();
        assert!(err.contains("không có thật"), "{err}");
    }

    #[test]
    fn a_question_whose_quote_is_not_in_its_chunk_is_dropped() {
        let mut q = base("truefalse");
        q["quote"] = json!("Ngân hàng Thế giới ấn định lãi suất điều hành");
        q["answer"] = json!(true);
        let err = validate(&q, &chunks(), KINDS).unwrap_err();
        assert!(err.contains("không có trong đoạn"), "{err}");
    }

    #[test]
    fn a_quote_that_differs_only_in_spacing_or_case_still_verifies() {
        let mut q = base("truefalse");
        q["quote"] = json!("lãi suất  điều hành   DO ngân hàng nhà nước công bố");
        q["answer"] = json!(true);
        assert!(validate(&q, &chunks(), KINDS).is_ok());
    }

    #[test]
    fn a_single_choice_answer_outside_the_option_list_is_dropped() {
        let mut q = base("single");
        q["options"] = json!(["NHNN", "Bộ Tài chính"]);
        q["answer"] = json!(5);
        assert!(validate(&q, &chunks(), KINDS).is_err());
    }

    #[test]
    fn a_cloze_without_a_blank_is_dropped() {
        let mut q = base("cloze");
        q["stem"] = json!("Ai công bố lãi suất điều hành?");
        q["answer"] = json!("NHNN");
        assert!(validate(&q, &chunks(), KINDS).is_err());
    }

    #[test]
    fn an_order_answer_that_is_not_a_permutation_is_dropped() {
        let mut q = base("order");
        q["options"] = json!(["A", "B", "C"]);
        q["answer"] = json!([0, 0, 1]);
        assert!(validate(&q, &chunks(), KINDS).is_err());
    }

    #[test]
    fn a_well_formed_question_survives_with_its_evidence_attached() {
        let mut q = base("single");
        q["options"] = json!(["Ngân hàng Nhà nước", "Bộ Tài chính"]);
        q["answer"] = json!(0);
        let v = validate(&q, &chunks(), KINDS).unwrap();
        assert_eq!(v.chunk_id, 7);
        assert!(v.quote.contains("Ngân hàng Nhà nước"));
    }

    #[test]
    fn kinds_not_requested_are_refused_even_if_valid() {
        let mut q = base("truefalse");
        q["answer"] = json!(true);
        let err = validate(&q, &chunks(), &["single"]).unwrap_err();
        assert!(err.contains("không được phép"), "{err}");
    }

    #[test]
    fn grading_is_exact_for_every_kind() {
        assert!(is_correct("single", &json!(2), &json!(2)));
        assert!(!is_correct("single", &json!(2), &json!(1)));

        // Order of a multi-select answer must not matter.
        assert!(is_correct("multi", &json!([0, 2]), &json!([2, 0])));
        assert!(!is_correct("multi", &json!([0, 2]), &json!([0])));

        assert!(is_correct("truefalse", &json!(false), &json!(false)));
        assert!(!is_correct("truefalse", &json!(false), &json!(true)));

        // Cloze forgives case, spacing and diacritic-free typing.
        assert!(is_correct("cloze", &json!("Ngân hàng Nhà nước"), &json!("ngan hang  nha nuoc")));
        assert!(!is_correct("cloze", &json!("Ngân hàng Nhà nước"), &json!("")));

        // Order does not forgive order.
        assert!(is_correct("order", &json!([1, 0, 2]), &json!([1, 0, 2])));
        assert!(!is_correct("order", &json!([1, 0, 2]), &json!([0, 1, 2])));
    }

    #[test]
    fn a_missing_or_malformed_answer_is_wrong_not_a_crash() {
        assert!(!is_correct("single", &json!(1), &Value::Null));
        assert!(!is_correct("multi", &json!([1]), &json!("một")));
        assert!(!is_correct("khong-biet", &json!(1), &json!(1)));
    }
}
