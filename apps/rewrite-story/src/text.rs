//! Vietnamese-aware hybrid text splitter.
//!
//! Port of `backend/pkg/text/hybrid_splitter.go` from the Go implementation.
//! Combines hard size bounds with TF/cosine-similarity topic detection so chunks
//! break at narrative shifts rather than arbitrary offsets, while protecting
//! chapter headers (`Chương`/`Hồi`/…) and `[System: …]` brackets from being split.
//!
//! Sizing is in **characters**, unlike the Go original which used `len()` — a
//! byte count. Every knob the app exposes (settings, MCP tool descriptions, the
//! UI) is labelled "ký tự", and on Vietnamese prose bytes run ~1.4x characters,
//! so byte semantics silently delivered chunks ~30% smaller than asked for.
//! Slicing stays byte-indexed internally (that is what `str` supports); the
//! conversion happens at the size comparisons and in `split_text_safely`.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

/// Basic Vietnamese stopword filter — dropped before building term-frequency vectors.
const VN_STOPWORDS: &[&str] = &[
    "và", "là", "của", "thì", "mà", "cho", "những", "các", "một", "đã", "đang", "sẽ", "ở", "tại",
    "để", "với", "này", "kia", "có", "không", "như", "trong", "khi", "người", "được", "về", "ra",
    "lên", "vào", "lại",
];

/// Strips everything that is not a Vietnamese letter, digit, or whitespace.
static NON_VIETNAMESE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)[^a-zàáảãạăắằẳẵặâấầẩẫậèéẻẽẹêếềểễệđìíỉĩịòóỏõọôốồổỗộơớờởỡợùúủũụưứừửữựỳýỷỹỵ\d\s]+",
    )
    .expect("static regex")
});

/// Matches chapter headings: "Chương 1", "Hồi thứ nhất", "Phần 2:", …
static CHAPTER_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(chương|hồi|tiết|phần|quyển)\s+[\d\w\s]+").expect("static regex")
});

/// Term-frequency vector, normalized by the number of non-stopword terms.
fn build_tf(text: &str) -> HashMap<String, f64> {
    let lowered = text.to_lowercase();
    let processed = NON_VIETNAMESE.replace_all(&lowered, " ");

    let mut tf: HashMap<String, f64> = HashMap::new();
    let mut valid_word_count = 0usize;

    for word in processed.split_whitespace() {
        // Go's `len(word) < 2` is a byte check, so it kept any single Vietnamese
        // letter (`ở`, `ế`, … are 3 bytes) while dropping single ASCII letters.
        // Counting characters applies the intended "drop 1-letter tokens" rule
        // uniformly.
        if word.chars().count() < 2 || VN_STOPWORDS.contains(&word) {
            continue;
        }
        *tf.entry(word.to_string()).or_insert(0.0) += 1.0;
        valid_word_count += 1;
    }

    if valid_word_count > 0 {
        let n = valid_word_count as f64;
        for v in tf.values_mut() {
            *v /= n;
        }
    }
    tf
}

fn cosine_similarity(tf1: &HashMap<String, f64>, tf2: &HashMap<String, f64>) -> f64 {
    if tf1.is_empty() || tf2.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0;
    let mut mag1 = 0.0;
    let mut mag2 = 0.0;

    for (word, v1) in tf1 {
        if let Some(v2) = tf2.get(word) {
            dot += v1 * v2;
        }
        mag1 += v1 * v1;
    }
    for v2 in tf2.values() {
        mag2 += v2 * v2;
    }

    if mag1 == 0.0 || mag2 == 0.0 {
        return 0.0;
    }
    dot / (mag1.sqrt() * mag2.sqrt())
}

