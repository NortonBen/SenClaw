//! Document → sections → enriched sections.
//!
//! Two tiers, and the order matters. The **deterministic** tier finds the
//! document's own structure (markdown headings, "Chương 3", "1.2.4", shouted
//! title lines). Only then does the **LLM** tier describe each section it
//! found. A model asked to invent the table of contents produces a plausible
//! one that does not match the file; a model asked to summarise a span it was
//! handed cannot.
//!
//! Everything the planner later needs — `est_minutes`, `difficulty`,
//! `prerequisites` — is clamped against a length-derived baseline here, because
//! a single hallucinated "estMinutes": 240 turns into a study plan the learner
//! cannot finish and will abandon.

use serde_json::{json, Value};

use crate::corpus;
use crate::db::{Db, NewSection};
use crate::llm;

/// Sections shorter than this are merged away — a heading with two lines under
/// it is not a study session.
const MIN_SECTION_CHARS: usize = 500;
/// When a document has no headings at all, cut it into blocks about this long.
const FALLBACK_SECTION_CHARS: usize = 3_000;
/// Upper bound on sections per document. Hitting it is reported, never silent.
pub const MAX_SECTIONS: usize = 400;
/// Characters of a section shown to the model when enriching.
const ENRICH_CHARS: usize = 2_600;
/// Sections per enrichment call. Small batches keep `finish == "length"` away.
const ENRICH_BATCH: usize = 4;

/// Vietnamese/English chapter words that start a top-level section.
const CHAPTER_WORDS: &[&str] = &[
    "chương", "phần", "bài", "mục", "chuyên đề", "chapter", "part", "section", "unit", "lesson",
    "module",
];

/// Does this line look like a heading? Returns its level (1 = biggest).
pub fn heading_level(line: &str) -> Option<i64> {
    let t = line.trim();
    if t.is_empty() || t.chars().count() > 120 {
        return None;
    }

    // Markdown.
    if let Some(rest) = t.strip_prefix('#') {
        let extra = rest.chars().take_while(|c| *c == '#').count();
        let after = rest.trim_start_matches('#');
        if after.starts_with(' ') && !after.trim().is_empty() {
            return Some((extra as i64 + 1).min(6));
        }
    }

    let lower = corpus::fold(t);

    // "Chương 3", "PHẦN II — …", "Bài 12:"
    for w in CHAPTER_WORDS {
        let w = corpus::fold(w);
        if let Some(rest) = lower.strip_prefix(&w) {
            let rest = rest.trim_start();
            let first = rest.chars().next().unwrap_or(' ');
            if first.is_ascii_digit() || is_roman(rest) {
                return Some(1);
            }
        }
    }

    // "1.", "1.2", "2.3.4 Tiêu đề" — level follows the depth of the numbering.
    let head: String = t.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    if head.chars().any(|c| c.is_ascii_digit()) {
        let after = &t[head.len()..];
        let depth = head.trim_end_matches('.').split('.').count() as i64;
        if (after.starts_with(' ') || after.starts_with('\t') || after.is_empty())
            && !after.trim().is_empty()
            && depth <= 5
        {
            return Some((depth + 1).min(6));
        }
    }

    // A shouted line with no sentence punctuation.
    let letters: Vec<char> = t.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() >= 3
        && letters.iter().all(|c| c.is_uppercase())
        && !t.ends_with('.')
        && t.chars().count() <= 80
    {
        return Some(2);
    }

    None
}

fn is_roman(s: &str) -> bool {
    let head: String = s
        .chars()
        .take_while(|c| "ivxlcdm".contains(c.to_ascii_lowercase()))
        .collect();
    !head.is_empty() && head.chars().count() <= 7
}

/// Line spans (char offsets) of `text`, with the line content.
fn lines_with_offsets(text: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut cur = String::new();
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            out.push((start, i, std::mem::take(&mut cur)));
            start = i + 1;
        } else {
            cur.push(c);
        }
    }
    let total = text.chars().count();
    if start <= total {
        out.push((start, total, cur));
    }
    out
}

