//! Bytes → text → chunks that know where they came from.
//!
//! Ported from `apps/search/src/corpus.rs`, which owns the same three problems,
//! plus one this app adds:
//!
//! 1. **Extraction that admits failure.** A scanned PDF has no text layer. The
//!    honest outcome is an error naming the cause, not a document stored with
//!    empty content that answers every future query with silence.
//! 2. **Chunking.** A whole document is a bad retrieval unit; a sentence is too
//!    small to be evidence. Chunks are paragraph-aligned with overlap.
//! 3. **FTS5 query syntax.** A user's query is not an FTS5 expression. `giá
//!    "vàng" - SJC` is a syntax error, and `AND`/`OR`/`NEAR` are keywords. Raw
//!    interpolation is both a crash and an injection vector.
//! 4. **Offsets (new here).** Every chunk carries `[char_start, char_end)` into
//!    the stored document body, because a citation that only names a file is
//!    not a citation a learner can check. `[n]` must scroll to the paragraph.

use anyhow::{anyhow, bail, Result};

/// Extensions we can read. Anything else is refused by name rather than stored
/// as mojibake.
pub const SUPPORTED: &[&str] = &[
    "txt", "md", "markdown", "csv", "tsv", "json", "jsonl", "log", "html", "htm", "pdf", "docx",
];

#[derive(Debug)]
pub struct Extracted {
    pub text: String,
    /// What we did, in the user's terms — surfaced in the upload response.
    pub note: String,
}

pub fn extension_of(filename: &str) -> String {
    filename
        .rsplit('/')
        .next()
        .unwrap_or(filename)
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Tolerates a BOM and invalid UTF-8 rather than refusing a file over one bad
/// byte.
fn decode_text(bytes: &[u8]) -> String {
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(body).to_string()
}

pub fn extract(filename: &str, bytes: &[u8]) -> Result<Extracted> {
    let ext = extension_of(filename);
    if !SUPPORTED.contains(&ext.as_str()) {
        bail!(
            "chưa hỗ trợ đuôi `.{ext}` — hỗ trợ: {}",
            SUPPORTED.join(", ")
        );
    }
    let out = match ext.as_str() {
        "pdf" => from_pdf(bytes)?,
        "docx" => from_docx(bytes)?,
        "html" | "htm" => Extracted {
            text: strip_html(&decode_text(bytes)),
            note: "đã bóc thẻ HTML".into(),
        },
        _ => Extracted {
            text: collapse_blank_lines(&decode_text(bytes)),
            note: format!("đọc trực tiếp dạng .{ext}"),
        },
    };
    if out.text.trim().is_empty() {
        bail!("tệp không có nội dung văn bản nào đọc được");
    }
    Ok(out)
}

/// PDF text layer. A scan has none — that must be an error, because a document
/// stored with no text silently answers every future query with nothing.
fn from_pdf(bytes: &[u8]) -> Result<Extracted> {
    let text =
        pdf_extract::extract_text_from_mem(bytes).map_err(|e| anyhow!("đọc PDF lỗi: {e}"))?;
    let trimmed = text.trim();
    if trimmed.chars().count() < 20 {
        bail!(
            "PDF này không có lớp văn bản (nhiều khả năng là bản scan) — \
             hãy OCR trước (mcp__senclaw-ocr__ocr_*) rồi tải lên phần văn bản"
        );
    }
    Ok(Extracted {
        text: collapse_blank_lines(trimmed),
        note: "đã lấy lớp văn bản của PDF".into(),
    })
}

/// docx = a zip whose `word/document.xml` holds the text.
fn from_docx(bytes: &[u8]) -> Result<Extracted> {
    use quick_xml::events::Event;

    let reader = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| anyhow!("docx không phải zip: {e}"))?;
    let mut xml = String::new();
    {
        use std::io::Read;
        let mut f = zip
            .by_name("word/document.xml")
            .map_err(|_| anyhow!("docx thiếu word/document.xml"))?;
        f.read_to_string(&mut xml)
            .map_err(|e| anyhow!("đọc docx lỗi: {e}"))?;
    }

    let mut xr = quick_xml::Reader::from_str(&xml);
    xr.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut out = String::new();
    loop {
        match xr.read_event_into(&mut buf) {
            Ok(Event::Text(t)) => out.push_str(&t.unescape().unwrap_or_default()),
            // A docx paragraph is a blank-line boundary here: the chunker and
            // the outline both key off "\n\n", and a single newline would glue
            // every paragraph of a chapter into one unsplittable wall.
            Ok(Event::End(e)) if e.name().as_ref() == b"w:p" => out.push_str("\n\n"),
            Ok(Event::Empty(e)) if e.name().as_ref() == b"w:br" => out.push('\n'),
            Ok(Event::Eof) => break,
            Err(e) => bail!("docx xml lỗi: {e}"),
            _ => {}
        }
        buf.clear();
    }
    Ok(Extracted {
        text: collapse_blank_lines(&out),
        note: "đã bóc văn bản từ docx".into(),
    })
}

