//! Flashcards: where they come from, and what a review does to them.
//!
//! Cards are generated **from a section the learner actually uploaded**, and
//! every card keeps the `section_id` (and, when known, the `chunk_id`) it came
//! from, so "why is this the answer?" is always one click from the source.
//!
//! Cloze cards are preferred over free-form Q/A wherever the source sentence
//! supports one: blanking a word out of the author's own sentence is the form
//! with the least room for a model to invent something.

use serde_json::Value;

use crate::corpus;
use crate::db::Db;
use crate::llm;
use crate::srs::{self, Grade};

/// Section characters shown to the model when generating.
const GEN_CHARS: usize = 3_000;

const SYSTEM: &str = "Bạn là người soạn thẻ ghi nhớ (flashcard) từ tài liệu học. \
Bạn CHỈ được dùng thông tin có trong đoạn tài liệu được cung cấp — không thêm kiến thức ngoài. \
Trả về JSON THUẦN, không lời dẫn, không bọc trong khối mã.";

/// Study slots + timezone, with sane defaults.
pub fn slots_and_tz(db: &Db) -> (Vec<String>, chrono_tz::Tz) {
    let slots: Vec<String> = db
        .setting("study_slots")
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| vec!["20:00".to_string()]);
    let tz = srs::parse_tz(&db.setting("tz").unwrap_or_else(|| "Asia/Ho_Chi_Minh".into()));
    (slots, tz)
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenReport {
    pub created: usize,
    /// Cards the model produced that already existed — reported, not hidden,
    /// so "generate" twice doesn't look like it did nothing.
    pub duplicates: usize,
    /// Cards dropped because they failed validation.
    pub rejected: usize,
    pub problems: Vec<String>,
}

/// Generate flashcards for one section.
pub async fn generate_for_section(
    db: &Db,
    section_id: &str,
    count: usize,
) -> Result<GenReport, String> {
    let sec = db
        .section_get(section_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "không tìm thấy mục".to_string())?;
    let body = db
        .doc_body(&sec.doc_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "không tìm thấy tài liệu".to_string())?;
    let chars: Vec<char> = body.chars().collect();
    let a = (sec.char_start as usize).min(chars.len());
    let b = (sec.char_end as usize).min(chars.len()).max(a);
    let text: String = chars[a..b].iter().collect();
    if text.trim().is_empty() {
        return Err("mục này không có nội dung".into());
    }

    let count = count.clamp(1, 30);
    let prompt = format!(
        "Soạn {count} thẻ ghi nhớ từ đoạn tài liệu dưới đây.\n\
         Mỗi thẻ là một object: {{ \"front\": \"mặt trước\", \"back\": \"mặt sau\", \
         \"kind\": \"qa\" | \"cloze\" | \"define\" }}.\n\
         - \"cloze\": lấy NGUYÊN một câu trong tài liệu và thay cụm từ khoá bằng `___`; \
         mặt sau là cụm đã bị che. Ưu tiên dạng này.\n\
         - \"define\": mặt trước là tên khái niệm, mặt sau là định nghĩa theo tài liệu.\n\
         - \"qa\": câu hỏi ngắn, trả lời trong 1-2 câu.\n\
         Mặt trước phải trả lời được mà không cần nhìn tài liệu. Không tạo thẻ trùng ý nhau.\n\
         Trả về MẢNG JSON.\n\nMỤC \"{}\":\n{}",
        sec.title,
        corpus::head(&text, GEN_CHARS)
    );

    let items = llm::ask_json_array(SYSTEM, &prompt, 6_000, "soạn thẻ ghi nhớ").await?;

    let mut rep = GenReport::default();
    for it in items {
        let front = it["front"].as_str().unwrap_or("").trim().to_string();
        let back = it["back"].as_str().unwrap_or("").trim().to_string();
        let kind = match it["kind"].as_str().unwrap_or("qa") {
            "cloze" => "cloze",
            "define" => "define",
            _ => "qa",
        };
        if front.is_empty() || back.is_empty() {
            rep.rejected += 1;
            continue;
        }
        // A cloze card with no blank is a Q/A card wearing the wrong label; the
        // learner would see the answer on the front.
        if kind == "cloze" && !front.contains("___") {
            rep.rejected += 1;
            continue;
        }
        if db
            .card_exists(Some(section_id), &front)
            .map_err(|e| e.to_string())?
        {
            rep.duplicates += 1;
            continue;
        }
        // Bind the card to the chunk its front text actually came from, when we
        // can find one; that is what makes "show me where this is from" work.
        let chunk_id = locate_chunk(db, section_id, &back).or_else(|| locate_chunk(db, section_id, &front));
        match db.card_insert(
            Some(&sec.doc_id),
            Some(section_id),
            chunk_id,
            None,
            &front,
            &back,
            kind,
            "ai",
        ) {
            Ok(_) => rep.created += 1,
            Err(e) => rep.problems.push(e.to_string()),
        }
    }
    Ok(rep)
}