/// Cut a document into sections using its own structure.
///
/// Returns the sections plus a human-readable note about anything that had to
/// be capped — a plan built on a truncated outline must say so.
pub fn outline(text: &str) -> (Vec<NewSection>, Option<String>) {
    let total = text.chars().count();
    if total == 0 {
        return (vec![], None);
    }
    let lines = lines_with_offsets(text);

    let mut raw: Vec<NewSection> = Vec::new();
    for (start, _end, content) in &lines {
        if let Some(level) = heading_level(content) {
            if let Some(prev) = raw.last_mut() {
                prev.char_end = *start;
            }
            raw.push(NewSection {
                title: content.trim().trim_start_matches('#').trim().to_string(),
                level,
                char_start: *start,
                char_end: total,
            });
        }
    }

    // Text before the first heading is its own section, not lost.
    if let Some(first) = raw.first() {
        if first.char_start > MIN_SECTION_CHARS {
            raw.insert(
                0,
                NewSection {
                    title: "Mở đầu".to_string(),
                    level: 1,
                    char_start: 0,
                    char_end: first.char_start,
                },
            );
        } else if first.char_start > 0 {
            raw[0].char_start = 0;
        }
    }

    let mut sections = if raw.is_empty() {
        fallback_split(text)
    } else {
        merge_tiny(raw)
    };

    let mut note = None;
    if sections.len() > MAX_SECTIONS {
        note = Some(format!(
            "tài liệu có {} mục, đã gộp còn {MAX_SECTIONS} mục lớn nhất để lập lịch — \
             chia nhỏ tệp nếu muốn học chi tiết hơn",
            sections.len()
        ));
        sections = coalesce_to(sections, MAX_SECTIONS);
    }
    (sections, note)
}

/// No headings anywhere: cut on paragraph boundaries into even blocks.
fn fallback_split(text: &str) -> Vec<NewSection> {
    let spans = corpus::paragraph_spans(text);
    let total = text.chars().count();
    if spans.is_empty() {
        return vec![NewSection {
            title: "Toàn văn".into(),
            level: 1,
            char_start: 0,
            char_end: total,
        }];
    }
    let mut out: Vec<NewSection> = Vec::new();
    let mut start = spans[0].0;
    let mut end = spans[0].1;
    for (ps, pe) in spans.iter().skip(1) {
        if pe - start > FALLBACK_SECTION_CHARS {
            out.push(NewSection {
                title: format!("Phần {}", out.len() + 1),
                level: 1,
                char_start: start,
                char_end: end,
            });
            start = *ps;
        }
        end = *pe;
    }
    out.push(NewSection {
        title: format!("Phần {}", out.len() + 1),
        level: 1,
        char_start: start,
        char_end: total,
    });
    out
}

/// Fold sections that are too short to be a study unit into a neighbour.
///
/// A short parent heading merges *forward* (it is the title of what follows);
/// a short trailing section merges *backward*.
fn merge_tiny(list: Vec<NewSection>) -> Vec<NewSection> {
    let mut out: Vec<NewSection> = Vec::new();
    let mut pending: Option<NewSection> = None;
    for s in list {
        let mut s = s;
        if let Some(p) = pending.take() {
            // Keep the shallower (more important) title.
            let title = if p.level <= s.level { p.title } else { s.title };
            s = NewSection {
                title,
                level: p.level.min(s.level),
                char_start: p.char_start,
                char_end: s.char_end,
            };
        }
        if s.char_end.saturating_sub(s.char_start) < MIN_SECTION_CHARS {
            pending = Some(s);
        } else {
            out.push(s);
        }
    }
    if let Some(p) = pending {
        match out.last_mut() {
            Some(last) => last.char_end = p.char_end,
            None => out.push(p),
        }
    }
    out
}

/// Merge neighbouring sections until at most `max` remain.
fn coalesce_to(list: Vec<NewSection>, max: usize) -> Vec<NewSection> {
    if list.len() <= max {
        return list;
    }
    let group = list.len().div_ceil(max);
    let mut out = Vec::new();
    for batch in list.chunks(group) {
        let first = &batch[0];
        let last = &batch[batch.len() - 1];
        out.push(NewSection {
            title: first.title.clone(),
            level: first.level,
            char_start: first.char_start,
            char_end: last.char_end,
        });
    }
    out
}

/// Minutes a section is expected to take from its length alone.
///
/// ~1,000 characters of Vietnamese prose ≈ 180 words ≈ 1.2 minutes of reading;
/// studying (not skimming) runs roughly 4× that, plus a fixed cost per section.
pub fn baseline_minutes(chars: usize) -> i64 {
    let m = 3.0 + (chars as f64 / 1_000.0) * 5.0;
    (m.round() as i64).clamp(3, 90)
}