/// Remove `<tag>…</tag>` bodies entirely (case-insensitive).
fn drop_element(src: &str, tag: &str) -> String {
    let lower = src.to_lowercase();
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while let Some(rel) = lower[i..].find(&open) {
        let start = i + rel;
        out.push_str(&src[i..start]);
        match lower[start..].find(&close) {
            Some(rel_end) => i = start + rel_end + close.len(),
            // Unclosed tag: drop the rest rather than emit script source.
            None => return out,
        }
    }
    out.push_str(&src[i..]);
    out
}

fn strip_html(s: &str) -> String {
    let without = drop_element(&drop_element(s, "script"), "style");

    let mut out = String::with_capacity(without.len());
    let mut in_tag = false;
    for c in without.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push('\n');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    collapse_blank_lines(&out)
}

fn collapse_blank_lines(s: &str) -> String {
    let mut lines: Vec<&str> = Vec::new();
    for l in s.lines().map(str::trim_end) {
        if l.trim().is_empty() && matches!(lines.last(), Some(x) if x.trim().is_empty()) {
            continue;
        }
        lines.push(l);
    }
    lines.join("\n").trim().to_string()
}

// ---- de-duplication --------------------------------------------------------

/// A short line must repeat at least this many times before it is even worth
/// asking the user about.
const SUSPECT_REPEATS: usize = 4;
/// Lines longer than this are prose, not page furniture.
const SUSPECT_MAX_CHARS: usize = 80;
/// A paragraph this long repeating verbatim is duplication, not emphasis.
const DUP_PARAGRAPH_CHARS: usize = 200;

/// A short line that repeats suspiciously often. **Reported, never removed.**
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suspect {
    pub line: String,
    pub count: usize,
}

#[derive(Debug, Default, PartialEq)]
pub struct Cleaned {
    pub text: String,
    /// Duplicate paragraphs removed automatically.
    pub removed_paragraphs: usize,
    /// A few of the removed paragraphs, so the user can see what went.
    pub samples: Vec<String>,
    /// Repeated short lines left in place for the user to judge.
    pub suspects: Vec<Suspect>,
}

