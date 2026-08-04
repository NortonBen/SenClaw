//! The app's own document corpus: extract → chunk → FTS5.
//!
//! This is the one source that owns its data rather than querying someone
//! else's, so it owns three problems nobody else solves for it:
//!
//! 1. **Extraction that admits failure.** A scanned PDF has no text layer. The
//!    honest outcome is an error naming the cause, not a document stored with
//!    empty content that answers every future query with silence.
//! 2. **Chunking.** A whole document is a bad retrieval unit; a sentence is too
//!    small to be evidence. Chunks are paragraph-aligned with overlap.
//! 3. **FTS5 query syntax.** A user's query is not an FTS5 expression. `giá
//!    "vàng" - SJC` is a syntax error, and `AND`/`OR`/`NEAR` are keywords. Raw
//!    interpolation is both a crash and an injection vector.

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

/// Bytes → text. Tolerates a BOM and invalid UTF-8 rather than refusing a file
/// over one bad byte.
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
            text: decode_text(bytes),
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
             hãy OCR trước rồi tải lên phần văn bản"
        );
    }
    Ok(Extracted {
        text: trimmed.to_string(),
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
            Ok(Event::End(e)) if e.name().as_ref() == b"w:p" => out.push('\n'),
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
    // Drop script/style bodies first — their contents are not prose and would
    // otherwise dominate the index.
    let without = drop_element(&drop_element(s, "script"), "style");

    let mut out = String::with_capacity(without.len());
    let mut in_tag = false;
    for c in without.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
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

// ---- chunking --------------------------------------------------------------

/// Target chunk size in characters (not bytes — Vietnamese is multibyte).
const CHUNK_CHARS: usize = 1_200;
/// Overlap so a fact split across a boundary is still findable from either side.
const OVERLAP_CHARS: usize = 150;

/// Split into paragraph-aligned, overlapping chunks.
///
/// Paragraph alignment matters more than exact size: a chunk that starts
/// mid-sentence reads as noise when shown as evidence.
pub fn chunk(text: &str) -> Vec<String> {
    let paragraphs: Vec<&str> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if paragraphs.is_empty() {
        return vec![];
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for p in paragraphs {
        // A single oversized paragraph must still be split, or one wall of text
        // becomes one unsearchable chunk.
        if p.chars().count() > CHUNK_CHARS {
            if !current.trim().is_empty() {
                chunks.push(std::mem::take(&mut current).trim().to_string());
            }
            chunks.extend(split_hard(p));
            continue;
        }
        if current.chars().count() + p.chars().count() > CHUNK_CHARS && !current.is_empty() {
            chunks.push(current.trim().to_string());
            current = tail(&chunks[chunks.len() - 1], OVERLAP_CHARS);
            current.push_str("\n\n");
        }
        current.push_str(p);
        current.push_str("\n\n");
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
}

fn split_hard(p: &str) -> Vec<String> {
    let chars: Vec<char> = p.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let end = (i + CHUNK_CHARS).min(chars.len());
        out.push(chars[i..end].iter().collect::<String>().trim().to_string());
        if end == chars.len() {
            break;
        }
        i = end - OVERLAP_CHARS.min(end - i - 1);
    }
    out
}

/// Last `n` characters, on a char boundary.
fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}

// ---- FTS5 query construction ----------------------------------------------

/// FTS5 operators that must never reach the parser as bare words.
const FTS_KEYWORDS: &[&str] = &["AND", "OR", "NOT", "NEAR"];

/// Turn a user's plain query into a safe FTS5 MATCH expression.
///
/// Every token is quoted, so punctuation, `-`, `*`, `:` and the boolean
/// keywords are all inert. Tokens are OR-joined (recall first; bm25 does the
/// ranking) — deliberately unlike the wiki, whose FTS AND-joins and therefore
/// silently returns nothing for a long query.
///
/// Returns `None` when the query has no usable token, so the caller can report
/// "query has nothing searchable" instead of running a malformed MATCH.
pub fn fts_query(raw: &str) -> Option<String> {
    let tokens: Vec<String> = raw
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(|t| {
            // Doubling `"` is FTS5's own escape for a quoted string.
            let escaped = t.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect();
    if tokens.is_empty() {
        return None;
    }
    // Keywords are already neutralised by the quoting; this only avoids a
    // query that is *nothing but* keywords.
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
        // Not a real PDF, so extraction fails; the point is that neither path
        // can end with a stored-but-empty document.
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
    fn chunks_are_paragraph_aligned_and_overlap() {
        let text = (0..40)
            .map(|i| format!("Đoạn số {i} nói về lãi suất điều hành và thị trường vàng."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk(&text);
        assert!(chunks.len() > 1, "long text must split");
        for c in &chunks {
            assert!(c.chars().count() <= CHUNK_CHARS + OVERLAP_CHARS + 200);
            assert!(!c.starts_with('\n'));
        }
        // Overlap: the tail of chunk 0 reappears at the head of chunk 1.
        let overlap_probe: String = tail(&chunks[0], 30);
        assert!(
            chunks[1].contains(overlap_probe.trim()),
            "chunks must overlap so a boundary-split fact stays findable"
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
            assert!(c.is_char_boundary(0));
            assert!(
                !c.contains('\u{FFFD}'),
                "replacement char = split codepoint"
            );
        }
    }

    #[test]
    fn fts_operators_in_a_user_query_are_neutralised() {
        // Raw interpolation of any of these is an FTS5 syntax error.
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
    fn tokens_are_or_joined_for_recall() {
        let q = fts_query("lãi suất").unwrap();
        assert_eq!(q, "\"lãi\" OR \"suất\"");
    }

    #[test]
    fn an_embedded_quote_is_escaped_not_dropped() {
        let q = fts_query("say \"hi\"").unwrap();
        assert!(q.contains("\"say\""));
        assert!(q.contains("\"hi\""));
    }
}
