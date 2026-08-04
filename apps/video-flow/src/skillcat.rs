//! Playbook skill catalog — port of `internal/skill/catalog.go`. Scans the
//! playbooks dir for `*.md`, parsing YAML frontmatter `name`/`description`.

use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Debug, Default)]
pub struct PlaybookSkill {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Phrases that should pull this playbook in automatically. Without them a
    /// skill only helps someone who already knows it exists.
    #[serde(default)]
    pub triggers: Vec<String>,
    pub body: String,
}

/// Read all `*.md` files in `dir` and return parsed skill metadata.
pub fn scan(dir: &Path) -> Result<Vec<PlaybookSkill>, String> {
    let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut names: Vec<std::path::PathBuf> = entries
        .flatten()
        .filter(|e| e.path().is_file() && e.file_name().to_string_lossy().ends_with(".md"))
        .map(|e| e.path())
        .collect();
    names.sort();
    let mut skills = Vec::new();
    for path in names {
        if let Some(s) = parse_file(&path) {
            skills.push(s);
        }
    }
    Ok(skills)
}

fn parse_file(path: &Path) -> Option<PlaybookSkill> {
    let content = std::fs::read_to_string(path).ok()?;
    let id = path.file_stem()?.to_string_lossy().to_string();
    let mut s = PlaybookSkill {
        id: id.clone(),
        body: content.clone(),
        ..Default::default()
    };
    if content.starts_with("---") {
        let (name, description, triggers) = parse_frontmatter(&content);
        s.name = name;
        s.description = description;
        s.triggers = triggers;
        s.body = strip_frontmatter_body(&content);
    }
    if s.name.is_empty() {
        s.name = id;
    }
    Some(s)
}

/// Remove the `---` frontmatter block; return the remaining body trimmed.
fn strip_frontmatter_body(content: &str) -> String {
    if !content.starts_with("---") {
        return content.to_string();
    }
    let rest = &content[3..];
    match rest.find("\n---") {
        Some(idx) => rest[idx + 4..].trim().to_string(),
        None => content.to_string(),
    }
}

/// Extract `name` / `description` / `triggers` from the YAML frontmatter.
///
/// Hand-rolled rather than a YAML dep: the shapes here are a scalar and a
/// `- item` list, and the frontmatter is authored alongside this parser.
fn parse_frontmatter(content: &str) -> (String, String, Vec<String>) {
    let mut name = String::new();
    let mut description = String::new();
    let mut triggers: Vec<String> = Vec::new();
    let mut in_front = false;
    let mut in_triggers = false;
    for line in content.lines() {
        if line.trim() == "---" {
            if !in_front {
                in_front = true;
                continue;
            }
            break;
        }
        if !in_front {
            continue;
        }
        let trimmed = line.trim();
        if in_triggers {
            if let Some(item) = trimmed.strip_prefix("- ") {
                let t = item.trim().trim_matches('"').trim_matches('\'').to_string();
                if !t.is_empty() {
                    triggers.push(t);
                }
                continue;
            }
            // A non-list line ends the block.
            in_triggers = false;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            match k.trim() {
                "name" => name = v.trim().to_string(),
                "description" => description = v.trim().to_string(),
                "triggers" => in_triggers = v.trim().is_empty(),
                _ => {}
            }
        }
    }
    (name, description, triggers)
}