impl Cleaned {
    pub fn note(&self) -> Option<String> {
        if self.removed_paragraphs == 0 {
            return None;
        }
        Some(format!(
            "đã bỏ {} đoạn trùng lặp ({})",
            self.removed_paragraphs,
            self.samples
                .iter()
                .map(|s| format!("“{}”", head(s, 40)))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

/// Remove repetition that is unambiguously an artefact, and *flag* the rest.
///
/// The split is deliberate, and it is the whole design:
///
/// * **Removed automatically — duplicate paragraphs.** Adjacent repeats (an
///   extraction stutter) and long paragraphs repeated verbatim. A 200-character
///   paragraph appearing twice word-for-word is never something an author did
///   on purpose.
/// * **Only reported — repeated short lines.** A PDF's running header repeats
///   on every page; so does "Bài tập 1" under every chapter of a textbook.
///   They are *indistinguishable* by frequency, length or spacing — both are
///   short, identical and evenly spread. Guessing means eventually deleting the
///   structure a learner needs, silently. So the user gets the list and
///   decides (see `db::strip_lines`).
pub fn dedupe(text: &str) -> Cleaned {
    let mut out = Cleaned::default();

    // ── Flag repeated short lines (no removal) ─────────────────────────────
    let mut counts: std::collections::HashMap<String, (String, usize)> =
        std::collections::HashMap::new();
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.chars().count() > SUSPECT_MAX_CHARS {
            continue;
        }
        let e = counts
            .entry(squash_ws(t))
            .or_insert_with(|| (t.to_string(), 0));
        e.1 += 1;
    }
    out.suspects = counts
        .into_values()
        .filter(|(_, n)| *n >= SUSPECT_REPEATS)
        .map(|(line, count)| Suspect { line, count })
        .collect();
    // Most repeated first, then alphabetical so the list is stable.
    out.suspects
        .sort_by(|a, b| b.count.cmp(&a.count).then(a.line.cmp(&b.line)));

    // ── Remove duplicate paragraphs ────────────────────────────────────────
    let spans = paragraph_spans(text);
    let chars: Vec<char> = text.chars().collect();
    let mut seen_long: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut prev_key: Option<String> = None;
    let mut kept: Vec<String> = Vec::new();

    for (a, b) in spans {
        let para: String = chars[a..b].iter().collect();
        let key = squash_ws(&fold(&para));
        if key.is_empty() {
            continue;
        }
        let is_dup = if Some(&key) == prev_key.as_ref() {
            true
        } else if key.chars().count() >= DUP_PARAGRAPH_CHARS {
            !seen_long.insert(key.clone())
        } else {
            false
        };

        if is_dup {
            out.removed_paragraphs += 1;
            if out.samples.len() < 3 {
                out.samples.push(head(para.trim(), 60));
            }
        } else {
            kept.push(para.trim().to_string());
        }
        prev_key = Some(key);
    }

    out.text = kept.join("\n\n");
    out
}

/// Drop every occurrence of the given lines from `text`.
///
/// Used by the review step once the user has confirmed which repeated lines are
/// page furniture. Matching is whitespace-insensitive so a header that picked
/// up stray spacing during extraction still goes.
pub fn strip_lines(text: &str, lines: &[String]) -> (String, usize) {
    let targets: std::collections::HashSet<String> =
        lines.iter().map(|l| squash_ws(l)).filter(|l| !l.is_empty()).collect();
    if targets.is_empty() {
        return (text.to_string(), 0);
    }
    let mut removed = 0;
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| {
            if targets.contains(&squash_ws(l.trim())) {
                removed += 1;
                false
            } else {
                true
            }
        })
        .collect();
    (collapse_blank_lines(&kept.join("\n")), removed)
}

// ---- chunking --------------------------------------------------------------

/// Target chunk size in characters (not bytes — Vietnamese is multibyte).
pub const CHUNK_CHARS: usize = 1_100;
/// Overlap so a fact split across a boundary is still findable from either side.
pub const OVERLAP_CHARS: usize = 150;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Inclusive char index into the document body.
    pub char_start: usize,
    /// Exclusive char index into the document body.
    pub char_end: usize,
    pub text: String,
}

/// Char spans of the blank-line-separated paragraphs of `text`.
pub fn paragraph_spans(text: &str) -> Vec<(usize, usize)> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        // Skip separator runs (a blank line = '\n' followed by whitespace-only
        // line); simply skip leading whitespace.
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let start = i;
        // Run until a blank line ("\n\n" possibly with spaces between).
        let mut end = chars.len();
        let mut j = i;
        while j < chars.len() {
            if chars[j] == '\n' {
                let mut k = j + 1;
                let mut newlines = 1;
                while k < chars.len() && chars[k].is_whitespace() {
                    if chars[k] == '\n' {
                        newlines += 1;
                    }
                    k += 1;
                }
                if newlines >= 2 {
                    end = j;
                    j = k;
                    break;
                }
                j = k;
            } else {
                j += 1;
            }
        }
        if end == chars.len() {
            spans.push((start, chars.len()));
            break;
        }
        spans.push((start, end));
        i = j;
    }
    spans
}