/// The chunk of this section whose text contains `needle` (whitespace-
/// insensitive). None when the model paraphrased rather than quoted.
fn locate_chunk(db: &Db, section_id: &str, needle: &str) -> Option<i64> {
    let probe = corpus::squash_ws(&corpus::fold(needle));
    let probe = probe.trim();
    if probe.chars().count() < 12 {
        return None;
    }
    let chunks = db.chunks_of_section(section_id).ok()?;
    chunks
        .into_iter()
        .find(|c| corpus::squash_ws(&corpus::fold(&c.text)).contains(probe))
        .map(|c| c.id)
}

/// Record a review and return the card's new state.
pub fn review(db: &Db, card_id: &str, grade: &str) -> Result<Value, String> {
    let Some(grade) = Grade::parse(grade) else {
        return Err(format!(
            "mức đánh giá không hợp lệ: `{grade}` — dùng again/hard/good/easy"
        ));
    };
    let card = db
        .card_get(card_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "không tìm thấy thẻ".to_string())?;
    let existing = db.card_progress_get(card_id).map_err(|e| e.to_string())?;
    let (slots, tz) = slots_and_tz(db);
    let next = srs::apply(existing.as_ref(), grade, &slots, tz, chrono::Utc::now());
    db.card_progress_put(card_id, &next)
        .map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "cardId": card.id,
        "level": next.level,
        "nextReview": srs::fmt(next.next_review),
        "isUrgent": next.is_urgent,
        "reviews": next.reviews,
        "lapses": next.lapses,
    }))
}

