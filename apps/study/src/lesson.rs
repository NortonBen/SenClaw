//! Turning a stored session into something a learner can actually read.
//!
//! One function, deliberately: the part-slicing arithmetic (a section split
//! across several sessions must hand each session *its own* slice) was written
//! twice — once for REST, once for MCP — and two copies of that maths is two
//! chances for the learner to be shown the wrong half of a chapter.

use serde_json::{json, Value};

use crate::db::Db;

/// Attach each item's slice of the document, plus its summary and key points.
///
/// Best-effort per item: a missing section or document leaves that item without
/// text rather than failing the whole session.
pub fn attach_text(db: &Db, mut sess: Value) -> Value {
    let Some(items) = sess["items"].as_array().cloned() else {
        return sess;
    };
    let mut out = Vec::with_capacity(items.len());
    for mut it in items {
        if let Some(sid) = it["sectionId"].as_str() {
            if let Ok(Some(sec)) = db.section_get(sid) {
                if let Ok(Some(body)) = db.doc_body(&sec.doc_id) {
                    let chars: Vec<char> = body.chars().collect();
                    let (a, b) = part_span(
                        sec.char_start as usize,
                        sec.char_end as usize,
                        it["part"].as_i64().unwrap_or(1),
                        it["parts"].as_i64().unwrap_or(1),
                        chars.len(),
                    );
                    it["text"] = json!(chars[a..b].iter().collect::<String>());
                    it["summary"] = json!(sec.summary);
                    it["keyPoints"] = json!(sec.key_points);
                    it["docId"] = json!(sec.doc_id);
                    it["charStart"] = json!(a);
                    it["charEnd"] = json!(b);
                }
            }
        }
        out.push(it);
    }
    sess["items"] = Value::Array(out);
    sess
}

/// Char span of part `part` of `parts` within `[start, end)`, clamped to `len`.
///
/// The parts tile the section exactly: part *i* ends where part *i+1* begins,
/// so nothing is skipped and nothing is read twice.
pub fn part_span(start: usize, end: usize, part: i64, parts: i64, len: usize) -> (usize, usize) {
    let a0 = start.min(len);
    let b0 = end.min(len).max(a0);
    let parts = parts.max(1) as usize;
    let part = (part.max(1) as usize).min(parts);
    let span = b0 - a0;
    let a = a0 + (span * (part - 1)) / parts;
    let b = a0 + (span * part) / parts;
    (a, b.min(len).max(a))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_part_is_the_whole_section() {
        assert_eq!(part_span(10, 50, 1, 1, 100), (10, 50));
    }

    #[test]
    fn parts_tile_the_section_with_no_gap_and_no_overlap() {
        let (a1, b1) = part_span(0, 100, 1, 3, 100);
        let (a2, b2) = part_span(0, 100, 2, 3, 100);
        let (a3, b3) = part_span(0, 100, 3, 3, 100);
        assert_eq!((a1, b1), (0, 33));
        assert_eq!(a2, b1);
        assert_eq!(a3, b2);
        assert_eq!(b3, 100, "the last part must reach the end of the section");
    }

    #[test]
    fn an_out_of_range_part_is_clamped_rather_than_panicking() {
        let (a, b) = part_span(0, 100, 9, 3, 100);
        assert!(a < b && b <= 100);
        assert_eq!(part_span(0, 100, 0, 0, 100), (0, 100));
    }

    #[test]
    fn a_span_past_the_end_of_the_document_is_clamped() {
        assert_eq!(part_span(90, 500, 1, 1, 100), (90, 100));
        assert_eq!(part_span(500, 900, 1, 1, 100), (100, 100));
    }

    #[test]
    fn attaching_text_fills_every_item_that_names_a_section() {
        let db = Db::open_memory().unwrap();
        let body = format!("# Chương 1\n\n{}", "nội dung học ".repeat(120));
        let doc = db.doc_insert("D", "d.md", "md", 1, "ok", &body).unwrap();
        crate::outline::index_document(&db, &doc).unwrap();
        let sec = db.sections_of(&doc).unwrap()[0].clone();

        let sess = json!({ "items": [
            { "sectionId": sec.id, "part": 1, "parts": 2 },
            { "sectionId": sec.id, "part": 2, "parts": 2 },
            { "sectionId": null,   "part": 1, "parts": 1 },
        ]});
        let out = attach_text(&db, sess);
        let items = out["items"].as_array().unwrap();
        let t1 = items[0]["text"].as_str().unwrap();
        let t2 = items[1]["text"].as_str().unwrap();
        assert!(!t1.is_empty() && !t2.is_empty());
        assert_ne!(t1, t2, "each part must show a different slice");
        assert_eq!(items[0]["docId"], doc);
        assert!(items[2]["text"].is_null(), "an item with no section stays as it was");
    }
}