/// Split into paragraph-aligned, overlapping chunks that keep their offsets.
///
/// Paragraph alignment matters more than exact size: a chunk that starts
/// mid-sentence reads as noise when shown as evidence. Overlap is expressed as
/// a *span* that reaches back into the previous chunk, so `text` is always
/// exactly `body[char_start..char_end]` — which is what makes quote
/// verification (used by the quiz guard) a substring check against the real
/// document rather than against a reassembled copy.
pub fn chunk(text: &str) -> Vec<Chunk> {
    let chars: Vec<char> = text.chars().collect();
    let slice = |a: usize, b: usize| -> String { chars[a..b].iter().collect::<String>() };

    let spans = paragraph_spans(text);
    if spans.is_empty() {
        return vec![];
    }

    let mut out: Vec<Chunk> = Vec::new();
    let mut cur_start: Option<usize> = None;
    let mut cur_end = 0usize;

    let push = |out: &mut Vec<Chunk>, start: usize, end: usize| {
        // Reach back for overlap, but never past the previous chunk's start.
        let back = out
            .last()
            .map(|c| c.char_start + 1)
            .unwrap_or(0)
            .max(start.saturating_sub(OVERLAP_CHARS));
        let s = if out.is_empty() { start } else { back.min(start) };
        let t = slice(s, end);
        if !t.trim().is_empty() {
            out.push(Chunk {
                char_start: s,
                char_end: end,
                text: t.trim().to_string(),
            });
        }
    };

    for (ps, pe) in spans {
        let plen = pe - ps;
        // A single oversized paragraph must still be split, or one wall of text
        // becomes one unsearchable chunk.
        if plen > CHUNK_CHARS {
            if let Some(s) = cur_start.take() {
                push(&mut out, s, cur_end);
            }
            let mut i = ps;
            while i < pe {
                let end = (i + CHUNK_CHARS).min(pe);
                push(&mut out, i, end);
                if end == pe {
                    break;
                }
                i = end;
            }
            continue;
        }
        match cur_start {
            Some(s) if (pe - s) > CHUNK_CHARS => {
                push(&mut out, s, cur_end);
                cur_start = Some(ps);
                cur_end = pe;
            }
            Some(_) => cur_end = pe,
            None => {
                cur_start = Some(ps);
                cur_end = pe;
            }
        }
    }
    if let Some(s) = cur_start {
        push(&mut out, s, cur_end);
    }
    out
}

// ---- normalisation ---------------------------------------------------------

/// Search-normalised copy of a string: lower case, no Vietnamese diacritics,
/// `đ` → `d`.
///
/// Two different consumers need this and they need it to agree:
///
/// * **FTS5.** Its `unicode61 remove_diacritics 2` strips combining marks, so
///   `ố` folds to `o` on its own — but `đ` is a distinct letter, not `d` plus a
///   mark, and survives. Without folding it, "dong" never matches "đông".
/// * **Plain Rust comparisons** — cloze grading, duplicate-card detection,
///   verifying that a model's quote really occurs in a chunk. SQLite is not
///   involved there, so the folding has to happen here or a learner who types
///   "ngan hang nha nuoc" is marked wrong.
///
/// Folding both sides of every comparison keeps the two paths consistent.
pub fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.to_lowercase().chars() {
        out.push(match c {
            'đ' => 'd',
            'á' | 'à' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ấ' | 'ầ'
            | 'ẩ' | 'ẫ' | 'ậ' => 'a',
            'é' | 'è' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' => 'e',
            'í' | 'ì' | 'ỉ' | 'ĩ' | 'ị' => 'i',
            'ó' | 'ò' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ớ' | 'ờ'
            | 'ở' | 'ỡ' | 'ợ' => 'o',
            'ú' | 'ù' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' => 'u',
            'ý' | 'ỳ' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
            other => other,
        });
    }
    out
}

/// Collapse all whitespace runs to a single space — the comparison form used
/// when checking that a model's quote really appears in the source chunk.
pub fn squash_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// First `n` chars, never splitting a codepoint (`&s[..n]` panics on
/// multibyte text — the exact crash this repo has hit before).
pub fn head(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n).collect()
}

// ---- FTS5 query construction ----------------------------------------------

/// FTS5 operators that must never reach the parser as bare words.
const FTS_KEYWORDS: &[&str] = &["AND", "OR", "NOT", "NEAR"];

