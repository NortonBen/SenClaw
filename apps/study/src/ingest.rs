//! The one path a document takes to get into the app.
//!
//! Extract → **clean** → store → outline → index. REST upload, REST paste and
//! the MCP tool all go through here; when this was written twice, the two
//! copies drifted immediately.
//!
//! "Clean" here means only what is unambiguous: duplicate paragraphs. Repeated
//! short lines — a PDF running header, or a textbook's "Bài tập 1" under every
//! chapter — are reported for the user to judge, never guessed at. See
//! [`crate::corpus::dedupe`].

use serde_json::{json, Value};

use crate::corpus;
use crate::db::Db;
use crate::outline;

/// Take raw bytes to an indexed document. Returns the upload response body.
pub fn ingest(db: &Db, filename: &str, bytes: &[u8], title: &str) -> Result<Value, String> {
    let ex = corpus::extract(filename, bytes).map_err(|e| e.to_string())?;

    let cleaned = corpus::dedupe(&ex.text);
    // Never let the cleaner empty a document. If it would, the heuristics were
    // wrong about this file and the original is the safer answer.
    let body = if cleaned.text.trim().is_empty() {
        ex.text.clone()
    } else {
        cleaned.text.clone()
    };

    let ext = corpus::extension_of(filename);
    let title = if title.trim().is_empty() {
        filename
            .rsplit('/')
            .next()
            .unwrap_or(filename)
            .rsplit_once('.')
            .map(|(a, _)| a.to_string())
            .unwrap_or_else(|| filename.to_string())
    } else {
        title.trim().to_string()
    };

    let note = match cleaned.note() {
        Some(c) => format!("{} · {c}", ex.note),
        None => ex.note.clone(),
    };

    let id = db
        .doc_insert(&title, filename, &ext, bytes.len() as i64, &note, &body)
        .map_err(|e| e.to_string())?;
    db.set_suspects(&id, &cleaned.suspects).map_err(|e| e.to_string())?;
    let (sections, chunks, outline_note) = outline::index_document(db, &id)?;

    Ok(json!({
        "id": id,
        "title": title,
        "extractNote": note,
        "removedParagraphs": cleaned.removed_paragraphs,
        "removedSamples": cleaned.samples,
        // Left in the text on purpose — the UI shows these for confirmation.
        "suspectedFurniture": cleaned.suspects,
        "sections": sections,
        "chunks": chunks,
        "note": outline_note,
        "next": if cleaned.suspects.is_empty() {
            "gọi /api/docs/{id}/enrich để AI mô tả từng mục trước khi lập kế hoạch"
        } else {
            "có dòng lặp lại nhiều lần — xem `suspectedFurniture`, bỏ những dòng đúng là đầu/chân trang rồi mới enrich"
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pdf_style_running_header_is_flagged_but_left_in_place() {
        let mut src = String::new();
        for i in 0..8 {
            src.push_str("Giáo trình Kinh tế vĩ mô — NXB Giáo dục\n\n");
            src.push_str(&format!(
                "Đoạn nội dung số {i} nói về một khía cạnh khác của chính sách tiền tệ.\n\n"
            ));
        }
        let out = corpus::dedupe(&src);
        assert_eq!(out.suspects.len(), 1);
        assert_eq!(out.suspects[0].count, 8);
        assert!(
            out.text.contains("NXB Giáo dục"),
            "flagged, not deleted — the user decides"
        );
    }

    #[test]
    fn a_repeated_section_label_is_flagged_the_same_way_and_survives() {
        // The case that made guessing indefensible: "Bài tập 1" under every
        // chapter is structure, and it is indistinguishable from a page header
        // by frequency, length or spacing.
        let mut src = String::new();
        for i in 1..=6 {
            src.push_str(&format!("# Chương {i}\n\nNội dung chương {i}.\n\nBài tập 1\n\n"));
        }
        let out = corpus::dedupe(&src);
        assert_eq!(out.text.matches("Bài tập 1").count(), 6);
        assert!(out.suspects.iter().any(|s| s.line == "Bài tập 1"));
    }

    #[test]
    fn an_adjacent_repeated_paragraph_collapses() {
        let src = "Một câu ngắn.\n\nMột câu ngắn.\n\nCâu khác.";
        let out = corpus::dedupe(src);
        assert_eq!(out.removed_paragraphs, 1);
        assert_eq!(out.text, "Một câu ngắn.\n\nCâu khác.");
    }

    #[test]
    fn a_long_paragraph_repeated_far_apart_keeps_only_the_first() {
        let long = "Lãi suất điều hành do Ngân hàng Nhà nước công bố và là công cụ chính sách \
                    tiền tệ quan trọng nhất, tác động tới chi phí vốn của ngân hàng thương mại \
                    và qua đó tới lãi suất huy động cũng như lãi suất cho vay trên thị trường.";
        let src = format!("{long}\n\nĐoạn xen giữa.\n\n{long}");
        let out = corpus::dedupe(&src);
        assert_eq!(out.removed_paragraphs, 1);
        assert_eq!(out.text.matches("Lãi suất điều hành do").count(), 1);
        assert!(out.text.contains("Đoạn xen giữa"));
    }

    #[test]
    fn a_short_paragraph_repeated_far_apart_is_left_alone() {
        let src = "Có.\n\nMột đoạn dài hơn ở giữa để tách hai câu trả lời ra.\n\nCó.";
        let out = corpus::dedupe(src);
        assert_eq!(out.removed_paragraphs, 0);
        assert_eq!(out.text.matches("Có.").count(), 2);
    }

    #[test]
    fn ordinary_prose_comes_through_untouched() {
        let src = "# Chương 1\n\nCâu thứ nhất.\n\nCâu thứ hai khác hẳn.";
        let out = corpus::dedupe(src);
        assert_eq!(out.removed_paragraphs, 0);
        assert!(out.suspects.is_empty());
        assert!(out.note().is_none());
        assert_eq!(out.text, src);
    }

    #[test]
    fn stripping_lines_is_whitespace_insensitive_and_counts_what_it_took() {
        let src = "Đầu  trang\n\nNội dung.\n\nĐầu trang\n\nNội dung 2.";
        let (out, n) = corpus::strip_lines(src, &["Đầu trang".to_string()]);
        assert_eq!(n, 2);
        assert!(!out.contains("Đầu"));
        assert!(out.contains("Nội dung 2."));
    }

    #[test]
    fn stripping_nothing_changes_nothing() {
        let src = "Một câu.";
        assert_eq!(corpus::strip_lines(src, &[]), (src.to_string(), 0));
    }

    #[test]
    fn the_cleaner_never_empties_a_document() {
        let db = Db::open_memory().unwrap();
        let src = "ghi chú\n\n".repeat(10);
        let out = ingest(&db, "x.md", src.as_bytes(), "").unwrap();
        let body = db.doc_body(out["id"].as_str().unwrap()).unwrap().unwrap();
        assert!(!body.trim().is_empty(), "a cleaned-away document is a lost document");
    }

    #[test]
    fn ingest_reports_suspects_and_tells_the_caller_to_review_before_enriching() {
        let db = Db::open_memory().unwrap();
        let mut src = String::new();
        for i in 0..6 {
            src.push_str("Trang chạy — Tài liệu nội bộ\n\n");
            src.push_str(&format!("Nội dung thật số {i} với đủ chữ để không bị coi là rác.\n\n"));
        }
        let out = ingest(&db, "tai-lieu.md", src.as_bytes(), "Tài liệu").unwrap();
        let suspects = out["suspectedFurniture"].as_array().unwrap();
        assert_eq!(suspects.len(), 1);
        assert_eq!(suspects[0]["count"], 6);
        assert!(out["next"].as_str().unwrap().contains("dòng lặp"));

        // Still present until the user says otherwise, and readable back.
        let id = out["id"].as_str().unwrap();
        assert!(db.doc_body(id).unwrap().unwrap().contains("Trang chạy"));
        assert_eq!(db.suspects(id).unwrap().len(), 1);
    }

    #[test]
    fn chunk_offsets_index_into_the_stored_body() {
        let db = Db::open_memory().unwrap();
        let mut src = String::new();
        for i in 0..6 {
            src.push_str(&format!(
                "# Chương {i}\n\nNội dung chương {i} viết đủ dài để tạo thành một mục riêng. {}\n\n",
                "chữ ".repeat(120)
            ));
        }
        let out = ingest(&db, "d.md", src.as_bytes(), "D").unwrap();
        let id = out["id"].as_str().unwrap();
        let body = db.doc_body(id).unwrap().unwrap();
        let chars: Vec<char> = body.chars().collect();
        for c in db.chunks_of_section(&db.sections_of(id).unwrap()[0].id).unwrap() {
            let span: String = chars[c.char_start as usize..c.char_end as usize].iter().collect();
            assert_eq!(corpus::squash_ws(&span), corpus::squash_ws(&c.text));
        }
    }
}