fn is_chapter_start(line: &str) -> bool {
    let line = line.trim();
    // Chapter headings are never long. Counted in characters — Go's byte check
    // rejected Vietnamese headings from ~70 characters up.
    if line.chars().count() > 100 || line.is_empty() {
        return false;
    }
    CHAPTER_START.is_match(line)
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

/// Splits an over-long paragraph, preserving system brackets and dialogue.
///
/// Positions are tracked in **characters**; `bounds` maps a character index onto
/// its byte offset so the actual slicing stays cheap and can never land
/// mid-codepoint.
fn split_text_safely(text: &str, max_chars: usize) -> Vec<String> {
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let total_chars = bounds.len() - 1;

    let mut chunks = Vec::new();
    let mut cur = 0usize; // character index

    while cur < total_chars {
        if total_chars - cur <= max_chars {
            chunks.push(text[bounds[cur]..].trim().to_string());
            break;
        }

        let start_b = bounds[cur];
        let window = &text[start_b..bounds[cur + max_chars]];

        // Protection barrier: never cut inside an unclosed "[...]" block (e.g. system
        // messages like "[Hệ thống: ...]"). If the window opens a bracket it doesn't
        // close, extend the cut past the closing "]" even if that exceeds max_chars.
        if let Some(open_b) = window.rfind('[') {
            if !window[open_b..].contains(']') {
                if let Some(close_rel) = text[start_b + open_b..].find(']') {
                    let mut cut_b = open_b + close_rel + 1;
                    // Also consume the trailing space after the bracket.
                    if text.as_bytes().get(start_b + cut_b) == Some(&b' ') {
                        cut_b += 1;
                    }
                    chunks.push(text[start_b..start_b + cut_b].trim().to_string());
                    cur += char_len(&text[start_b..start_b + cut_b]);
                    continue;
                }
            }
        }

        // Priority 1: break after a safe terminator (system block or dialogue).
        let mut cut = 0usize; // characters into the window
        for d in ["] ", ".\" ", "!\" ", "?\" ", ".\n", "\n"] {
            if let Some(b) = window.rfind(d) {
                cut = cut.max(char_len(&window[..b + d.len()]));
            }
        }

        // Priority 2: no good break found near the end — fall back to plain punctuation.
        if cut <= max_chars / 2 {
            if let Some(b) = window.rfind(". ") {
                cut = char_len(&window[..b + 2]);
            } else if let Some(b) = window.rfind(' ') {
                cut = char_len(&window[..b + 1]);
            }
        }

        // Last resort: hard cut at max_chars. Also guards against a zero-width
        // step, which would spin forever.
        if cut == 0 {
            cut = max_chars;
        }

        let end = (cur + cut).min(total_chars);
        chunks.push(text[start_b..bounds[end]].trim().to_string());
        cur = end;
    }
    chunks
}

/// First (`from_start`) or last `n` whitespace-separated words of `text`.
fn get_words(text: &str, n: usize, from_start: bool) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= n {
        return text.to_string();
    }
    if from_start {
        words[..n].join(" ")
    } else {
        words[words.len() - n..].join(" ")
    }
}

/// Splits `text` into semantically coherent chunks.
///
/// * `min_size` — a chunk is only eligible for a semantic break past this size.
/// * `max_size` — hard upper bound (bytes); paragraphs longer than this are cut safely.
/// * `sim_threshold` — cosine similarity below which a topic shift is declared.
pub fn hybrid_split(
    text: &str,
    min_size: usize,
    max_size: usize,
    sim_threshold: f64,
) -> Vec<String> {
    let text = text.replace("\r\n", "\n");

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    // Tracked alongside `current` so the size checks stay O(1) instead of
    // re-counting the whole accumulated chunk on every paragraph.
    let mut current_chars = 0usize;
    let mut line_count = 0usize;

    for para in text.split('\n') {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        let para_chars = char_len(para);

        // 1. A single paragraph over max_size — flush, then cut it safely.
        if para_chars > max_size {
            if !current.is_empty() {
                chunks.push(current.trim().to_string());
                current.clear();
                current_chars = 0;
            }
            chunks.extend(split_text_safely(para, max_size));
            line_count = 0;
            continue;
        }

        // 2. Appending this paragraph would exceed max_size.
        if current_chars + para_chars + 1 > max_size {
            chunks.push(current.trim().to_string());
            current.clear();
            current_chars = 0;
            line_count = 0;
        }

        // 3. Semantic split & chapter detection. The `line_count >= 3` debouncer
        //    stops a single stray line from fragmenting the chunk.
        if current_chars > min_size && line_count >= 3 {
            if is_chapter_start(para) {
                chunks.push(current.trim().to_string());
                current.clear();
                current_chars = 0;
                line_count = 0;
            } else {
                let last_ctx = get_words(&current, 40, false);
                let next_ctx = get_words(para, 40, true);
                if cosine_similarity(&build_tf(&last_ctx), &build_tf(&next_ctx)) < sim_threshold {
                    chunks.push(current.trim().to_string());
                    current.clear();
                    current_chars = 0;
                    line_count = 0;
                }
            }
        }

        if !current.is_empty() {
            current.push('\n');
            current_chars += 1;
        }
        current.push_str(para);
        current_chars += para_chars;
        line_count += 1;
    }

    if !current.is_empty() {
        chunks.push(current.trim().to_string());
    }

    // An empty chunk is unrecoverable downstream: the split is cached against the
    // story forever, the model is asked to rewrite nothing, and every retry dies
    // on the same index. Two paths can produce one — flushing an empty `current`
    // when a paragraph exactly fills `max_size`, and `split_text_safely` picking a
    // segment that is entirely whitespace — so filter here rather than chase both.
    chunks.retain(|c| !c.trim().is_empty());
    chunks
}

