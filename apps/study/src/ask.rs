//! Answering questions about the learner's material, with citations that
//! resolve.
//!
//! The citation contract, borrowed wholesale from `apps/zeach`:
//!
//! * `[n]` is the **1-based position in the evidence list returned to the
//!   caller**. The numbering is computed here, in code; the model is told the
//!   numbers, never asked to invent them.
//! * A citation pointing past the end of the list is **removed** from the
//!   answer before it is shown. A number the reader cannot resolve is worse
//!   than no number: it reads as verified.
//! * Every internal piece of evidence carries `docId` + `charStart..charEnd`,
//!   so clicking `[3]` scrolls to the paragraph rather than to the file.
//!
//! And the degradation rule: if the bridge fails, or truncates
//! (`finish == "length"`), the caller gets a **deterministically assembled**
//! answer built from the retrieved passages — never an empty response and never
//! a half-written one.

use std::collections::HashMap;

use serde_json::{json, Value};

use crate::corpus;
use crate::db::Db;
use crate::llm;

/// How many passages reach the prompt.
const MAX_EVIDENCE: usize = 12;
/// Characters of each passage shown to the model.
const PER_ITEM_CHARS: usize = 700;
/// At most this many passages from any one section, so a single long section
/// cannot crowd out the rest of the answer.
const PER_SECTION_CAP: usize = 2;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    /// Stable within one answer; `[n]` is its 1-based position.
    pub id: String,
    /// `doc` (the learner's own material) or `external`.
    pub kind: String,
    pub title: String,
    pub text: String,
    pub doc_id: Option<String>,
    pub section_id: Option<String>,
    pub chunk_id: Option<i64>,
    pub char_start: Option<i64>,
    pub char_end: Option<i64>,
    pub url: Option<String>,
    pub source: Option<String>,
}

/// Reciprocal-rank fusion constant. 60 is the value the original RRF paper
/// used and what `apps/search` uses, so scores stay comparable across apps.
const RRF_K: f64 = 60.0;

