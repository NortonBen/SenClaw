//! Handing a finished rewrite to the video pipeline.
//!
//! The target is `apps/video-flow`, whose `vf_pipeline_create(mode="production")`
//! and `POST /api/script/parse` both take **screenplay markdown**: a flat list of
//! `# Cảnh N` headings, each followed by that scene's prose. Its parser splits on
//! those headings and turns one block into one scene, so the heading layout here
//! is the actual interface contract — not a formatting preference.
//!
//! Scenes are cut with the same Vietnamese hybrid splitter the rewrite pipeline
//! uses, at a much smaller size. That splitter already breaks at narrative
//! shifts, which is exactly where a scene should end.
//!
//! Note the apps cannot call each other directly: the daemon bridge's `mcp.call`
//! and `space.rest` actions are declared but not implemented, so the handoff goes
//! through a file or through an agent that holds both MCP servers.

use serde::Serialize;

use crate::text;

/// Default characters per scene. At roughly 8 seconds of video per scene this
/// keeps a scene's prose to about what a shot can carry.
pub const DEFAULT_SCENE_CHARS: usize = 900;

#[derive(Debug, Clone, Serialize)]
pub struct Scene {
    /// 1-based, matching video-flow's `display_order`.
    pub index: usize,
    pub heading: String,
    pub text: String,
    pub chars: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportBundle {
    pub story_id: i64,
    pub name: String,
    pub source_type: String,
    pub version_number: i64,
    pub total_chars: usize,
    pub total_scenes: usize,
    pub scene_chars: usize,
    pub scenes: Vec<Scene>,
}

/// A short scene title taken from the opening of the scene itself.
///
/// video-flow reads the block body, not the heading, so this is for the human
/// skimming the file — but a screenplay of forty `# Cảnh N` lines is unusable.
fn heading_for(text_body: &str) -> String {
    let first = text_body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");

    // Prefer ending at a sentence break so the title is a clause, not a fragment.
    let sentence_end = first
        .char_indices()
        .find(|(_, c)| matches!(c, '.' | '!' | '?' | '…'))
        .map(|(i, c)| i + c.len_utf8());
    let candidate = match sentence_end {
        Some(end) if first[..end].chars().count() <= 70 => &first[..end],
        _ => first,
    };

    let mut out = String::new();
    for word in candidate.split_whitespace() {
        if out.chars().count() + word.chars().count() + 1 > 60 {
            out.push('…');
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out.trim_end_matches(['.', ',', ';', ':']).to_string()
}

/// Split a story into scene-sized blocks.
pub fn build_scenes(story_text: &str, scene_chars: usize) -> Vec<Scene> {
    let scene_chars = scene_chars.clamp(200, 5000);
    // A low similarity threshold keeps a scene together unless the topic really
    // shifts; the size bound does most of the work at this scale.
    let min = (scene_chars * 3 / 5).max(100);
    text::hybrid_split(story_text, min, scene_chars, 0.15)
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .enumerate()
        .map(|(i, body)| Scene {
            index: i + 1,
            heading: heading_for(&body),
            chars: body.chars().count(),
            text: body,
        })
        .collect()
}

pub fn bundle(
    story_id: i64,
    name: &str,
    source_type: &str,
    version_number: i64,
    story_text: &str,
    scene_chars: usize,
) -> ExportBundle {
    let scenes = build_scenes(story_text, scene_chars);
    ExportBundle {
        story_id,
        name: name.to_string(),
        source_type: source_type.to_string(),
        version_number,
        total_chars: story_text.chars().count(),
        total_scenes: scenes.len(),
        scene_chars,
        scenes,
    }
}

/// Screenplay markdown — the format `vf_pipeline_create` consumes.
///
/// Metadata rides in an HTML comment rather than YAML front-matter: video-flow
/// splits the document on markdown headings, and a front-matter block would be
/// swept into the first scene.
pub fn to_screenplay(b: &ExportBundle) -> String {
    let mut out = String::with_capacity(b.total_chars + b.total_scenes * 64);
    out.push_str(&format!(
        "<!-- rewrite-story export | story_id={} | {} | {} cảnh | {} ký tự -->\n\n",
        b.story_id, b.name, b.total_scenes, b.total_chars
    ));
    for s in &b.scenes {
        if s.heading.is_empty() {
            out.push_str(&format!("# Cảnh {}\n\n", s.index));
        } else {
            out.push_str(&format!("# Cảnh {} — {}\n\n", s.index, s.heading));
        }
        out.push_str(s.text.trim());
        out.push_str("\n\n");
    }
    out
}

/// Human-readable document (not for the parser).
pub fn to_markdown(b: &ExportBundle) -> String {
    let kind = if b.source_type == "ai" {
        format!("bản viết lại v{}", b.version_number)
    } else {
        "bản gốc".to_string()
    };
    let mut out = format!(
        "# {}\n\n> {} · {} ký tự · {} cảnh\n\n",
        b.name, kind, b.total_chars, b.total_scenes
    );
    for s in &b.scenes {
        out.push_str(&format!("## Cảnh {} — {}\n\n{}\n\n", s.index, s.heading, s.text.trim()));
    }
    out
}

/// Folds a Vietnamese letter to its ASCII base, leaving anything else alone.
fn fold_vietnamese(c: char) -> Option<char> {
    const TABLE: [(&str, char); 12] = [
        ("àáảãạăắằẳẵặâấầẩẫậ", 'a'),
        ("èéẻẽẹêếềểễệ", 'e'),
        ("ìíỉĩị", 'i'),
        ("òóỏõọôốồổỗộơớờởỡợ", 'o'),
        ("ùúủũụưứừửữự", 'u'),
        ("ỳýỷỹỵ", 'y'),
        ("đ", 'd'),
        ("ÀÁẢÃẠĂẮẰẲẴẶÂẤẦẨẪẬ", 'a'),
        ("ÈÉẺẼẸÊẾỀỂỄỆ", 'e'),
        ("ÌÍỈĨỊ", 'i'),
        ("ÒÓỎÕỌÔỐỒỔỖỘƠỚỜỞỠỢ", 'o'),
        ("ÙÚỦŨỤƯỨỪỬỮỰ", 'u'),
    ];
    for (set, base) in TABLE {
        if set.contains(c) {
            return Some(base);
        }
    }
    match c {
        'Ỳ' | 'Ý' | 'Ỷ' | 'Ỹ' | 'Ỵ' => Some('y'),
        'Đ' => Some('d'),
        _ => None,
    }
}

/// Filename-safe slug.
///
/// Vietnamese is transliterated rather than stripped — dropping the diacritics
/// outright turns "Truyện Kiều" into "truyn-kiu".
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if let Some(base) = fold_vietnamese(c) {
            out.push(base);
        } else if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let s = out.trim_matches('-').to_string();
    if s.is_empty() {
        "truyen".to_string()
    } else {
        s.chars().take(60).collect::<String>().trim_matches('-').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        (1..=30)
            .map(|i| format!("Đoạn {i}. Nhân vật chính bước vào khu rừng và nhìn thấy một điều kỳ lạ đang chờ đợi phía trước."))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn scenes_are_numbered_from_one_and_bounded() {
        let scenes = build_scenes(&sample(), 400);

        assert!(scenes.len() > 1);
        assert_eq!(scenes[0].index, 1);
        assert_eq!(scenes.last().unwrap().index, scenes.len());
        for s in &scenes {
            assert!(s.chars <= 400, "scene {} is {} chars", s.index, s.chars);
            assert!(!s.text.trim().is_empty());
        }
    }

    /// The heading layout is video-flow's parsing contract: it splits on `#`
    /// headings and makes one scene per block.
    #[test]
    fn screenplay_emits_one_h1_per_scene() {
        let b = bundle(7, "Truyện thử", "ai", 2, &sample(), 400);
        let md = to_screenplay(&b);

        let h1 = md.lines().filter(|l| l.starts_with("# ")).count();
        assert_eq!(h1, b.total_scenes);
        assert!(md.starts_with("<!--"), "metadata must not be YAML front-matter");
        assert!(md.contains("# Cảnh 1 — "));
    }

    #[test]
    fn export_preserves_every_scene_body() {
        let b = bundle(1, "T", "ai", 1, &sample(), 400);
        let md = to_screenplay(&b);
        for s in &b.scenes {
            let first_line = s.text.lines().next().unwrap();
            assert!(md.contains(first_line), "scene {} missing from export", s.index);
        }
    }

    #[test]
    fn headings_stay_short_and_are_derived_from_the_text() {
        let b = bundle(1, "T", "ai", 1, &sample(), 400);
        for s in &b.scenes {
            assert!(s.heading.chars().count() <= 61, "heading too long: {}", s.heading);
            assert!(!s.heading.contains('\n'));
        }
    }

    #[test]
    fn slug_transliterates_vietnamese_instead_of_stripping_it() {
        assert_eq!(slug("Truyện Kiều — bản mới!"), "truyen-kieu-ban-moi");
        assert_eq!(slug("Đường về cố hương"), "duong-ve-co-huong");
        assert_eq!(slug("!!!"), "truyen");
        assert!(slug(&"a".repeat(200)).chars().count() <= 60);
        // Never leave a trailing separator, even when truncation lands on one.
        assert!(!slug(&format!("{} x", "a".repeat(59))).ends_with('-'));
    }

    #[test]
    fn empty_story_yields_no_scenes_rather_than_one_empty_scene() {
        let b = bundle(1, "T", "human", 1, "   \n\n  ", 400);
        assert_eq!(b.total_scenes, 0);
        assert!(!to_screenplay(&b).contains("# Cảnh"));
    }
}