/// Playbooks whose triggers match `text`, best match first.
///
/// Substring matching alone is too brittle for how people actually type: real
/// queries were "khoong xem dk video" (doubled letter, abbreviation) and "ghép
/// clip lại thành 1 video" (trigger words present but not adjacent). So a
/// trigger also matches when all of its content words appear anywhere in the
/// text — order and filler words ignored.
pub fn match_playbooks(skills: &[PlaybookSkill], text: &str) -> Vec<PlaybookSkill> {
    let hay = normalize_for_match(text);
    if hay.is_empty() {
        return Vec::new();
    }
    let hay_tokens: Vec<&str> = hay.split(' ').collect();
    let mut hits: Vec<(usize, PlaybookSkill)> = Vec::new();
    for s in skills {
        let best = s
            .triggers
            .iter()
            .filter_map(|t| trigger_score(&normalize_for_match(t), &hay, &hay_tokens))
            .max();
        if let Some(score) = best {
            hits.push((score, s.clone()));
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    hits.into_iter().map(|(_, s)| s).collect()
}

/// Filler words that must not carry a match on their own.
const STOPWORDS: &[&str] = &[
    "duoc", "cho", "lai", "thanh", "cua", "va", "voi", "de", "khi", "cai", "mot", "toi", "ban",
];

/// Match strength of one trigger, or `None` when it does not apply.
/// A contiguous hit outranks a scattered one.
fn trigger_score(trigger: &str, hay: &str, hay_tokens: &[&str]) -> Option<usize> {
    if trigger.is_empty() {
        return None;
    }
    if hay.contains(trigger) {
        return Some(trigger.len() + 100);
    }
    let content: Vec<&str> = trigger
        .split(' ')
        .filter(|w| w.len() >= 2 && !STOPWORDS.contains(w))
        .collect();
    // A single generic word ("video") must not pull a playbook in on its own.
    if content.len() < 2 {
        return None;
    }
    if content.iter().all(|w| hay_tokens.contains(w)) {
        Some(trigger.len())
    } else {
        None
    }
}

/// Lowercase, strip Vietnamese diacritics, collapse doubled letters and
/// whitespace. Doubling is folded because "khoong"/"khong" are the same word to
/// everyone except a substring matcher.
fn normalize_for_match(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut prev: Option<char> = None;
    for ch in lower.chars() {
        let c = fold_vi(ch);
        if c.is_whitespace() {
            if prev != Some(' ') && prev.is_some() {
                out.push(' ');
                prev = Some(' ');
            }
            continue;
        }
        if prev == Some(c) {
            continue; // collapse a doubled letter
        }
        out.push(c);
        prev = Some(c);
    }
    out.trim().to_string()
}

fn fold_vi(c: char) -> char {
    const TABLE: &[(&str, char)] = &[
        ("àáạảãâầấậẩẫăằắặẳẵ", 'a'),
        ("èéẹẻẽêềếệểễ", 'e'),
        ("ìíịỉĩ", 'i'),
        ("òóọỏõôồốộổỗơờớợởỡ", 'o'),
        ("ùúụủũưừứựửữ", 'u'),
        ("ỳýỵỷỹ", 'y'),
        ("đ", 'd'),
    ];
    for (set, base) in TABLE {
        if set.chars().any(|x| x == c) {
            return *base;
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trigger_list() {
        let (n, d, t) = parse_frontmatter(
            "---\nname: refresh-urls\ndescription: lấy lại link\ntriggers:\n  - không xem được video\n  - mất hình\n---\nbody",
        );
        assert_eq!(n, "refresh-urls");
        assert_eq!(d, "lấy lại link");
        assert_eq!(t, vec!["không xem được video", "mất hình"]);
        // No triggers block is fine.
        let (_, _, none) = parse_frontmatter("---\nname: x\n---\nbody");
        assert!(none.is_empty());
    }

    /// Real queries from use: a doubled letter plus an abbreviation, and
    /// trigger words split apart by filler. Substring matching missed both.
    #[test]
    fn matches_how_people_actually_type() {
        let skills = vec![
            PlaybookSkill {
                id: "refresh-urls".into(),
                triggers: vec!["không xem được video".into()],
                ..Default::default()
            },
            PlaybookSkill {
                id: "concat".into(),
                triggers: vec!["ghép video".into()],
                ..Default::default()
            },
        ];
        // doubled letter + "dk" abbreviation, no diacritics
        assert_eq!(
            match_playbooks(&skills, "khoong xem dk video")[0].id,
            "refresh-urls"
        );
        // trigger words present but separated by filler
        assert_eq!(
            match_playbooks(&skills, "ghép clip lại thành 1 video")[0].id,
            "concat"
        );
        assert!(match_playbooks(&skills, "hôm nay trời đẹp").is_empty());
    }

    /// One generic word must not drag a playbook in.
    #[test]
    fn single_common_word_does_not_match() {
        let skills = vec![PlaybookSkill {
            id: "gen-videos".into(),
            triggers: vec!["sinh video".into()],
            ..Default::default()
        }];
        assert!(match_playbooks(&skills, "cái video này đẹp").is_empty());
        assert_eq!(
            match_playbooks(&skills, "sinh video giúp tôi")[0].id,
            "gen-videos"
        );
    }

    /// Vietnamese gets typed both ways — a trigger must fire with or without
    /// diacritics, otherwise it only helps people who type carefully.
    #[test]
    fn matches_with_and_without_diacritics() {
        let skills = vec![
            PlaybookSkill {
                id: "refresh-urls".into(),
                name: "refresh".into(),
                triggers: vec!["không xem được video".into(), "mất hình".into()],
                ..Default::default()
            },
            PlaybookSkill {
                id: "gen-videos".into(),
                name: "gen".into(),
                triggers: vec!["sinh video".into()],
                ..Default::default()
            },
        ];
        let hit = match_playbooks(&skills, "Sao tôi KHONG XEM DUOC VIDEO vậy?");
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, "refresh-urls");

        let hit = match_playbooks(&skills, "giúp tôi sinh video cho project");
        assert_eq!(hit[0].id, "gen-videos");

        assert!(match_playbooks(&skills, "hôm nay trời đẹp").is_empty());
    }

    /// The more specific trigger wins when several match.
    #[test]
    fn longer_trigger_ranks_first() {
        let skills = vec![
            PlaybookSkill {
                id: "broad".into(),
                triggers: vec!["video".into()],
                ..Default::default()
            },
            PlaybookSkill {
                id: "specific".into(),
                triggers: vec!["không xem được video".into()],
                ..Default::default()
            },
        ];
        let hit = match_playbooks(&skills, "không xem được video");
        assert_eq!(hit[0].id, "specific");
        assert_eq!(hit.len(), 2);
    }

    #[test]
    fn frontmatter_parsed() {
        let (n, d, _) =
            parse_frontmatter("---\nname: Gen Images\ndescription: makes: images\n---\nbody");
        assert_eq!(n, "Gen Images");
        assert_eq!(d, "makes: images");
        assert_eq!(
            strip_frontmatter_body("---\nname: x\n---\n\nBody here"),
            "Body here"
        );
    }

    #[test]
    fn scan_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("a.md"),
            "---\nname: A\ndescription: first\n---\nHello",
        )
        .unwrap();
        std::fs::write(dir.path().join("b.md"), "no frontmatter").unwrap();
        std::fs::write(dir.path().join("c.txt"), "ignored").unwrap();
        let skills = scan(dir.path()).unwrap();
        assert_eq!(skills.len(), 2);
        assert_eq!(skills[0].id, "a");
        assert_eq!(skills[0].name, "A");
        assert_eq!(skills[0].description, "first");
        assert_eq!(skills[0].body, "Hello");
        assert_eq!(skills[1].name, "b");
        assert_eq!(skills[1].body, "no frontmatter");
    }
}