/// Outline a stored document and rebuild its chunk index.
///
/// Deterministic and offline — no LLM. Safe to re-run at any time; it is the
/// repair path when an outline looks wrong.
pub fn index_document(db: &Db, doc_id: &str) -> Result<(usize, usize, Option<String>), String> {
    let body = db
        .doc_body(doc_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "không tìm thấy tài liệu".to_string())?;

    let (secs, note) = outline(&body);
    let ids = db
        .sections_replace(doc_id, &secs)
        .map_err(|e| e.to_string())?;
    let bounds: Vec<(usize, usize, String)> = secs
        .iter()
        .zip(ids.iter())
        .map(|(s, id)| (s.char_start, s.char_end, id.clone()))
        .collect();

    let chunks = corpus::chunk(&body);
    let n = db
        .chunks_replace(doc_id, &chunks, |ch| {
            // Midpoint, not start: the overlap prefix belongs to the previous
            // section, and the chunk's substance is where its middle lands.
            let mid = ch.char_start + (ch.char_end - ch.char_start) / 2;
            bounds
                .iter()
                .find(|(a, b, _)| mid >= *a && mid < *b)
                .map(|(_, _, id)| id.clone())
        })
        .map_err(|e| e.to_string())?;

    db.doc_set_status(doc_id, "outlined", None)
        .map_err(|e| e.to_string())?;
    Ok((secs.len(), n, note))
}

/// Whole-document synthesis, written from the section summaries rather than
/// from the raw text — the sections are already grounded, and a 300-page book
/// does not fit in one prompt.
pub async fn summarize_document(db: &Db, doc_id: &str) -> Result<String, String> {
    let secs = db.sections_of(doc_id).map_err(|e| e.to_string())?;
    if secs.is_empty() {
        return Err("tài liệu chưa được chia mục".into());
    }
    let outline_text = secs
        .iter()
        .map(|s| {
            format!(
                "- {} ({} phút, độ khó {}): {}",
                s.title,
                s.est_minutes,
                s.difficulty,
                s.summary.clone().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Dưới đây là dàn ý một tài liệu học. Viết phần TỔNG HỢP bằng tiếng Việt gồm:\n\
         1) 3-5 câu tài liệu này nói về cái gì;\n\
         2) mục \"Học xong bạn làm được gì\" dạng gạch đầu dòng;\n\
         3) mục \"Nên học theo thứ tự\" nêu 3-6 chặng.\n\
         CHỈ dùng thông tin trong dàn ý. Trả về Markdown thuần.\n\nDÀN Ý:\n{}",
        corpus::head(&outline_text, 12_000)
    );
    let (text, finish) = llm::bridge_llm(
        "Bạn là chuyên viên thiết kế chương trình học. Chỉ dùng thông tin được cung cấp.",
        &prompt,
        4_000,
    )
    .await?;
    if finish == "length" {
        return Err("model cắt output giữa chừng khi tổng hợp tài liệu".into());
    }
    db.doc_set_summary(doc_id, text.trim())
        .map_err(|e| e.to_string())?;
    Ok(text.trim().to_string())
}

// ── LLM enrichment ──────────────────────────────────────────────────────────

const ENRICH_SYSTEM: &str = "Bạn là chuyên viên thiết kế chương trình học. \
Bạn nhận các MỤC đã được cắt sẵn từ một tài liệu và mô tả từng mục. \
Bạn KHÔNG được bịa nội dung không có trong mục. \
Trả về JSON THUẦN, không kèm lời dẫn, không bọc trong khối mã.";