/// Turn a user's plain query into a safe FTS5 MATCH expression.
///
/// Every token is quoted, so punctuation, `-`, `*`, `:` and the boolean
/// keywords are all inert. Tokens are OR-joined (recall first; bm25 does the
/// ranking).
///
/// Returns `None` when the query has no usable token, so the caller can report
/// "query has nothing searchable" instead of running a malformed MATCH.
pub fn fts_query(raw: &str) -> Option<String> {
    let folded = fold(raw);
    let tokens: Vec<String> = folded
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| {
            let escaped = t.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect();
    if tokens.is_empty() {
        return None;
    }
    if tokens
        .iter()
        .all(|t| FTS_KEYWORDS.contains(&t.trim_matches('"').to_uppercase().as_str()))
    {
        return None;
    }
    Some(tokens.join(" OR "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_extensions_are_refused_by_name() {
        let err = extract("virus.exe", b"MZ").unwrap_err().to_string();
        assert!(err.contains(".exe"), "{err}");
    }

    #[test]
    fn an_empty_file_is_an_error_not_an_empty_document() {
        assert!(extract("a.txt", b"   \n  ").is_err());
    }

    #[test]
    fn a_bom_does_not_end_up_in_the_text() {
        let out = extract("a.txt", "\u{FEFF}lãi suất".as_bytes()).unwrap();
        assert_eq!(out.text.trim(), "lãi suất");
    }

    #[test]
    fn a_pdf_without_a_text_layer_says_so_instead_of_indexing_nothing() {
        assert!(extract("scan.pdf", b"%PDF-1.4 no text here").is_err());
    }

    #[test]
    fn script_and_style_bodies_do_not_reach_the_index() {
        let html = "<html><style>.a{color:red}</style><body><p>Nội dung thật</p>\
                    <script>var x=1;</script></body></html>";
        let out = strip_html(html);
        assert!(out.contains("Nội dung thật"));
        assert!(!out.contains("color:red"), "style leaked: {out}");
        assert!(!out.contains("var x"), "script leaked: {out}");
    }

    #[test]
    fn every_chunk_text_is_exactly_its_span_of_the_body() {
        let text = (0..40)
            .map(|i| format!("Đoạn số {i} nói về lãi suất điều hành và thị trường vàng."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let chars: Vec<char> = text.chars().collect();
        let chunks = chunk(&text);
        assert!(chunks.len() > 1, "long text must split");
        for c in &chunks {
            let span: String = chars[c.char_start..c.char_end].iter().collect();
            assert_eq!(
                squash_ws(&span),
                squash_ws(&c.text),
                "chunk text must be the span it claims — citations depend on it"
            );
        }
    }

    #[test]
    fn chunks_overlap_so_a_boundary_split_fact_stays_findable() {
        let text = (0..40)
            .map(|i| format!("Đoạn số {i} nói về lãi suất điều hành và thị trường vàng."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk(&text);
        assert!(
            chunks[1].char_start < chunks[0].char_end,
            "chunk 1 must reach back into chunk 0"
        );
    }

    #[test]
    fn one_giant_paragraph_still_gets_split() {
        let wall = "từ ".repeat(3_000);
        let chunks = chunk(&wall);
        assert!(chunks.len() > 1, "a wall of text must not become one chunk");
    }

    #[test]
    fn short_text_stays_a_single_chunk() {
        assert_eq!(chunk("một câu ngắn").len(), 1);
        assert!(chunk("   ").is_empty());
    }

    #[test]
    fn chunking_never_splits_a_multibyte_character() {
        let viet = "lãi suất điều hành ".repeat(400);
        for c in chunk(&viet) {
            assert!(!c.text.contains('\u{FFFD}'), "split codepoint");
        }
    }

    #[test]
    fn fts_operators_in_a_user_query_are_neutralised() {
        for raw in [
            "giá \"vàng\" SJC",
            "lãi suất - ngân hàng",
            "a AND b",
            "foo* NEAR bar",
            "x: y",
            "(unbalanced",
        ] {
            let q = fts_query(raw).expect("should produce a query");
            assert!(q.contains('"'), "tokens must be quoted: {q}");
            assert!(!q.contains('('), "unquoted paren survived: {q}");
            assert!(!q.contains('*'), "unquoted star survived: {q}");
            assert!(!q.contains('-'), "unquoted dash survived: {q}");
        }
    }

    #[test]
    fn a_query_with_nothing_searchable_returns_none() {
        assert_eq!(fts_query("   "), None);
        assert_eq!(fts_query("!!! ??? ***"), None);
    }

    #[test]
    fn folding_removes_case_diacritics_and_d_with_stroke() {
        assert_eq!(fold("Đông Dương"), "dong duong");
        assert_eq!(fold("Lãi Suất Điều Hành"), "lai suat dieu hanh");
        assert_eq!(fold("Ngân hàng Nhà nước"), "ngan hang nha nuoc");
        assert_eq!(fts_query("dong").unwrap(), "\"dong\"");
    }

    #[test]
    fn folding_leaves_non_vietnamese_text_alone_apart_from_case() {
        assert_eq!(fold("HTTP/2 API v3"), "http/2 api v3");
    }

    #[test]
    fn head_does_not_panic_on_multibyte_text() {
        assert_eq!(head("lãi suất", 3), "lãi");
        assert_eq!(head("abc", 10), "abc");
    }
}