/// Fuse several ranked lists of chunk ids into one ranking.
pub fn rrf(rankings: &[Vec<i64>]) -> Vec<i64> {
    let mut score: HashMap<i64, f64> = HashMap::new();
    let mut first_seen: HashMap<i64, usize> = HashMap::new();
    for list in rankings {
        for (rank, id) in list.iter().enumerate() {
            *score.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + rank as f64 + 1.0);
            first_seen.entry(*id).or_insert(rank);
        }
    }
    let mut ids: Vec<i64> = score.keys().copied().collect();
    ids.sort_by(|a, b| {
        score[b]
            .partial_cmp(&score[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            // Deterministic tie-break, so the same query always numbers the
            // same evidence the same way.
            .then(first_seen[a].cmp(&first_seen[b]))
            .then(a.cmp(b))
    });
    ids
}

/// Retrieve passages for a question from the learner's own documents.
///
/// Runs the full query and each significant token as separate retrievals and
/// fuses them: a long natural-language question often matches nothing as a
/// whole but matches well on its content words.
pub fn retrieve(
    db: &Db,
    question: &str,
    doc_ids: &[String],
    limit: usize,
) -> Result<Vec<Evidence>, String> {
    let mut rankings: Vec<Vec<i64>> = Vec::new();
    let mut by_id: HashMap<i64, crate::db::ChunkRow> = HashMap::new();

    let run = |q: &str, rankings: &mut Vec<Vec<i64>>, by_id: &mut HashMap<i64, _>| {
        if let Ok(hits) = db.search_chunks(q, doc_ids, 30) {
            if !hits.is_empty() {
                rankings.push(hits.iter().map(|(c, _)| c.id).collect());
                for (c, _) in hits {
                    by_id.entry(c.id).or_insert(c);
                }
            }
        }
    };

    run(question, &mut rankings, &mut by_id);
    for tok in significant_tokens(question) {
        run(&tok, &mut rankings, &mut by_id);
    }

    let fused = rrf(&rankings);
    let mut per_section: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();

    for id in fused {
        if out.len() >= limit.min(MAX_EVIDENCE) {
            break;
        }
        let Some(c) = by_id.get(&id) else { continue };
        // Diversity: several passages from one section usually say the same
        // thing twice and starve the rest of the answer.
        let key = c.section_id.clone().unwrap_or_else(|| c.doc_id.clone());
        let n = per_section.entry(key).or_insert(0);
        if *n >= PER_SECTION_CAP {
            continue;
        }
        *n += 1;

        let title = c
            .section_id
            .as_deref()
            .and_then(|s| db.section_get(s).ok().flatten())
            .map(|s| s.title)
            .unwrap_or_else(|| "Tài liệu".to_string());
        out.push(Evidence {
            id: format!("c{}", c.id),
            kind: "doc".into(),
            title,
            text: c.text.clone(),
            doc_id: Some(c.doc_id.clone()),
            section_id: c.section_id.clone(),
            chunk_id: Some(c.id),
            char_start: Some(c.char_start),
            char_end: Some(c.char_end),
            url: None,
            source: None,
        });
    }
    Ok(out)
}

/// Content words worth searching on their own.
fn significant_tokens(q: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "la", "gi", "the", "nao", "cua", "va", "co", "khong", "cho", "voi", "trong", "tai", "sao",
        "nhu", "mot", "cac", "nhung", "duoc", "thi", "ma", "de", "hay", "what", "is", "the", "how",
        "why", "does", "do", "of", "and", "a", "an", "to", "in", "for",
    ];
    let folded = corpus::fold(q);
    let mut out: Vec<String> = Vec::new();
    for t in folded.split(|c: char| !c.is_alphanumeric()) {
        let t = t.trim();
        if t.chars().count() < 3 || STOP.contains(&t) || out.iter().any(|x| x == t) {
            continue;
        }
        out.push(t.to_string());
        if out.len() >= 6 {
            break;
        }
    }
    out
}

/// Map every evidence id to its 1-based citation number.
pub fn number_evidence(evidence: &[Evidence]) -> HashMap<String, usize> {
    evidence
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id.clone(), i + 1))
        .collect()
}