/// Enrich every section of a document that hasn't been enriched yet.
///
/// Returns `(enriched, skipped_note)`. A failed batch is reported, not
/// swallowed: the sections keep their length-derived defaults so the planner
/// still works, and the caller can tell the user which part is un-described.
pub async fn enrich_document(db: &Db, doc_id: &str) -> Result<(usize, Vec<String>), String> {
    let body = db
        .doc_body(doc_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "không tìm thấy tài liệu".to_string())?;
    let chars: Vec<char> = body.chars().collect();
    let sections = db.sections_of(doc_id).map_err(|e| e.to_string())?;
    let titles: Vec<(String, String)> = sections
        .iter()
        .map(|s| (s.id.clone(), s.title.clone()))
        .collect();

    let mut done = 0usize;
    let mut problems = Vec::new();

    for batch in sections.chunks(ENRICH_BATCH) {
        let mut payload = Vec::new();
        for s in batch {
            let a = (s.char_start as usize).min(chars.len());
            let b = (s.char_end as usize).min(chars.len()).max(a);
            let text: String = chars[a..b].iter().collect();
            payload.push(json!({
                "id": s.id,
                "title": s.title,
                "text": corpus::head(&text, ENRICH_CHARS),
            }));
        }
        let prompt = format!(
            "Với MỖI mục dưới đây, trả về một object:\n\
             {{ \"id\": \"<id đúng như đầu vào>\", \"summary\": \"2-4 câu tóm tắt\", \
             \"keyPoints\": [\"ý chính\", …tối đa 6], \"concepts\": [\"khái niệm\", …tối đa 8], \
             \"difficulty\": 1-5, \"estMinutes\": số phút học thực tế, \
             \"prerequisites\": [\"tiêu đề mục cần học trước\", …] }}\n\
             Chỉ dùng tiêu đề mục có trong danh sách sau làm prerequisites: {}\n\
             Trả về MẢNG JSON đúng {} phần tử.\n\nCÁC MỤC:\n{}",
            titles
                .iter()
                .map(|(_, t)| format!("\"{t}\""))
                .collect::<Vec<_>>()
                .join(", "),
            batch.len(),
            serde_json::to_string(&payload).unwrap_or_default()
        );

        match llm::ask_json_array(ENRICH_SYSTEM, &prompt, 8_000, "mô tả các mục tài liệu").await {
            Ok(items) => {
                for item in items {
                    let id = item["id"].as_str().unwrap_or("").to_string();
                    let Some(sec) = batch.iter().find(|s| s.id == id) else {
                        // A model that renames ids gets ignored rather than
                        // writing a description onto the wrong section.
                        continue;
                    };
                    let len = (sec.char_end - sec.char_start).max(0) as usize;
                    let base = baseline_minutes(len);
                    let est = item["estMinutes"]
                        .as_i64()
                        .unwrap_or(base)
                        .clamp((base / 2).max(3), base * 2);
                    let key_points = str_list(&item["keyPoints"], 6);
                    let concepts = str_list(&item["concepts"], 8);
                    let prereq = resolve_prereq(&item["prerequisites"], &titles, &sec.id);
                    let summary = item["summary"].as_str().unwrap_or("").trim().to_string();

                    db.section_enrich(
                        &sec.id,
                        &summary,
                        &key_points,
                        item["difficulty"].as_i64().unwrap_or(3),
                        est,
                        &prereq,
                    )
                    .map_err(|e| e.to_string())?;

                    for c in concepts {
                        if let Ok(cid) = db.concept_upsert(doc_id, &c) {
                            let _ = db.concept_link(&cid, &sec.id);
                        }
                    }
                    done += 1;
                }
            }
            Err(e) => problems.push(format!(
                "mục {}–{}: {e}",
                batch[0].ord + 1,
                batch[batch.len() - 1].ord + 1
            )),
        }
    }

    Ok((done, problems))
}

fn str_list(v: &Value, max: usize) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .take(max)
                .collect()
        })
        .unwrap_or_default()
}

