//! Expansion of in-message directives typed by the user in the chat composer.
//!
//! Two forms, both surfaced by the Web UI's suggestion popup:
//!
//! * `/skill-name` or `#skill-name` — pin a skill for this turn. The composer
//!   only ever suggested them; without this pass the token reached the LLM as
//!   plain prose and the skill fired (or didn't) on the model's whim.
//! * `@path/to/file.md` — attach a text file. Image references are left alone;
//!   [`crate::agent::input_builder`] turns those into image blocks.
//!
//! Expansion appends blocks *after* the user's text and leaves the original
//! tokens in place, so the model still reads the sentence the user wrote.

use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use regex::Regex;

use crate::util::paths::expand_tilde;

/// Per-file read cap. Larger files are truncated with a visible notice.
const MAX_FILE_BYTES: usize = 128 * 1024;
/// Total inlined bytes across all `@` mentions in one message.
const MAX_TOTAL_BYTES: usize = 384 * 1024;
/// Upper bound on how many files one message may inline.
const MAX_FILES: usize = 10;
/// Upper bound on pinned skills — more than this is a typo, not intent.
const MAX_SKILLS: usize = 3;

/// `/name` or `#name` at a word boundary. Trailing punctuation is excluded so
/// "use #pdf, then …" resolves the skill rather than `pdf,`.
static SKILL_TOKEN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|\s)[/#]([A-Za-z0-9][A-Za-z0-9._:-]*)").unwrap());

/// `@path` at a word boundary. Quotes and angle brackets terminate the path.
static FILE_TOKEN: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:^|\s)@([^\s'\x22<>]+)").unwrap());

/// Extensions handled by the image pipeline — never inline these as text.
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico"];

/// Mention prefixes that name an agent/MCP target rather than a file.
const NON_FILE_PREFIXES: &[&str] = &["agent:", "subagent:", "mcp:", "mcp__"];

#[derive(Debug, Clone, Default)]
pub struct Expansion {
    /// Prompt to hand to the agent (original text plus appended blocks).
    pub prompt: String,
    /// Skills the user pinned, in the order typed.
    pub skills: Vec<String>,
    /// Absolute paths successfully inlined.
    pub files: Vec<PathBuf>,
    /// Human-readable notes about mentions that could not be resolved.
    pub warnings: Vec<String>,
}

/// True when `text` contains something worth running [`expand`] over. Lets the
/// caller skip the skill scan (a filesystem walk) for ordinary messages.
pub fn has_directives(text: &str) -> bool {
    SKILL_TOKEN.is_match(text) || FILE_TOKEN.is_match(text)
}

/// Expand `/skill`, `#skill` and `@file` directives found in `text`.
///
/// `known_skills` gates skill tokens: an unknown name is left as prose, so a
/// bare URL path or a "#1" issue reference never turns into a bogus directive.
/// `work_dir` anchors relative `@` paths; mentions that escape it are refused.
pub fn expand(text: &str, work_dir: Option<&Path>, known_skills: &[String]) -> Expansion {
    let mut out = Expansion {
        prompt: text.to_string(),
        ..Default::default()
    };

    for name in collect_skills(text, known_skills) {
        out.skills.push(name);
    }

    let root = work_dir.map(canonical_or_owned);
    let mut budget = MAX_TOTAL_BYTES;
    let mut blocks: Vec<String> = Vec::new();

    for mention in collect_file_mentions(text) {
        if out.files.len() >= MAX_FILES {
            out.warnings
                .push(format!("bỏ qua @{mention}: quá {MAX_FILES} tệp trong một tin nhắn"));
            continue;
        }
        match resolve_mention(&mention, root.as_deref()) {
            Ok(path) => {
                if out.files.contains(&path) {
                    continue;
                }
                match read_text_file(&path, budget) {
                    Ok((body, truncated)) => {
                        budget = budget.saturating_sub(body.len());
                        blocks.push(render_file_block(&mention, &path, &body, truncated));
                        out.files.push(path);
                    }
                    Err(e) => out.warnings.push(format!("@{mention}: {e}")),
                }
            }
            Err(e) => out.warnings.push(format!("@{mention}: {e}")),
        }
    }

    if !out.skills.is_empty() {
        out.prompt.push_str(&render_skill_block(&out.skills));
    }
    for block in blocks {
        out.prompt.push_str(&block);
    }
    if !out.warnings.is_empty() {
        out.prompt.push_str(&format!(
            "\n\n<system-reminder>\nMột số tệp người dùng nhắc đến không đọc được:\n{}\n</system-reminder>",
            out.warnings
                .iter()
                .map(|w| format!("- {w}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    out
}

fn collect_skills(text: &str, known: &[String]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for cap in SKILL_TOKEN.captures_iter(text) {
        if found.len() >= MAX_SKILLS {
            break;
        }
        let raw = cap[1].trim_end_matches(|c: char| matches!(c, '.' | ',' | ':' | ';' | '!' | '?'));
        if let Some(name) = known.iter().find(|k| k.eq_ignore_ascii_case(raw)) {
            if !found.contains(name) {
                found.push(name.clone());
            }
        }
    }
    found
}

fn collect_file_mentions(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for cap in FILE_TOKEN.captures_iter(text) {
        let raw = cap[1].trim_end_matches(|c: char| matches!(c, '.' | ',' | ':' | ';' | '!' | '?'));
        if raw.is_empty() || found.iter().any(|f| f == raw) {
            continue;
        }
        if NON_FILE_PREFIXES.iter().any(|p| raw.starts_with(p)) {
            continue;
        }
        if has_image_ext(raw) {
            continue;
        }
        found.push(raw.to_string());
    }
    found
}

fn has_image_ext(p: &str) -> bool {
    Path::new(p)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn canonical_or_owned(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Resolve one mention to an absolute file path.
///
/// Absolute (or `~`-prefixed) mentions are taken at face value: the user typed
/// a full path deliberately, and the agent can read any path it has access to
/// anyway. Relative mentions are anchored to `root` and must resolve inside it
/// — that is the case where a `../../` sequence would otherwise silently reach
/// outside the workspace the user believes they are talking about.
fn resolve_mention(mention: &str, root: Option<&Path>) -> Result<PathBuf, String> {
    let is_absolute = mention.starts_with('/') || mention.starts_with('~');
    let candidate = if is_absolute {
        expand_tilde(mention)
    } else {
        let root = root.ok_or("phiên chat không có thư mục làm việc để tra đường dẫn tương đối")?;
        root.join(mention)
    };

    let resolved = std::fs::canonicalize(&candidate).map_err(|e| format!("không mở được ({e})"))?;

    if !is_absolute {
        let root = root.expect("checked above");
        if !resolved.starts_with(root) {
            return Err("đường dẫn nằm ngoài thư mục làm việc".to_string());
        }
    }
    if !resolved.is_file() {
        return Err("không phải tệp".to_string());
    }
    Ok(resolved)
}

/// Read `path` as UTF-8, capped by both the per-file limit and the remaining
/// message budget. Returns the body and whether it was cut short.
fn read_text_file(path: &Path, budget: usize) -> Result<(String, bool), String> {
    if budget == 0 {
        return Err("đã đạt giới hạn tổng dung lượng đính kèm".to_string());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("đọc lỗi ({e})"))?;
    let cap = MAX_FILE_BYTES.min(budget);
    let truncated = bytes.len() > cap;
    let slice = if truncated { &bytes[..cap] } else { &bytes[..] };
    // A truncated read can land mid-codepoint; keep the valid prefix instead of
    // rejecting the whole file.
    let body = match std::str::from_utf8(slice) {
        Ok(s) => s.to_string(),
        Err(e) if truncated && e.valid_up_to() > 0 => {
            String::from_utf8_lossy(&slice[..e.valid_up_to()]).into_owned()
        }
        Err(_) => return Err("tệp nhị phân, không đính kèm được".to_string()),
    };
    Ok((body, truncated))
}

fn render_skill_block(skills: &[String]) -> String {
    let list = skills
        .iter()
        .map(|s| format!("- {s}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\n\n<system-reminder>\nNgười dùng đã chỉ định skill cho lượt này:\n{list}\n\
         Hãy gọi tool `Skill` với đúng tên skill ở trên TRƯỚC khi làm việc khác, \
         rồi làm theo hướng dẫn trong skill. Nếu skill không tồn tại hoặc không phù hợp, \
         hãy nói rõ với người dùng thay vì im lặng bỏ qua.\n</system-reminder>"
    )
}

fn render_file_block(mention: &str, path: &Path, body: &str, truncated: bool) -> String {
    let note = if truncated {
        "\n[... nội dung bị cắt bớt do vượt giới hạn kích thước ...]"
    } else {
        ""
    };
    format!(
        "\n\n<attached-file mention=\"@{mention}\" path=\"{}\">\n{body}{note}\n</attached-file>",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmpdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("senclaw-directives-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(&dir).unwrap()
    }

    #[test]
    fn detects_nothing_in_plain_text() {
        assert!(!has_directives("chào bạn, giúp mình việc này"));
    }

    #[test]
    fn pins_known_skill_and_ignores_unknown() {
        let known = vec!["agent-browser".to_string()];
        let out = expand("/agent-browser mở trang chủ /khong-co-that", None, &known);
        assert_eq!(out.skills, vec!["agent-browser"]);
        assert!(out.prompt.contains("Hãy gọi tool `Skill`"));
        assert!(out.prompt.starts_with("/agent-browser mở trang chủ"));
    }

    #[test]
    fn hash_form_pins_the_same_skill() {
        let known = vec!["pdf".to_string()];
        assert_eq!(expand("dùng #pdf nhé", None, &known).skills, vec!["pdf"]);
    }

    #[test]
    fn bare_url_path_is_not_a_skill() {
        let known = vec!["api".to_string()];
        let out = expand("xem https://x.dev/api thôi", None, &known);
        assert!(out.skills.is_empty(), "path segment must not pin a skill");
    }

    #[test]
    fn inlines_relative_file_under_work_dir() {
        let root = tmpdir("inline");
        fs::create_dir_all(root.join("task-20")).unwrap();
        fs::write(root.join("task-20/01-nghien-cuu.md"), "# Nghiên cứu\nnội dung").unwrap();

        let out = expand("@task-20/01-nghien-cuu.md check", Some(&root), &[]);
        assert_eq!(out.files.len(), 1);
        assert!(out.prompt.contains("<attached-file"));
        assert!(out.prompt.contains("# Nghiên cứu"));
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn refuses_escape_from_work_dir() {
        let root = tmpdir("escape");
        fs::create_dir_all(root.join("inner")).unwrap();
        fs::write(root.join("secret.txt"), "nope").unwrap();
        let inner = root.join("inner");

        let out = expand("@../secret.txt", Some(&inner), &[]);
        assert!(out.files.is_empty());
        assert!(out.prompt.contains("nằm ngoài thư mục làm việc"));
        assert!(!out.prompt.contains("nope"));
    }

    #[test]
    fn image_mentions_are_left_to_the_image_pipeline() {
        let out = expand("@/tmp/shot.png xem hộ", None, &[]);
        assert!(out.files.is_empty());
        assert!(out.warnings.is_empty());
        assert_eq!(out.prompt, "@/tmp/shot.png xem hộ");
    }

    #[test]
    fn agent_and_mcp_mentions_are_not_files() {
        let out = expand("@agent:general-purpose và @mcp:senclaw-browser", None, &[]);
        assert!(out.files.is_empty());
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn truncates_oversized_file_with_notice() {
        let root = tmpdir("big");
        fs::write(root.join("big.txt"), "x".repeat(MAX_FILE_BYTES + 4096)).unwrap();

        let out = expand("@big.txt", Some(&root), &[]);
        assert_eq!(out.files.len(), 1);
        assert!(out.prompt.contains("bị cắt bớt"));
    }

    #[test]
    fn missing_file_becomes_a_warning_not_a_silent_drop() {
        let root = tmpdir("missing");
        let out = expand("@khong-ton-tai.md", Some(&root), &[]);
        assert!(out.files.is_empty());
        assert_eq!(out.warnings.len(), 1);
        assert!(out.prompt.contains("không đọc được"));
    }

    #[test]
    fn same_file_mentioned_twice_is_inlined_once() {
        let root = tmpdir("dupe");
        fs::write(root.join("a.md"), "one").unwrap();
        let out = expand("@a.md rồi @a.md", Some(&root), &[]);
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.prompt.matches("<attached-file").count(), 1);
    }
}