/// How much of the previous chunk is fed forward for prose continuity.
const MAX_CONTINUITY_CHARS: usize = 300;

/// Last `n` characters of `s`, char-safe.
fn tail_chars(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= n {
        return s.to_string();
    }
    chars[chars.len() - n..].iter().collect()
}

/// The tail of a rewritten chunk, handed to the next chunk's prompt so the prose
/// stays continuous across the seam.
///
/// The Go original used two different functions for this: `ExtractLastParagraph`
/// in the rewrite loop but `ExtractLastSentences(_, 2)` when rebuilding state on
/// resume — so a resumed run fed the model a differently-shaped continuity hint
/// than an uninterrupted one. Unified here on the paragraph form; both paths cap
/// at 300 characters as before.
pub fn continuity_tail(text: &str) -> String {
    let last = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .last()
        .unwrap_or("");
    let last = if last.is_empty() { text.trim() } else { last };
    tail_chars(last, MAX_CONTINUITY_CHARS).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuity_tail_takes_the_final_paragraph() {
        let text = "Đoạn một.\n\nĐoạn hai.\n\nĐoạn cuối cùng.";
        assert_eq!(continuity_tail(text), "Đoạn cuối cùng.");
    }

    #[test]
    fn continuity_tail_caps_at_300_chars_without_splitting_a_char() {
        let text = "à".repeat(1000);
        let tail = continuity_tail(&text);
        assert_eq!(tail.chars().count(), MAX_CONTINUITY_CHARS);
        assert!(tail.chars().all(|c| c == 'à'));
    }

    #[test]
    fn continuity_tail_handles_text_with_no_paragraph_breaks() {
        assert_eq!(
            continuity_tail("  Một dòng duy nhất.  "),
            "Một dòng duy nhất."
        );
    }

    /// Ported from `TestHybridSplit_V2Rules/Rule_2` — a system bracket must never
    /// be split across chunks, even when the size boundary lands inside it.
    #[test]
    fn rule_2_protects_system_brackets() {
        let msg = "[Hệ thống: Bạn đã đạt được thành tựu Tuyệt Thế Thiên Tài!]";
        let long_line = format!("{}{} {}", "a".repeat(80), msg, "b".repeat(80));

        let chunks = hybrid_split(&long_line, 50, 100, 0.15);

        assert!(
            chunks.iter().any(|c| c.contains(msg)),
            "system prompt was split across chunks: {chunks:?}"
        );
    }

    /// Ported from `TestHybridSplit_V2Rules/Rule_3` — the debouncer must hold the
    /// first chunk to at least 3 lines even when similarity says "split now".
    #[test]
    fn rule_3_tiling_debouncer() {
        let text = "Câu 1: Nam chính bắt đầu hành trình.\n\
                    Câu 2: Anh ta gặp một thế lực lạ.\n\
                    Câu 3: Cuộc chiến nổ ra dữ dội.\n\
                    CHUYỂN CẢNH: Một nơi hoàn toàn khác với nhân vật khác.";

        let chunks = hybrid_split(text, 10, 1000, 0.9);
        let first_chunk_lines = chunks[0].split('\n').count();

        assert!(
            first_chunk_lines >= 3,
            "debouncer allowed a split too early: {first_chunk_lines} lines"
        );
    }

    /// Chapter headings force a break once the chunk is past min_size.
    #[test]
    fn chapter_heading_forces_break() {
        let body = "Nhân vật chính tiếp tục cuộc hành trình dài trong rừng sâu.\n".repeat(6);
        let text = format!("{body}Chương 2: Gặp gỡ định mệnh\nMột ngày mới bắt đầu.");

        let chunks = hybrid_split(&text, 100, 100_000, 0.0);

        assert!(chunks.len() >= 2, "expected a break at the chapter heading");
        assert!(
            chunks[1].starts_with("Chương 2"),
            "chapter heading should open the next chunk, got: {:?}",
            chunks[1]
        );
    }

    /// Sizes are characters, not bytes.
    ///
    /// Go compared `len()` — a byte count — against these bounds, so on
    /// Vietnamese prose (~1.4 bytes/char) every chunk came out ~30% shorter than
    /// the number the user typed into a field labelled "ký tự".
    #[test]
    fn size_bounds_are_measured_in_characters() {
        let para = "Đây là một đoạn văn tiếng Việt có dấu đầy đủ để đo đơn vị kích thước.";
        assert!(
            para.len() > para.chars().count(),
            "test text must be multibyte"
        );
        let text = vec![para; 40].join("\n");

        // Threshold 0.0 disables semantic splitting, isolating the size bound.
        let chunks = hybrid_split(&text, 500, 2000, 0.0);

        let longest = chunks.iter().map(|c| c.chars().count()).max().unwrap();
        assert!(
            longest > 1500,
            "chunks came out far short of 2000 chars: {longest}"
        );
        assert!(
            longest <= 2000,
            "chunk exceeded the character bound: {longest}"
        );
    }

    /// A chapter heading in Vietnamese can exceed 100 *bytes* while being well
    /// under 100 characters; Go's byte check silently stopped detecting those.
    #[test]
    fn long_vietnamese_chapter_heading_is_still_detected() {
        // 108 bytes, 77 characters.
        let heading =
            "Chương 12: Đường về cố hương mịt mù khói lửa và những kẻ đã khuất bóng năm ấy";
        assert!(heading.len() > 100, "heading must exceed 100 bytes");
        assert!(heading.chars().count() < 100, "but stay under 100 chars");

        assert!(is_chapter_start(heading));
    }

    /// The hard-cut fallback must land on a char boundary rather than panicking
    /// mid-rune — the whole reason this port can't slice bytes like Go does.
    #[test]
    fn hard_cut_never_splits_a_multibyte_char() {
        // No spaces, no punctuation: forces the `cut_idx == 0` fallback path.
        let text = "à".repeat(500);

        let chunks = hybrid_split(&text, 10, 51, 0.2);

        assert!(chunks.len() > 1, "expected the text to be split");
        let rejoined: String = chunks.concat();
        assert_eq!(
            rejoined.chars().count(),
            500,
            "characters were lost or mangled"
        );
        assert!(chunks.iter().all(|c| c.chars().all(|ch| ch == 'à')));
    }

    /// Splitting must terminate and preserve content for a realistic mixed text.
    #[test]
    fn preserves_all_paragraphs() {
        let text = (1..=50)
            .map(|i| format!("Đoạn văn số {i} kể về một sự kiện trong câu chuyện dài."))
            .collect::<Vec<_>>()
            .join("\n");

        let chunks = hybrid_split(&text, 200, 400, 0.2);

        assert!(chunks.len() > 1);
        for i in 1..=50 {
            let needle = format!("Đoạn văn số {i} ");
            assert!(
                chunks.iter().any(|c| c.contains(&needle)),
                "paragraph {i} was dropped"
            );
        }
    }
}