/// Create (or find) the card a missed quiz question should drill.
///
/// A wrong answer is the strongest signal available that a concept has not
/// landed. Turning it straight into a due card is what closes the loop between
/// testing and review.
pub fn card_from_missed_question(db: &Db, question: &Value) -> Result<Option<String>, String> {
    let stem = question["stem"].as_str().unwrap_or("").trim();
    let quote = question["quote"].as_str().unwrap_or("").trim();
    if stem.is_empty() || quote.is_empty() {
        return Ok(None);
    }
    let doc_id = question["docId"].as_str();
    let section_id = question["sectionId"].as_str();
    if db
        .card_exists(section_id, stem)
        .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }
    let back = match question["explain"].as_str().map(str::trim) {
        Some(x) if !x.is_empty() => format!("{x}\n\nTrích tài liệu: “{quote}”"),
        _ => format!("Trích tài liệu: “{quote}”"),
    };
    let id = db
        .card_insert(
            doc_id,
            section_id,
            question["chunkId"].as_i64(),
            None,
            stem,
            &back,
            "qa",
            "quiz-miss",
        )
        .map_err(|e| e.to_string())?;
    Ok(Some(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn seeded() -> (Db, String, String) {
        let db = Db::open_memory().unwrap();
        let body = "# Lãi suất\n\nLãi suất điều hành do Ngân hàng Nhà nước công bố định kỳ và \
                    ảnh hưởng trực tiếp tới lãi suất huy động của các ngân hàng thương mại."
            .to_string();
        let doc = db
            .doc_insert("Kinh tế", "kt.md", "md", 10, "ok", &body)
            .unwrap();
        let (secs, _) = crate::outline::outline(&body);
        let ids = db.sections_replace(&doc, &secs).unwrap();
        let sid = ids[0].clone();
        let bounds: Vec<(usize, usize, String)> = secs
            .iter()
            .zip(ids.iter())
            .map(|(s, i)| (s.char_start, s.char_end, i.clone()))
            .collect();
        db.chunks_replace(&doc, &corpus::chunk(&body), |ch| {
            let mid = ch.char_start + (ch.char_end - ch.char_start) / 2;
            bounds
                .iter()
                .find(|(a, b, _)| mid >= *a && mid < *b)
                .map(|(_, _, i)| i.clone())
        })
        .unwrap();
        (db, doc, sid)
    }

    #[test]
    fn a_review_moves_the_card_up_the_ladder_and_persists() {
        let (db, doc, sid) = seeded();
        let id = db
            .card_insert(Some(&doc), Some(&sid), None, None, "Lãi suất là gì?", "Giá của tiền", "qa", "manual")
            .unwrap();
        let out = review(&db, &id, "good").unwrap();
        assert_eq!(out["level"], 1);
        let again = review(&db, &id, "good").unwrap();
        assert_eq!(again["level"], 1, "an early review must not promote");
        assert_eq!(db.card_get(&id).unwrap().unwrap().level, 1);
    }

    #[test]
    fn an_unknown_grade_is_refused_rather_than_treated_as_correct() {
        let (db, doc, sid) = seeded();
        let id = db
            .card_insert(Some(&doc), Some(&sid), None, None, "A", "B", "qa", "manual")
            .unwrap();
        assert!(review(&db, &id, "maybe").is_err());
    }

    #[test]
    fn a_new_card_is_due_immediately_and_leaves_the_due_list_after_review() {
        let (db, doc, sid) = seeded();
        let id = db
            .card_insert(Some(&doc), Some(&sid), None, None, "A", "B", "qa", "manual")
            .unwrap();
        let now = srs::fmt(chrono::Utc::now());
        assert_eq!(db.card_due_count(&now).unwrap(), 1);
        review(&db, &id, "good").unwrap();
        assert_eq!(
            db.card_due_count(&now).unwrap(),
            0,
            "a just-reviewed card must not still be due"
        );
    }

    #[test]
    fn duplicate_fronts_are_detected_ignoring_case_diacritics_and_spacing() {
        let (db, doc, sid) = seeded();
        db.card_insert(Some(&doc), Some(&sid), None, None, "Lãi suất điều hành", "x", "define", "ai")
            .unwrap();
        assert!(db.card_exists(Some(&sid), "lai  suat dieu hanh").unwrap());
        assert!(!db.card_exists(Some(&sid), "Tỷ giá trung tâm").unwrap());
    }

    #[test]
    fn a_cards_source_chunk_is_found_when_the_back_quotes_the_document() {
        let (_db, _doc, sid) = seeded();
        let (db, _d, s) = (_db, _doc, sid);
        let hit = locate_chunk(&db, &s, "do Ngân hàng Nhà nước công bố định kỳ");
        assert!(hit.is_some(), "a verbatim quote must resolve to its chunk");
        assert!(
            locate_chunk(&db, &s, "một câu hoàn toàn không có trong tài liệu này").is_none()
        );
    }

    #[test]
    fn a_missed_question_becomes_a_card_once_not_twice() {
        let (db, doc, sid) = seeded();
        let q = serde_json::json!({
            "stem": "Ai công bố lãi suất điều hành?",
            "quote": "Lãi suất điều hành do Ngân hàng Nhà nước công bố",
            "explain": "NHNN công bố.",
            "docId": doc,
            "sectionId": sid,
            "chunkId": 1,
        });
        assert!(card_from_missed_question(&db, &q).unwrap().is_some());
        assert!(
            card_from_missed_question(&db, &q).unwrap().is_none(),
            "the same miss must not pile up duplicate cards"
        );
    }

    #[test]
    fn default_study_slot_and_timezone_are_usable_without_configuration() {
        let db = Db::open_memory().unwrap();
        let (slots, tz) = slots_and_tz(&db);
        assert_eq!(slots, vec!["20:00".to_string()]);
        assert_eq!(tz, chrono_tz::Asia::Ho_Chi_Minh);
    }
}