/// Remove `[n]` markers that point past the evidence list.
///
/// Models cite numbers that do not exist. Leaving one in place hands the reader
/// a claim that looks sourced and cannot be checked.
pub fn strip_bad_citations(answer: &str, max_n: usize) -> (String, usize) {
    let mut out = String::with_capacity(answer.len());
    let mut removed = 0usize;
    let chars: Vec<char> = answer.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '[' {
            let mut j = i + 1;
            let mut num = String::new();
            while j < chars.len() && chars[j].is_ascii_digit() {
                num.push(chars[j]);
                j += 1;
            }
            if !num.is_empty() && j < chars.len() && chars[j] == ']' {
                let n: usize = num.parse().unwrap_or(0);
                if n >= 1 && n <= max_n {
                    out.push('[');
                    out.push_str(&num);
                    out.push(']');
                } else {
                    removed += 1;
                }
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    (out, removed)
}

const SYSTEM: &str = "Bạn là trợ giảng. Bạn CHỈ được dùng các ĐOẠN TRÍCH được đánh số bên dưới — \
tuyệt đối không thêm kiến thức ngoài. Mỗi nhận định phải dẫn nguồn bằng [n] đúng số của đoạn trích. \
Nếu các đoạn trích không đủ để trả lời, hãy nói thẳng là tài liệu không đề cập. \
Đoạn nào được đánh dấu NGUỒN NGOÀI thì khi dùng phải nói rõ đó là nguồn ngoài, chưa có trong tài liệu của người học. \
Trả lời bằng tiếng Việt, Markdown thuần, không bọc khối mã.";

/// Build the prompt block: numbered, truncated evidence.
fn evidence_block(evidence: &[Evidence]) -> String {
    evidence
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let tag = if e.kind == "external" {
                format!("NGUỒN NGOÀI · {}", e.source.clone().unwrap_or_default())
            } else {
                format!("TÀI LIỆU · {}", e.title)
            };
            format!(
                "[{}] ({tag})\n{}",
                i + 1,
                corpus::head(&e.text, PER_ITEM_CHARS)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// A mechanically assembled answer — the floor this feature never falls below.
pub fn assemble(question: &str, evidence: &[Evidence]) -> String {
    if evidence.is_empty() {
        return format!("Không tìm thấy đoạn nào trong tài liệu liên quan tới: **{question}**.");
    }
    let mut s = format!(
        "*(Chưa tổng hợp được bằng AI — dưới đây là các đoạn liên quan nhất trong tài liệu.)*\n\n\
         **{question}**\n\n"
    );
    for (i, e) in evidence.iter().enumerate() {
        s.push_str(&format!(
            "- [{}] **{}** — {}\n",
            i + 1,
            e.title,
            corpus::head(&corpus::squash_ws(&e.text), 240)
        ));
    }
    s
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    pub answer_md: String,
    pub evidence: Vec<Evidence>,
    pub degraded: bool,
    pub notes: Vec<String>,
}

/// Synthesize an answer over already-retrieved evidence.
pub async fn synthesize(question: &str, evidence: Vec<Evidence>) -> Answer {
    let mut notes = Vec::new();
    if evidence.is_empty() {
        return Answer {
            answer_md: assemble(question, &evidence),
            evidence,
            degraded: true,
            notes: vec!["không có đoạn nào khớp câu hỏi".into()],
        };
    }

    let prompt = format!(
        "CÂU HỎI: {question}\n\nCÁC ĐOẠN TRÍCH:\n{}\n\n\
         Viết câu trả lời ngắn gọn, mỗi ý dẫn [n].",
        evidence_block(&evidence)
    );

    match llm::bridge_llm(SYSTEM, &prompt, 4_000).await {
        Ok((text, finish)) if finish != "length" && !text.trim().is_empty() => {
            let (clean, removed) = strip_bad_citations(text.trim(), evidence.len());
            if removed > 0 {
                notes.push(format!(
                    "đã bỏ {removed} trích dẫn trỏ tới đoạn không tồn tại"
                ));
            }
            Answer {
                answer_md: clean,
                evidence,
                degraded: false,
                notes,
            }
        }
        Ok((_, finish)) => {
            notes.push(if finish == "length" {
                "model cắt output giữa chừng — trả về bản ghép cơ học".into()
            } else {
                "model trả lời rỗng — trả về bản ghép cơ học".into()
            });
            Answer {
                answer_md: assemble(question, &evidence),
                evidence,
                degraded: true,
                notes,
            }
        }
        Err(e) => {
            notes.push(format!("AI lỗi ({e}) — trả về bản ghép cơ học"));
            Answer {
                answer_md: assemble(question, &evidence),
                evidence,
                degraded: true,
                notes,
            }
        }
    }
}

/// Ask over the learner's documents only.
pub async fn ask(db: &Db, question: &str, doc_ids: &[String]) -> Result<Value, String> {
    let ev = retrieve(db, question, doc_ids, MAX_EVIDENCE)?;
    let ans = synthesize(question, ev).await;
    let scope = json!({ "docIds": doc_ids });
    let ev_json = serde_json::to_value(&ans.evidence).unwrap_or(Value::Null);
    let citations = number_evidence(&ans.evidence);
    let id = db
        .ask_insert(question, &scope, &ans.answer_md, &ev_json, false)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "id": id,
        "question": question,
        "answerMd": ans.answer_md,
        "evidence": ev_json,
        // evidence id → the [n] it is cited as, so a UI can resolve a marker
        // without re-deriving the numbering and getting it subtly wrong.
        "citations": citations,
        "degraded": ans.degraded,
        "notes": ans.notes,
        "external": false,
    }))
}

/// Ask over the learner's documents **and** whatever lookup MCPs are running.
///
/// External hits are appended after the internal ones and clearly labelled, so
/// the numbering puts the learner's own material first and the answer can say
/// which parts came from outside the syllabus.
pub async fn research(
    db: &Db,
    question: &str,
    doc_ids: &[String],
    source_setting: &str,
) -> Result<Value, String> {
    let mut ev = retrieve(db, question, doc_ids, 8)?;
    let mut notes = Vec::new();

    let all = crate::sources::discover().await;
    let picked = crate::sources::select(&all, source_setting, 2);
    let gathered =
        crate::sources::gather(&picked, &[question.to_string()], 5).await;
    notes.push(gathered.note.clone());
    if !gathered.filtered.is_empty() {
        notes.push(format!(
            "đã loại {} dòng mang dạng câu lệnh trong kết quả nguồn ngoài (không thi hành): {}",
            gathered.filtered.len(),
            gathered.filtered.join(" | ")
        ));
    }

    for (i, it) in gathered.items.iter().enumerate() {
        ev.push(Evidence {
            id: format!("x{i}"),
            kind: "external".into(),
            title: it["title"].as_str().unwrap_or("(không tiêu đề)").to_string(),
            text: it["snippet"].as_str().unwrap_or("").to_string(),
            doc_id: None,
            section_id: None,
            chunk_id: None,
            char_start: None,
            char_end: None,
            url: it["url"].as_str().filter(|u| !u.is_empty()).map(str::to_string),
            source: it["source"].as_str().map(str::to_string),
        });
    }

    let mut ans = synthesize(question, ev).await;
    ans.notes.extend(notes);
    let scope = json!({ "docIds": doc_ids, "sources": source_setting });
    let ev_json = serde_json::to_value(&ans.evidence).unwrap_or(Value::Null);
    let citations = number_evidence(&ans.evidence);
    let id = db
        .ask_insert(question, &scope, &ans.answer_md, &ev_json, true)
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "id": id,
        "question": question,
        "answerMd": ans.answer_md,
        "evidence": ev_json,
        "citations": citations,
        "degraded": ans.degraded,
        "notes": ans.notes,
        "external": true,
        "sourcesAvailable": all.iter().map(|s| s.to_json()).collect::<Vec<_>>(),
        "sourcesUsed": picked.iter().map(|s| s.key()).collect::<Vec<_>>(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(id: &str, kind: &str) -> Evidence {
        Evidence {
            id: id.into(),
            kind: kind.into(),
            title: "T".into(),
            text: "nội dung".into(),
            doc_id: Some("d".into()),
            section_id: Some("s".into()),
            chunk_id: Some(1),
            char_start: Some(0),
            char_end: Some(10),
            url: None,
            source: None,
        }
    }

    fn seeded() -> (Db, String) {
        let db = Db::open_memory().unwrap();
        let body = "# Lãi suất\n\nLãi suất điều hành do Ngân hàng Nhà nước công bố định kỳ.\n\n\
                    # Tỷ giá\n\nTỷ giá trung tâm được Ngân hàng Nhà nước công bố mỗi ngày làm việc.";
        let doc = db.doc_insert("KT", "kt.md", "md", 1, "ok", body).unwrap();
        crate::outline::index_document(&db, &doc).unwrap();
        (db, doc)
    }

    #[test]
    fn citation_numbers_are_positions_in_the_returned_list() {
        let list = vec![ev("c9", "doc"), ev("x0", "external")];
        let map = number_evidence(&list);
        assert_eq!(map["c9"], 1);
        assert_eq!(map["x0"], 2);
    }

    #[test]
    fn citations_past_the_end_of_the_list_are_removed() {
        let (clean, removed) = strip_bad_citations("Theo tài liệu [1], và [9] nữa.", 2);
        assert!(clean.contains("[1]"));
        assert!(!clean.contains("[9]"));
        assert_eq!(removed, 1);
    }

    #[test]
    fn ordinary_bracketed_text_is_not_mistaken_for_a_citation() {
        let (clean, removed) = strip_bad_citations("Ghi chú [xem thêm] và [12a].", 3);
        assert_eq!(clean, "Ghi chú [xem thêm] và [12a].");
        assert_eq!(removed, 0);
    }

    #[test]
    fn zero_is_not_a_valid_citation() {
        let (clean, removed) = strip_bad_citations("Sai [0].", 3);
        assert!(!clean.contains("[0]"));
        assert_eq!(removed, 1);
    }

    #[test]
    fn rrf_prefers_passages_that_several_retrievals_agree_on() {
        // 7 is second in both lists; 1 and 2 each top exactly one.
        let fused = rrf(&[vec![1, 7, 3], vec![2, 7, 4]]);
        assert_eq!(fused[0], 7, "agreement beats a single first place");
    }

    #[test]
    fn rrf_is_deterministic_for_the_same_input() {
        let a = rrf(&[vec![1, 2, 3], vec![3, 2, 1]]);
        let b = rrf(&[vec![1, 2, 3], vec![3, 2, 1]]);
        assert_eq!(a, b, "the same query must number evidence the same way");
    }

    #[test]
    fn retrieval_finds_the_relevant_section_and_keeps_its_offsets() {
        let (db, doc) = seeded();
        let ev = retrieve(&db, "ai công bố tỷ giá trung tâm?", &[doc.clone()], 5).unwrap();
        assert!(!ev.is_empty());
        let top = &ev[0];
        assert_eq!(top.doc_id.as_deref(), Some(doc.as_str()));
        assert!(top.char_end.unwrap() > top.char_start.unwrap());
        assert!(top.text.to_lowercase().contains("tỷ giá"));
    }

    #[test]
    fn a_question_with_no_match_returns_no_evidence_rather_than_noise() {
        let (db, doc) = seeded();
        let ev = retrieve(&db, "zzzz qqqq wwww", &[doc], 5).unwrap();
        assert!(ev.is_empty());
    }

    #[test]
    fn no_single_section_can_monopolise_the_evidence_list() {
        let db = Db::open_memory().unwrap();
        let long = format!("# Một mục\n\n{}", "lãi suất điều hành rất quan trọng. ".repeat(400));
        let doc = db.doc_insert("X", "x.md", "md", 1, "ok", &long).unwrap();
        crate::outline::index_document(&db, &doc).unwrap();
        let ev = retrieve(&db, "lãi suất", &[doc], 12).unwrap();
        assert!(ev.len() <= PER_SECTION_CAP, "got {} passages", ev.len());
    }

    #[test]
    fn the_assembled_fallback_still_carries_resolvable_numbers() {
        let out = assemble("Câu hỏi?", &[ev("c1", "doc"), ev("c2", "doc")]);
        assert!(out.contains("[1]"));
        assert!(out.contains("[2]"));
        let (_, removed) = strip_bad_citations(&out, 2);
        assert_eq!(removed, 0);
    }

    #[test]
    fn the_fallback_says_so_when_nothing_matched() {
        let out = assemble("Câu hỏi?", &[]);
        assert!(out.contains("Không tìm thấy"));
    }

    #[test]
    fn external_passages_are_labelled_in_the_prompt() {
        let mut x = ev("x0", "external");
        x.source = Some("news-mcp.news_search".into());
        let block = evidence_block(&[ev("c1", "doc"), x]);
        assert!(block.contains("[1] (TÀI LIỆU"));
        assert!(block.contains("[2] (NGUỒN NGOÀI · news-mcp.news_search"));
    }

    #[test]
    fn stopwords_do_not_become_search_tokens() {
        let toks = significant_tokens("Lãi suất điều hành là gì và tại sao?");
        assert!(toks.contains(&"lai".to_string()));
        assert!(!toks.contains(&"gi".to_string()));
        assert!(!toks.contains(&"va".to_string()));
    }
}