/// Map prerequisite *titles* back to section ids.
///
/// Titles, not ids: a model handed a list of uuids reliably invents new ones,
/// and an invented prerequisite id silently drops out of the topological sort.
/// A self-reference or a forward reference is dropped — a prerequisite that
/// comes later cannot be one.
fn resolve_prereq(v: &Value, titles: &[(String, String)], self_id: &str) -> Vec<String> {
    let self_pos = titles.iter().position(|(id, _)| id == self_id);
    let mut out = Vec::new();
    for name in str_list(v, 6) {
        let n = corpus::fold(&name);
        if let Some(pos) = titles
            .iter()
            .position(|(_, t)| corpus::fold(t) == n || corpus::fold(t).contains(&n))
        {
            let (id, _) = &titles[pos];
            if id == self_id {
                continue;
            }
            if let Some(sp) = self_pos {
                if pos > sp {
                    continue;
                }
            }
            if !out.contains(id) {
                out.push(id.clone());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_headings_are_detected_with_their_depth() {
        assert_eq!(heading_level("# Giới thiệu"), Some(1));
        assert_eq!(heading_level("### Chi tiết"), Some(3));
        assert_eq!(heading_level("#nothashtag"), None);
    }

    #[test]
    fn vietnamese_chapter_lines_are_headings() {
        assert_eq!(heading_level("Chương 1: Tổng quan"), Some(1));
        assert_eq!(heading_level("PHẦN II — Kỹ thuật"), Some(1));
        assert_eq!(heading_level("Bài 12"), Some(1));
    }

    #[test]
    fn numbered_headings_take_their_depth_from_the_numbering() {
        assert_eq!(heading_level("1. Mở đầu"), Some(2));
        assert_eq!(heading_level("2.3.4 Chi tiết"), Some(4));
    }

    #[test]
    fn a_normal_sentence_is_not_a_heading() {
        assert_eq!(heading_level("Đây là một câu bình thường trong đoạn văn."), None);
        assert_eq!(heading_level(""), None);
    }

    #[test]
    fn sections_tile_the_document_without_gaps() {
        let body = format!(
            "# A\n\n{}\n\n# B\n\n{}",
            "nội dung a ".repeat(80),
            "nội dung b ".repeat(80)
        );
        let (secs, note) = outline(&body);
        assert!(note.is_none());
        assert_eq!(secs.len(), 2);
        assert_eq!(secs[0].char_start, 0);
        assert_eq!(secs[0].char_end, secs[1].char_start);
        assert_eq!(secs[1].char_end, body.chars().count());
    }

    #[test]
    fn a_document_with_no_headings_still_becomes_sections() {
        let body = (0..200)
            .map(|i| format!("Đoạn {i} với khá nhiều chữ để dài ra một chút."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let (secs, _) = outline(&body);
        assert!(secs.len() > 1, "a long unstructured doc must still split");
        assert_eq!(secs[0].char_start, 0);
    }

    #[test]
    fn a_bare_heading_merges_into_the_content_that_follows_it() {
        let body = format!("# Chương 1\n\n## 1.1 Nội dung\n\n{}", "chữ ".repeat(400));
        let (secs, _) = outline(&body);
        assert_eq!(secs.len(), 1, "a heading with no body is not a session");
        assert_eq!(
            secs[0].title, "Chương 1",
            "the shallower title survives the merge"
        );
    }

    #[test]
    fn text_before_the_first_heading_is_not_lost() {
        let body = format!("{}\n\n# Chương 1\n\n{}", "lời nói đầu ".repeat(80), "nội dung ".repeat(80));
        let (secs, _) = outline(&body);
        assert_eq!(secs[0].char_start, 0);
        assert_eq!(secs[0].title, "Mở đầu");
    }

    #[test]
    fn too_many_sections_are_coalesced_and_reported() {
        let body = (0..MAX_SECTIONS + 50)
            .map(|i| format!("# Mục {i}\n\n{}", "chữ ".repeat(200)))
            .collect::<Vec<_>>()
            .join("\n\n");
        let (secs, note) = outline(&body);
        assert!(secs.len() <= MAX_SECTIONS);
        assert!(note.is_some(), "a cap the user can't see is a lie");
    }

    #[test]
    fn baseline_minutes_scale_with_length_and_stay_sane() {
        assert!(baseline_minutes(0) >= 3);
        assert!(baseline_minutes(2_000) > baseline_minutes(500));
        assert!(baseline_minutes(10_000_000) <= 90);
    }

    #[test]
    fn prerequisites_resolve_by_title_and_drop_forward_references() {
        let titles = vec![
            ("s1".to_string(), "Cơ bản".to_string()),
            ("s2".to_string(), "Nâng cao".to_string()),
        ];
        let got = resolve_prereq(&json!(["Cơ bản"]), &titles, "s2");
        assert_eq!(got, vec!["s1".to_string()]);

        // s1 cannot depend on s2, which comes after it.
        let forward = resolve_prereq(&json!(["Nâng cao"]), &titles, "s1");
        assert!(forward.is_empty());

        // Self-reference and unknown titles are dropped.
        assert!(resolve_prereq(&json!(["Cơ bản"]), &titles, "s1").is_empty());
        assert!(resolve_prereq(&json!(["Không có"]), &titles, "s2").is_empty());
    }
}
